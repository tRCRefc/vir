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
        receive(master, b"\x1b[4;1H[VIEW]")
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


def command_session(path, quit_command):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 100, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    export = path.parent / "中文 export"
    try:
        receive(master, b"[VIEW]")
        os.write(master, b"/LAST-LINE\r")
        receive(master, b"22/22  col 1")
        os.write(master, b"?FIRST-LINE\r")
        receive(master, b"1/22  col 1")
        os.write(master, b"/missing\r")
        receive(master, b"? pattern not found")
        os.write(master, b"/\r")
        receive(master, b"1/22  col 1")
        os.write(master, b"nN")
        receive(master, b"1/22  col 1")
        os.write(master, b"/ignored")
        receive(master, b"/ignored")
        os.write(master, b"\x03")
        receive(master, b"[VIEW]")
        os.write(master, b":a\r")
        receive(master, b"? visual supports only")
        os.write(master, f":w {path.parent}\r".encode())
        receive(master, b"? ")
        assert process.poll() is None
        os.write(master, b":w")
        receive(master, b"\x1b[8;1H:w\x1b[")
        os.write(master, b"\x1b")
        receive(master, b"[VIEW]")
        # Resize during input and cancel with Ctrl-C without exiting.
        os.write(master, b":w")
        receive(master, b"\x1b[8;1H:w\x1b[")
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 4, 20, 0, 0))
        os.kill(process.pid, signal.SIGWINCH)
        receive(master, b"\x1b[4;1H:w\x1b[")
        os.write(master, b"\x03")
        receive(master, b"[VIEW]")
        assert process.poll() is None
        # Unicode path plus a typo removed using Backspace.
        os.write(master, f":w {export}X".encode() + b"\x7f\r")
        receive(master, b"Written")
        assert export.read_bytes() == path.read_bytes()
        os.write(master, quit_command)
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
        export.unlink(missing_ok=True)


def editing_session(directory):
    path = directory / "editing"
    path.write_text("FIRST\nMIDDLE\nLAST\n")
    export = directory / "renamed"
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 120, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/3  col 1")
        os.write(master, b"G:2,$d\r")
        receive(master, b"1/1  col 1")
        os.write(master, b":q\r")
        receive(master, b"? unsaved changes")
        assert process.poll() is None
        os.write(master, b":u\r")
        receive(master, b"3/3  col 1")
        os.write(master, b":s/LAST/changed/\r")
        data = receive(master, b"3/3  col 1")
        assert b"changed" in data and b"[+]" in data
        os.write(master, f":e {path}\r".encode())
        receive(master, b"? unsaved changes")
        os.write(master, f":e! {path}\r".encode())
        data = receive(master, b"3/3  col 1")
        assert b"LAST" in data and b"[VIEW] [+]" not in data
        os.write(master, f":f {export}\r".encode())
        receive(master, b"line 3 of 3")
        os.write(master, b":%d\r")
        receive(master, b"0/0  col 1")
        os.write(master, b":u\r")
        receive(master, b"3/3  col 1")
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert export.read_bytes() == path.read_bytes()
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


with tempfile.TemporaryDirectory(prefix="vir-visual-") as directory:
    editing_session(Path(directory))
    path = Path(directory) / "text"
    content = "FIRST-LINE\n" + "中\ttext\n" * 20 + "LAST-LINE\n"
    path.write_text(content)
    for quit_key in [b"q", b"\x03"]:
        session(path, quit_key)
        assert path.read_text() == content
    for quit_command in [b":q\r", b":q!\r", b":wq\r"]:
        command_session(path, quit_command)
        assert path.read_text() == content
print("PTY: browse, editing/undo/load/save/error recovery, resize, exits and terminal restoration passed")

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
