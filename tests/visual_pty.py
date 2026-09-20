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
        os.write(master, b"10j")
        receive(master, b"11/22  col 1")
        os.write(master, b"2l")
        receive(master, b"11/22  col 3")
        os.write(master, b"2$")
        receive(master, b"12/22  col 6")
        os.write(master, b"5G")
        receive(master, b"5/22  col 1")
        os.write(master, b"9\x1b")
        receive(master, b"5/22  col 1")
        os.write(master, b"j")
        receive(master, b"6/22  col 1")
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


def insert_session(directory):
    path = directory / "inserted"
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 100, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"[VIEW]")
        os.write(master, "i中X".encode())
        receive(master, b"col 3")
        os.write(master, b"\x7f\rsecond")
        receive(master, b"2/2  col 7")
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 4, 30, 0, 0))
        os.kill(process.pid, signal.SIGWINCH)
        receive(master, b"\x1b[4;1H[INSERT]")
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 100, 0, 0))
        os.kill(process.pid, signal.SIGWINCH)
        receive(master, b"\x1b[6;1H[INSERT]")
        os.write(master, b"\x1b")
        receive(master, b"[VIEW] [+]")
        os.write(master, b"u")
        receive(master, b"0/0  col 1")
        os.write(master, b"u")
        receive(master, b"[VIEW] [+]")
        os.write(master, b"Ga!\x03")
        receive(master, b"[VIEW] [+]")
        assert process.poll() is None
        os.write(master, b"oTHIRD")
        receive(master, b"3/3  col 6")
        os.write(master, b"\x1b")
        receive(master, b"[VIEW] [+]")
        os.write(master, b"x")
        receive(master, b"THIR")
        os.write(master, b":q\r")
        receive(master, b"? unsaved changes")
        os.write(master, f":w {directory}\r".encode())
        receive(master, b"? ")
        assert process.poll() is None
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == "中\ns!econd\nTHIR\n", path.read_text()
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def normal_actions_session(directory):
    path = directory / "normal-actions"
    path.write_text(" \tfirst\n" + "line\n" * 10 + "last\n")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 120, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/12  col 1")
        os.write(master, b"I!\x1b")
        receive(master, b"[VIEW] [+]")
        os.write(master, b"A?\x1b")
        receive(master, b"[VIEW] [+]")
        os.write(master, b"Oabove\x1b")
        receive(master, b"1/13  col 5")
        os.write(master, b"10dd")
        receive(master, b"1/3  col 1")
        os.write(master, b"u")
        receive(master, b"1/13  col 1")
        os.write(master, b"2d3d")
        receive(master, b"1/7  col 1")
        os.write(master, b"u")
        receive(master, b"1/13  col 1")
        os.write(master, b"9d\x1b")
        receive(master, b"1/13  col 1")
        os.write(master, b"dq")
        receive(master, b"command cancelled")
        assert process.poll() is None
        os.write(master, b"dd")
        receive(master, b"1/12  col 1")
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == " \t!first?\n" + "line\n" * 10 + "last\n"
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def register_session(directory):
    path = directory / "registers"
    path.write_text("one\nlast\n")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 100, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/2  col 1")
        os.write(master, b"p")
        receive(master, b"register is empty")
        os.write(master, b"yy2p")
        receive(master, b"2/4  col 1")
        os.write(master, b"u")
        receive(master, b"1/2  col 1")
        os.write(master, b"ly2lp")
        receive(master, b"onnee")
        os.write(master, b"u")
        receive(master, b"1/2  col 3")
        os.write(master, b"dd")
        receive(master, b"1/1  col 1")
        os.write(master, b"P")
        receive(master, b"1/2  col 1")
        os.write(master, b"cc")
        receive(master, b"[INSERT]")
        os.write(master, b"NEW\x03")
        receive(master, b"[VIEW] [+]")
        os.write(master, b"p")
        receive(master, b"2/3  col 1")
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == "NEW\none\nlast\n"
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def change_session(directory):
    path = directory / "changes"
    path.write_text("ab中tail\nnext\nlast")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 100, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/3  col 1")
        os.write(master, b"llc$")
        receive(master, b"[INSERT]")
        os.write(master, b"XYZ\x03")
        receive(master, b"[VIEW] [+]")
        os.write(master, b"u")
        receive(master, "ab中tail".encode())
        os.write(master, b"u")
        receive(master, b"abXYZ")
        os.write(master, b"2cc")
        receive(master, b"[INSERT]")
        os.write(master, "新\x03".encode())
        receive(master, b"[VIEW] [+]")
        os.write(master, b"u")
        receive(master, b"abXYZ")
        os.write(master, b"u")
        receive(master, "新".encode())
        os.write(master, b"ck")
        receive(master, b"motion out of bounds")
        os.write(master, b"c\x03")
        receive(master, b"[VIEW]")
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == "新\nlast"
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def operator_session(directory):
    path = directory / "operators"
    path.write_text("a中🙂xyz\none\ntwo\nlast")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 100, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/4  col 1")
        os.write(master, b"2d3l")
        receive(master, b"[VIEW] [+]")
        os.write(master, b"u")
        receive(master, "a中🙂xyz".encode())
        os.write(master, b"lld0")
        receive(master, "🙂xyz".encode())
        os.write(master, b"d2$")
        receive(master, b"1/3  col 1")
        os.write(master, b"u")
        receive(master, b"1/4  col 1")
        os.write(master, b"dj")
        receive(master, b"1/2  col 1")
        os.write(master, b"dk")
        receive(master, b"operator motion out of bounds")
        os.write(master, b"dG")
        receive(master, b"0/0  col 1")
        os.write(master, b"u")
        receive(master, b"1/2  col 1")
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == "two\nlast"
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def replace_session(directory):
    path = directory / "replace-text"
    path.write_text("a中z\nlast")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 100, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"[VIEW]")
        os.write(master, "lR🙂XY".encode())
        receive(master, "a🙂XY".encode())
        os.write(master, b"\x7f\x7f\rQ\x7f\x7f")
        receive(master, "a🙂z".encode())
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 4, 25, 0, 0))
        os.kill(process.pid, signal.SIGWINCH)
        receive(master, b"\x1b[4;1H[REPLACE]")
        os.write(master, b"\x03")
        receive(master, b"[VIEW] [+]")
        os.write(master, b"u")
        receive(master, "a中z".encode())
        os.write(master, b"u")
        receive(master, "a🙂z".encode())
        os.write(master, b"R\x1b")
        receive(master, b"[VIEW]")
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == "a🙂z\nlast"
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def line_actions_session(directory):
    path = directory / "line-actions"
    path.write_text("ab中tail\n  next.\n  third\n)end")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 100, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/4  col 1")
        os.write(master, b"llD")
        receive(master, b"1/4  col 2")
        os.write(master, b"u")
        receive(master, "ab中tail".encode())
        os.write(master, b"0llCXYZ\x1b")
        receive(master, b"1/4  col 5")
        os.write(master, b"u")
        receive(master, "ab中tail".encode())
        os.write(master, b"u")
        receive(master, b"abXYZ")
        os.write(master, b"0r\x03r\x1b[3~")
        receive(master, b"replacement requires one character")
        os.write(master, "r文".encode())
        receive(master, "文bXYZ".encode())
        os.write(master, b"3J")
        receive(master, "文bXYZ next.  third".encode())
        os.write(master, b"u")
        receive(master, b"1/4  col 1")
        os.write(master, b"uJ")
        receive(master, "文bXYZ next.  third)end".encode())
        os.write(master, b"J")
        receive(master, b"no following line to join")
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == "文bXYZ next.  third)end"
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def word_session(directory):
    path = directory / "words"
    content = "one two...中文\n\n  three four\nlast"
    path.write_text(content)
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 5, 120, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/4  col 1")
        for keys, position in [
            (b"w", b"1/4  col 5"),
            (b"2w", b"1/4  col 11"),
            (b"e", b"1/4  col 12"),
            (b"w", b"2/4  col 1"),
            (b"w", b"3/4  col 3"),
            (b"2e", b"3/4  col 12"),
            (b"2b", b"3/4  col 3"),
            (b"b", b"2/4  col 1"),
            (b"e", b"3/4  col 7"),
            (b"999999999999999999w", b"4/4  col 4"),
            (b"999999999999999999b", b"1/4  col 1"),
        ]:
            os.write(master, keys)
            receive(master, position)
        for keys, position in [
            (b"2E", b"1/4  col 12"),
            (b"B", b"1/4  col 5"),
            (b"W", b"2/4  col 1"),
            (b"W", b"3/4  col 3"),
            (b"2E", b"3/4  col 12"),
            (b"3B", b"2/4  col 1"),
            (b"B", b"1/4  col 5"),
            (b"B", b"1/4  col 1"),
            (b"999999999999999999W", b"4/4  col 4"),
            (b"999999999999999999B", b"1/4  col 1"),
        ]:
            os.write(master, keys)
            receive(master, position)
        os.write(master, b"cg")
        receive(master, b"unsupported operator motion")
        os.write(master, b"2dg")
        receive(master, b"unsupported operator motion")
        os.write(master, b"w")
        receive(master, b"1/4  col 5")
        os.write(master, b"rX")
        receive(master, b"one Xwo")
        os.write(master, b"2wu")
        receive(master, "one two...中文".encode())
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == content
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def word_operator_session(directory):
    path = directory / "word-operators"
    path.write_text("one two three four five six seven\nalpha.beta 中文\nlast")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 7, 120, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/3  col 1")
        os.write(master, b"2d3w")
        receive(master, b"\x1b[1;1Hseven")
        os.write(master, b"u")
        receive(master, b"one two three four five six seven")
        os.write(master, b"cwX")
        receive(master, b"X two three")
        os.write(master, b"\x1b")
        receive(master, b"[VIEW]")
        os.write(master, b"u")
        receive(master, b"one two three")
        os.write(master, b"u")
        receive(master, b"X two three")
        os.write(master, b"g0wy2wP")
        receive(master, b"X two three two three four")
        os.write(master, b"u")
        receive(master, b"X two three four five six seven")
        os.write(master, b"g02c3wY")
        receive(master, b"Y seven")
        os.write(master, b"\x1b")
        receive(master, b"[VIEW]")
        os.write(master, b"j0yEP")
        receive(master, "alpha.betaalpha.beta 中文".encode())
        os.write(master, b"u")
        receive(master, "alpha.beta 中文".encode())
        os.write(master, b"0dW")
        receive(master, "\x1b[2;1H中文".encode())
        os.write(master, "cE字".encode())
        receive(master, "\x1b[2;1H字".encode())
        os.write(master, b"\x1b")
        receive(master, b"[VIEW]")
        os.write(master, b"g0db")
        receive(master, b"empty operator motion")
        os.write(master, b"3c\x1b")
        receive(master, b"[VIEW]")
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == "Y seven\n字\nlast"
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def repeat_session(directory):
    path = directory / "repeat"
    path.write_text("one two three four\nalpha beta\nlast")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 7, 120, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/3  col 1")
        os.write(master, b".")
        receive(master, b"no change to repeat")
        os.write(master, "cw中".encode())
        receive(master, "中 two three four".encode())
        os.write(master, b"\x1b")
        receive(master, b"[VIEW]")
        for keys, text in [
            (b"w.", "中 中 three four"),
            (b"u", "中 two three four"),
            (b"u", "中 中 three four"),
            (b"g0w2.", "中 中 four"),
        ]:
            os.write(master, keys)
            receive(master, text.encode())
        os.write(master, b"g0A!")
        receive(master, "中 中 four!".encode())
        os.write(master, b"\x1b")
        receive(master, b"[VIEW]")
        for keys, text in [
            (b"2.", "中 中 four!!!"),
            (b".", "中 中 four!!!!!"),
            (b"u", "中 中 four!!!"),
            (b"u", "中 中 four!!!!!"),
        ]:
            os.write(master, keys)
            receive(master, text.encode())
        os.write(master, b"j0RXY\x7fZ")
        receive(master, b"XZpha beta")
        os.write(master, b"\x1b")
        receive(master, b"[VIEW]")
        os.write(master, b"w.")
        receive(master, b"XZpha XZta")
        os.write(master, b"u")
        receive(master, b"XZpha beta")
        os.write(master, b"u")
        receive(master, b"XZpha XZta")
        os.write(master, b"/four\r")
        receive(master, b"1/3  col 5")
        os.write(master, b"rQ")
        receive(master, "中 中 Qour!!!!!".encode())
        os.write(master, b"999.")
        receive(master, b"not enough characters to replace")
        os.write(master, b"l.")
        receive(master, "中 中 QQur!!!!!".encode())
        os.write(master, b"9\x1b")
        receive(master, b"[VIEW]")
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 5, 120, 0, 0))
        os.kill(process.pid, signal.SIGWINCH)
        receive(master, b"\x1b[5;1H[VIEW]")
        os.write(master, f":w {directory}\r".encode())
        receive(master, b"? ")
        os.write(master, b":w\r")
        receive(master, b"Written")
        expected = "中 中 QQur!!!!!\nXZpha XZta\nlast"
        assert path.read_text() == expected
        os.write(master, b":e!\r")
        receive(master, b"3/3  col 1")
        os.write(master, b"G0.")
        receive(master, b"Qast")
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == expected.replace("last", "Qast")
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def conflict_session(directory):
    path = directory / "conflict"
    export = directory / "conflict-export"
    path.write_text("one two three\n")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 7, 180, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/1  col 1")
        os.write(master, b"dw")
        receive(master, b"[+]")
        path.write_text("external\n")
        os.write(master, b":wq\r")
        receive(master, b"file changed since loading or saving")
        assert process.poll() is None
        assert path.read_text() == "external\n"
        os.write(master, b"u")
        receive(master, b"one two three")
        os.write(master, b"u")
        receive(master, b"two three")
        os.write(master, b".")
        receive(master, b"1/1  col 1")
        os.write(master, f":w {export}\r".encode())
        receive(master, b"Written")
        assert export.read_text() == "three\n"
        os.write(master, b":w\r")
        receive(master, b"file changed since loading or saving")
        os.write(master, b":e!\r")
        receive(master, b"external")
        os.write(master, b"A!")
        receive(master, b"external!")
        os.write(master, b"\x1b")
        receive(master, b"[VIEW]")
        os.write(master, b":w\r")
        receive(master, b"Written")
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == "external!\n"
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def counted_replace_session(directory):
    path = directory / "counted-replace"
    path.write_text("abcdefghij\n中文测试abcd\nshort")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 150, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    def send(keys, marker):
        os.write(master, keys)
        receive(master, marker)
    try:
        receive(master, b"1/3  col 1")
        send(b"3rX", b"XXXdefghij")
        send(b"u", b"abcdefghij")
        send(b"u", b"XXXdefghij")
        send(b"g0l", b"col 2")
        # Failure is reported on r, before any replacement key is supplied.
        send(b"99r", b"not enough characters to replace")
        send(b"l", b"col 3")
        send(b"3r\x1b", b"[VIEW]")
        send(b"l", b"col 4")
        send(b"3r\x03", b"[VIEW]")
        send(b"l", b"col 5")
        send(b"2r\x1b[3~", b"replacement requires one character")
        send(b"l", b"col 6")
        # Overflow clears the prefix; subsequent r is uncounted.
        maximum = (1 << (struct.calcsize("P") * 8)) - 1
        send(str(maximum).encode() + b"0", b"count too large")
        send(b"rZ", b"XXXdeZghij")
        send(b"j0", b"2/3  col 1")
        send(b"3r", b"[VIEW]")
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 150, 0, 0))
        os.kill(process.pid, signal.SIGWINCH)
        receive(master, b"\x1b[6;1H[VIEW]")
        send("字".encode(), "字字字试abcd".encode())
        send(b"u", "中文测试abcd".encode())
        send(b"u", "字字字试abcd".encode())
        send(b"g0", b"1/3  col 1")
        send(b"3r0", b"000deZghij")
        send(b"l.", b"000000ghij")
        send(b"l2.", b"00000000ij")
        send(b"l.", b"0000000000")
        send(b"l.", b"not enough characters to replace")
        send(b":w\r", b"Written")
        expected = "0000000000\n字字字试abcd\nshort"
        assert path.read_text() == expected
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        data = receive(master, b"\x1b[?1049l")
        assert b"\x1b[?2004l" in data
        assert path.read_text() == expected
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def counted_insert_session(directory):
    path = directory / "counted-insert"
    path.write_text("abc\n")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 100, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    def send(keys, marker):
        os.write(master, keys)
        return receive(master, marker)
    try:
        receive(master, b"[VIEW]")
        send(b"3iX", b"Xabc")
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 100, 0, 0))
        os.kill(process.pid, signal.SIGWINCH)
        receive(master, b"\x1b[6;1H[INSERT]")
        send(b"\x1b", b"XXXabc")
        send(b"u", b"abc")
        send(b"3iX\rY", b"Yabc")
        send(b"\x1b", b"[VIEW]")
        send(b":w\r", b"Written")
        assert path.read_text() == "XXX\nYabc\n"
        send(b"u", b"1/1")
        send(b"0", b"col 1")
        send(b"2aQ\x1b", b"aQQbc")
        send(b"2IZ\x1b", b"ZZaQQbc")
        send("3A字\x03".encode(), "ZZaQQbc字字字".encode())
        send(b"2.", "ZZaQQbc字字字字字".encode())
        send(b".", "ZZaQQbc字字字字字字字".encode())
        send(b"u", "ZZaQQbc字字字字字".encode())
        send(b"0", b"col 1")
        send(b"3iab\x7fX\x1b", b"aXaXaXZZaQQbc")
        send(b"u", b"ZZaQQbc")
        # Budget rejection keeps the input transaction and allows correction.
        send(b"500000iX", b"XZZaQQbc")
        send(b"Y", b"input repeat too large")
        send(b"\x7f\x1b", b"[VIEW]")
        send(b"3i\x1b[200~P\r\nQ\x1b[201~", b"QZZaQQbc")
        send(b"\x1b", b"[VIEW]")
        send(b":w\r", b"Written")
        assert path.read_text() == "PPP\nQZZaQQbc字字字字字\n"
        os.write(master, b":wq\r")
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


