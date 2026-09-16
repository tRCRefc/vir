"""Linux/Unix PTY acceptance: run after cargo build, using only Python stdlib."""
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
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
    return data


def session(path, quit_key):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 60, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        data = receive(master, b"[VIEW]")
        assert b"\x1b[?1049h" in data
        active = termios.tcgetattr(slave)
        assert not active[3] & termios.ICANON
        assert not active[3] & termios.ECHO
        os.write(master, b"G")
        receive(master, b"LAST-LINE")
        os.write(master, b"gllj")
        receive(master, b"FIRST-LINE")
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 4, 40, 0, 0))
        os.kill(process.pid, signal.SIGWINCH)
        receive(master, b"[VIEW]")
        os.write(master, quit_key)
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


with tempfile.TemporaryDirectory(prefix="vir-visual-") as directory:
    path = Path(directory) / "text"
    content = "FIRST-LINE\n" + "中\ttext\n" * 20 + "LAST-LINE\n"
    path.write_text(content)
    for quit_key in [b"q", b"\x03"]:
        session(path, quit_key)
        assert path.read_text() == content
print("PTY: browse, resize, q/Ctrl-C, terminal restoration and unchanged file passed")

# Exercise RAII cleanup on injected errors and unwinding panics in a real PTY.
master, slave = pty.openpty()
original = termios.tcgetattr(slave)
process = subprocess.Popen(
    ["cargo", "test", "--offline", "--locked", "--lib", "restores_raw_mode_after_error_and_panic", "--", "--ignored", "--nocapture"],
    cwd=BINARY.parents[2], stdin=slave, stdout=slave, stderr=slave,
)
try:
    receive(master, b"test result: ok")
    assert process.wait(timeout=5) == 0
    assert termios.tcgetattr(slave) == original
finally:
    if process.poll() is None:
        process.kill()
        process.wait()
    os.close(master)
    os.close(slave)
print("PTY: injected error and panic restoration passed")
