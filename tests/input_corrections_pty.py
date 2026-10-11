"""Regression for corrected counted input; run after cargo build on Unix."""
import fcntl
import os
from pathlib import Path
import pty
import select
import time
import struct
import subprocess
import tempfile
import termios

def receive(master, marker):
    data = b""
    deadline = time.monotonic() + 5
    while marker not in data and time.monotonic() < deadline:
        if select.select([master], [], [], 0.1)[0]:
            data += os.read(master, 65536)
    assert marker in data, (marker, data)
    return data

BINARY = Path(__file__).resolve().parents[1] / "target/debug/vir"


def saved_input(directory, label, original, setup, keys, expected):
    path = directory / label
    path.write_text(original)
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 100, 0, 0))
    attributes = termios.tcgetattr(slave)
    process = subprocess.Popen(
        [str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave,
        stderr=subprocess.PIPE, env=dict(os.environ, VIR_RECOVERY_DIR=str(directory)),
    )
    try:
        receive(master, b"[VIEW]")
        if setup:
            os.write(master, setup)
            receive(master, b"[VIEW]")
        os.write(master, keys)
        receive(master, b"[VIEW] [+]")
        os.write(master, b":wq\r")
        receive(master, b"\x1b[?1049l")
        assert process.wait(timeout=5) == 0
        assert process.stderr.read() == b""
        assert path.read_text() == expected, (label, repr(path.read_text()), repr(expected))
        assert termios.tcgetattr(slave) == attributes
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def rejected_change(directory, label, original, keys):
    path = directory / label
    path.write_text(original)
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 100, 0, 0))
    attributes = termios.tcgetattr(slave)
    process = subprocess.Popen(
        [str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave,
        stderr=subprocess.PIPE, env=dict(os.environ, VIR_RECOVERY_DIR=str(directory)),
    )
    try:
        receive(master, b"[VIEW]")
        os.write(master, keys)
        receive(master, b"invalid register")
        # Rejection must stay in Normal so the next i starts an independent edit.
        os.write(master, b"iX\x1b")
        receive(master, b"[VIEW] [+]")
        os.write(master, b":wq\r")
        receive(master, b"\x1b[?1049l")
        assert process.wait(timeout=5) == 0
        assert process.stderr.read() == b""
        assert path.read_text() == "X\n", (label, repr(path.read_text()))
        assert termios.tcgetattr(slave) == attributes
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def undo_after_kill(directory, label, original, column, count):
    path = directory / label
    path.write_text(original)
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 100, 0, 0))
    attributes = termios.tcgetattr(slave)
    process = subprocess.Popen(
        [str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave,
        stderr=subprocess.PIPE, env=dict(os.environ, VIR_RECOVERY_DIR=str(directory)),
    )
    try:
        receive(master, b"[VIEW]")
        os.write(master, f"{column}l{count}i".encode() + b"\x15X\x1b")
        receive(master, b"[VIEW] [+]")
        os.write(master, b"u")
        receive(master, b"col 1")
        os.write(master, b"rZ")
        receive(master, b"[VIEW] [+]")
        os.write(master, b":wq\r")
        receive(master, b"\x1b[?1049l")
        assert process.wait(timeout=5) == 0
        assert process.stderr.read() == b""
        assert path.read_text() == "Z" + original[1:], (label, repr(path.read_text()))
        assert termios.tcgetattr(slave) == attributes
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="vir-input-regression-") as name:
        directory = Path(name)
        for original, column, label in [("old tail\n", 4, "ascii"),
                                        ("中🙂 tail\n", 3, "unicode")]:
            for count in [1, 3]:
                undo_after_kill(directory, f"kill-undo-{label}-{count}",
                                original, column, count)
        for original, label in [("", "empty"), ("\n", "blank")]:
            for command in ["cc", "S", "C"]:
                rejected_change(directory, f"invalid-{label}-{command}", original,
                                f'"1{command}'.encode())
        for label, keys, expected in [
            ("ctrl-u", b"3iabc\x15X\x1b", "XXXtail\n"),
            ("ctrl-w", b"3ione two\x17X\x1b", "one Xone Xone Xtail\n"),
            ("ctrl-t", b"3i \x14X\x1b", "\tX\tX\tXtail\n"),
            ("ctrl-d", b"3i \x14\x04X\x1b", "XXXtail\n"),
            ("ctrl-u-lf", b"3iabc\x15X\rY\x1b", "XXX\nYtail\n"),
        ]:
            saved_input(directory, label, "tail\n", b"", keys, expected)
        for count in [1, 2, 3]:
            saved_input(directory, f"replace-{count}", "a\n", b":set ai\r",
                        f"{count}RX\rY\x1b".encode(), "X" * count + "\nY\n")
    print("counted input corrections PTY passed")