def line_replacement_undo_session(directory):
    path = directory / "line-replacement-undo"
    path.write_text("first\n  abc\n def\nlast")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 80, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/4  col 1")
        for keys, position in [
            (b"j2lJ", b"2/3  col 6"),
            (b"Gu", b"2/4  col 3"),
            (b"Gu", b"2/3  col 3"),
            (b"u", b"2/4  col 3"),
            (b"jck\tX\rY\x1b", b"3/4  col 1"),
            (b"Gu", b"2/4  col 3"),
            (b"Gu", b"2/4  col 2"),
        ]:
            os.write(master, keys)
            receive(master, position)
        os.write(master, b":w\r")
        receive(master, b"Written")
        assert path.read_text() == "first\n\tX\nY\nlast"
        os.write(master, b":u\r")
        receive(master, b"4/4  col 1")
        os.write(master, b"u")
        receive(master, b"2/4  col 2")
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == "first\n\tX\nY\nlast"
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def multiline_input_undo_session(directory):
    path = directory / "multiline-input-undo"
    content = "first\n  abc\nlast"
    path.write_text(content)
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 80, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave, stderr=subprocess.PIPE)
    try:
        receive(master, b"1/3  col 1")
        for keys, position in [
            (b"j3l", b"2/3  col 4"),
            (b"2iX\rY\x1b", b"3/4  col 1"),
            (b"Gu", b"2/3  col 3"),
            (b"Gu", b"2/4  col 3"),
        ]:
            os.write(master, keys)
            receive(master, position)
        os.write(master, b":w\r")
        receive(master, b"Written")
        assert path.read_text() == "first\n  aXX\nYbc\nlast"
        os.write(master, b":u\r")
        receive(master, b"3/3  col 1")
        os.write(master, b"Gu")
        receive(master, b"2/4  col 3")
        os.write(master, b"u:wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == content
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


with tempfile.TemporaryDirectory(prefix="vir-visual-") as directory:
    line_replacement_undo_session(Path(directory))
    multiline_input_undo_session(Path(directory))
    counted_insert_session(Path(directory))
    counted_replace_session(Path(directory))
    conflict_session(Path(directory))
    repeat_session(Path(directory))
    word_operator_session(Path(directory))
    word_session(Path(directory))
    register_session(Path(directory))
    change_session(Path(directory))
    operator_session(Path(directory))
    replace_session(Path(directory))
    line_actions_session(Path(directory))
    normal_actions_session(Path(directory))
    insert_session(Path(directory))
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
    data = receive(master, b"test result: ok")
    assert data.count(b"\x1b[?2004h") == 2
    assert data.count(b"\x1b[?2004l") == 2
    assert process.wait(timeout=5) == 0
    assert termios.tcgetattr(slave) == original
finally:
    if process.poll() is None:
        process.kill()
        process.wait()
    os.close(master)
    os.close(slave)
print("PTY: injected error and panic restoration passed")
