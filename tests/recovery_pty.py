"""Unix process-kill recovery acceptance. Run after cargo build."""
import fcntl
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import tempfile
import termios
import time

BINARY = Path(__file__).resolve().parents[1] / "target/debug/vir"


def receive(master, marker):
    data = b""
    deadline = time.monotonic() + 5
    while marker not in data and time.monotonic() < deadline:
        ready, _, _ = select.select([master], [], [], 0.1)
        if ready:
            data += os.read(master, 65536)
    assert marker in data, (marker, data)


def environment(directory):
    return dict(os.environ, VIR_RECOVERY_DIR=str(directory))


def restore(snapshot, destination, directory, expected):
    result = subprocess.run(
        [str(BINARY), "--ex", "--recover", str(snapshot)],
        input=f"w {destination}\nq\n", text=True, capture_output=True,
        env=environment(directory), timeout=5,
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout == "", result.stdout
    assert destination.read_bytes() == expected
    assert snapshot.exists()  # Explicit source snapshot is never consumed implicitly.
    listing = subprocess.run([str(BINARY), "--recover-list", str(directory)], capture_output=True, text=True, timeout=5)
    assert listing.returncode == 0 and snapshot.name in listing.stdout


def visual_crash(directory, mode, expected, marker):
    path = directory / "original"
    path.write_text("alpha beta\n")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 7, 150, 0, 0))
    process = subprocess.Popen(
        [str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave,
        stderr=subprocess.PIPE, env=environment(directory),
    )
    try:
        receive(master, b"1/1  col 1")
        os.write(master, mode)
        receive(master, marker)  # Render follows durable checkpoint, no timing guess.
        process.kill()  # SIGKILL: no destructor or signal handler can flush text.
        process.wait(timeout=5)
        assert path.read_text() == "alpha beta\n"
        snapshots = list(directory.glob(".vir-recovery-*"))
        assert len(snapshots) == 1
        # External changes after the crash must never be overwritten by recovery.
        path.write_text("external after crash\n")
        restore(snapshots[0], directory / "restored", directory, expected)
        assert path.read_text() == "external after crash\n"
        assert len(list(directory.glob(".vir-recovery-*"))) == 1
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def ex_crash(directory):
    path = directory / "original"
    path.write_text("old\n")
    process = subprocess.Popen(
        [str(BINARY), "--ex", str(path)], stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment(directory),
    )
    try:
        process.stdin.write(b"c\ncomplete draft\n")
        process.stdin.flush()
        # ex intentionally has no output while collecting lines. Poll only for a
        # complete atomically published record, with a bounded deadline.
        deadline = time.monotonic() + 5
        snapshot = None
        while time.monotonic() < deadline:
            candidates = list(directory.glob(".vir-recovery-*"))
            if candidates:
                data = candidates[0].read_bytes()
                if b"complete draft\n" in data:
                    snapshot = candidates[0]
                    break
            select.select([], [], [], 0.01)
        assert snapshot is not None
        process.kill()
        process.wait(timeout=5)
        restore(snapshot, directory / "restored", directory, b"complete draft\n")
        assert path.read_text() == "old\n"
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()


def normal_cleanup(directory):
    path = directory / "normal"
    path.write_text("old\n")
    for script in ["a\nnew\n.\nw\nq\n", "a\ndiscard\n.\nq!\n", "a\neof\n"]:
        result = subprocess.run([str(BINARY), "--ex", str(path)], input=script,
                                text=True, capture_output=True, env=environment(directory), timeout=5)
        assert result.returncode == 0, result.stderr
        assert not list(directory.glob(".vir-recovery-*"))
    assert path.read_text() == "old\nnew\n"


def visual_recovery_and_cleanup(directory):
    # Manufacture a real crash snapshot, then verify the visual recovery entry.
    visual_crash(directory, b"iZ", b"Zalpha beta\n", b"Zalpha beta")
    snapshot = next(directory.glob(".vir-recovery-*"))
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 7, 150, 0, 0))
    process = subprocess.Popen([str(BINARY), "--vi", "--recover", str(snapshot)],
                               stdin=slave, stdout=slave, stderr=subprocess.PIPE, env=environment(directory))
    try:
        receive(master, b"[+]")
        assert len(list(directory.glob(".vir-recovery-*"))) == 2
        os.write(master, b":q!\r")
        assert process.wait(timeout=5) == 0
        assert list(directory.glob(".vir-recovery-*")) == [snapshot]
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


with tempfile.TemporaryDirectory(prefix="vir-recovery-test-") as temporary:
    root = Path(temporary)
    for index, (keys, expected, marker) in enumerate([
        ("i中".encode(), "中alpha beta\n".encode(), "中alpha beta".encode()),
        (b"3iX", b"Xalpha beta\n", b"Xalpha beta"),
        (b"3iX\rY", b"XXX\nYalpha beta\n", b"Yalpha beta"),
        (b"3oX\rY", b"alpha beta\nX\nY\n", b"Y"),
        (b"3OZ", b"Z\nalpha beta\n", b"col 2"),
        (b"3RXY", b"XYpha beta\n", b"XYpha beta"),
        (b"3RX\rY", b"XXX\nYpha beta\n", b"Ypha beta"),
        (b"i\x16\x03", b"\x03alpha beta\n", b"^Calpha beta"),
        (b"R\x16\x08", b"\x08lpha beta\n", b"^Hlpha beta"),
        (b"iX\x16", b"Xalpha beta\n", b"quote: type one character"),
        (b"3r\r", b"\nha beta\n", b"2/2  col 1"),
        (b"2r\x16\x03", b"\x03\x03pha beta\n", b"^C^Cpha beta"),
        (b"rXlrYUiZ", b"Zalpha beta\n", b"Zalpha beta"),
        (b"RXY", b"XYpha beta\n", b"XYpha beta"),
        (b"cwnew", b"new beta\n", b"new beta"),
        (b"dw", b"beta\n", b"[+]"),
    ]):
        directory = root / str(index)
        directory.mkdir()
        visual_crash(directory, keys, expected, marker)
    for name, action in [("ex", ex_crash), ("cleanup", normal_cleanup), ("visual", visual_recovery_and_cleanup)]:
        directory = root / name
        directory.mkdir()
        action(directory)
print("Recovery: SIGKILL in Insert/Replace/change/normal/ex draft, safe reopen and cleanup passed")
