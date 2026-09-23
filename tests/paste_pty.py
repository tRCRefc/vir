"""Unix bracketed-paste protocol acceptance; run after cargo build."""
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
    deadline = time.monotonic() + 8
    while marker not in data and time.monotonic() < deadline:
        if select.select([master], [], [], 0.1)[0]:
            data += os.read(master, 65536)
    assert marker in data, (marker, data[-4000:])
    return data


class Session:
    def __init__(self, directory, text):
        self.directory = directory
        self.path = directory / "text"
        self.path.write_bytes(text)
        self.master, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 150, 0, 0))
        self.original = termios.tcgetattr(self.slave)
        self.process = subprocess.Popen([str(BINARY), "--vi", str(self.path)],
            stdin=self.slave, stdout=self.slave, stderr=subprocess.PIPE,
            env=dict(os.environ, VIR_RECOVERY_DIR=str(directory)))
        assert b"\x1b[?2004h" in receive(self.master, b"[VIEW]")

    def send(self, data, marker=None):
        while data:
            data = data[os.write(self.master, data):]
        if marker is not None:
            return receive(self.master, marker)

    def paste(self, data, marker):
        # Start, payload and terminator are deliberately separate writes.
        self.send(b"\x1b[200~")
        self.send(data)
        return self.send(b"\x1b[201~", marker)

    def save_exit(self, expected):
        self.send(b":wq\r")
        assert self.process.wait(timeout=5) == 0
        data = receive(self.master, b"\x1b[?1049l")
        assert b"\x1b[?2004l" in data
        assert self.path.read_bytes() == expected
        assert termios.tcgetattr(self.slave) == self.original
        assert self.process.stderr.read() == b""
        assert not list(self.directory.glob(".vir-recovery-*"))

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait()
        os.close(self.master)
        os.close(self.slave)


def insert(session):
    session.send(b"i", b"[INSERT]")
    payload = b":q!\x1b\x03\x08\x7f\x00\tX\r\nY"
    session.paste(payload, b"Ytail")
    assert session.process.poll() is None
    fcntl.ioctl(session.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 150, 0, 0))
    receive(session.master, b"\x1b[6;1H[INSERT]")
    session.send(b"\x1b", b"[VIEW]")
    session.send(b"u", b"1/1  col 1")
    session.send(b".", b"Ytail")
    session.save_exit(payload.replace(b"\r\n", b"\n") + b"tail\n")


def replace(session):
    session.send(b"R", b"[REPLACE]")
    session.paste(b"X\nY", b"Ycdef")
    session.send(b"\x7f", b"bcdef")
    session.send(b"\x7f", b"Xbcdef")
    session.send(b"Z", b"XZcdef")
    session.send(b"\x1b", b"[VIEW]")
    session.send(b"u", b"abcdef")
    session.send(b".", b"XZcdef")
    session.save_exit(b"XZcdef\n")


def normal(session):
    session.paste(b"XY", b"XYabcdef")
    session.send(b"u", b"1/1  col 1")
    session.send(b"u", b"XYabcdef")
    # Undo uses the historical snapshot cursor; place back on pasted Y.
    session.send(b"l", b"col 2")
    session.send(b".", b"XYXYabcdef")
    session.save_exit(b"XYXYabcdef\n")


def pending_and_command(session):
    for pending in [b"d", b"r", b"3r", b"2", b"c"]:
        session.send(pending)
        session.paste(b":q!\n", b"paste cancelled pending command")
        session.send(b":w\r", b"Written")
        assert session.path.read_bytes() == b"abcdef\n"
    session.send(b":", b"\x1b[8;1H:")
    session.paste(b"wq\n", b"command-line paste must be a single line")
    assert session.process.poll() is None
    session.send(b"\x1b", b"[VIEW]")
    session.send(b":", b"\x1b[8;1H:")
    session.paste(b"wq", b":wq")
    assert session.process.poll() is None
    session.send(b"\x1b", b"[VIEW]")
    session.save_exit(b"abcdef\n")


def oversized(session):
    session.paste(b"x" * 65537, b"paste exceeds 64 KiB")
    session.send(b"i", b"[INSERT]")
    session.paste(b"x" * 65537, b"paste exceeds 64 KiB")
    session.paste(b"OK", b"OKabcdef")
    session.send(b"\x1b", b"[VIEW]")
    session.save_exit(b"OKabcdef\n")


def counted_open(session):
    session.send(b"3o", b"[INSERT]")
    fcntl.ioctl(session.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 150, 0, 0))
    receive(session.master, b"\x1b[6;1H[INSERT]")
    session.paste("字\r\nY".encode(), b"Y")
    session.send(b"\x1b", b"[VIEW]")
    session.send(b":w\r", b"Written")
    assert session.path.read_text() == "abcdef\n字\nY\n"
    session.send(b"u", b"1/1")
    session.send(b"2.", b"Y")
    session.send(b"3OQ\x03", b"[VIEW]")
    session.send(b":w\r", b"Written")
    assert session.path.read_text() == "abcdef\n字\nQ\nY\n"
    maximum = (1 << (struct.calcsize("P") * 8)) - 1
    session.send(str(maximum).encode() + b".", b"[VIEW]")
    session.send(b":w\r", b"Written")
    assert session.path.read_text() == "abcdef\n字\nQ\nQ\nY\n"
    # A representable screen count is ignored; arithmetic overflow is cancelled.
    session.send(str(maximum).encode() + b"0", b"count too large")
    session.send(str(maximum).encode() + b"o", b"[INSERT]")
    session.send(b"\x1b", b"[VIEW]")
    session.save_exit("abcdef\n字\nQ\n\nQ\nY\n".encode())


def counted_replace(session):
    session.send(b"3RXY", b"XYcdef")
    fcntl.ioctl(session.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 150, 0, 0))
    receive(session.master, b"\x1b[6;1H[REPLACE]")
    session.send(b"\x03", b"XYXYXYcdef")
    session.send(b"u", b"abcdef")
    session.send(b"0", b"col 1")
    session.send(b"2.", b"XYXYcdef")
    session.send(b"u", b"abcdef")
    session.send(b"0", b"col 1")
    session.send(b"3R", b"[REPLACE]")
    session.paste(b"X\r\nY", b"Ycdef")
    session.send(b"\x7f\x7f\x7f", b"XXbcdef")
    session.send(b"\x1b", b"[VIEW]")
    session.send(b":w\r", b"Written")
    assert session.path.read_text() == "XXbcdef\n"
    session.send(b"u", b"abcdef")
    session.send(b"0", b"col 1")
    session.send(b"250000R", b"[REPLACE]")
    session.send("字".encode(), "字bcdef".encode())
    session.send(b"X", b"input repeat too large")
    session.send(b"\x7f", b"abcdef")
    session.send(b"\x1b", b"[VIEW]")
    session.save_exit(b"abcdef\n")


def quoted_input(session):
    session.send(b"i\x16", b"quote: type one character")
    fcntl.ioctl(session.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 150, 0, 0))
    receive(session.master, b"\x1b[6;1H[INSERT] quote:")
    # Standalone ESC must be a distinct terminal event, not Alt plus next key.
    session.send(b"\x1b", b"^[abcdef")
    session.send(b"\x16\x03", b"^[^Cabcdef")
    session.send(b"\x11\x08", b"^[^C^Habcdef")
    session.send(b"\x16\x7f", b"^[^C^H^?abcdef")
    session.send(b"\x16\x1c", b"^[^C^H^?^\\abcdef")
    session.send(b"\x16\x00", b"quoted NUL is unsupported")
    session.send(b"9", b"9abcdef")  # No numeric-escape accumulation.
    session.send(b"\x16", b"quote: type one character")
    session.paste(b"must not execute", b"nothing pasted")
    session.send(b"\x1b", b"[VIEW]")
    session.send(b":w\r", b"Written")
    literal = b"\x1b\x03\x08\x7f\x1c9"
    assert session.path.read_bytes() == literal + b"abcdef\n"
    session.send(b"u", b"abcdef")
    session.send(b"0.", b"9abcdef")
    session.send(b"u", b"abcdef")
    session.send(b"03R\x16\x03", b"^Cbcdef")
    session.send(b"\x7f", b"abcdef")
    session.send(b"\x16\x03", b"^Cbcdef")
    session.send(b"\x1b", b"^C^C^Cbcdef")
    session.send(b"u", b"abcdef")
    session.send(b"03RZ\x16\x0aY", b"Ycdef")
    session.send(b"\x1b", b"[VIEW]")
    session.send(b":w\r", b"Written")
    assert session.path.read_bytes() == b"ZZZ\nYcdef\n"
    session.send(b"g0cw\x16\x03", b"^C")
    session.send(b"\x1b", b"[VIEW]")
    session.save_exit(b"\x03\nYcdef\n")


def special_r(session):
    session.send(b"3r\t", b"col 3")
    session.send(b":w\r", b"Written")
    assert session.path.read_bytes() == b"\t\t\tdef\n"
    session.send(b"u0", b"abcdef")
    session.send(b"3r\r", b"2/2  col 1")
    session.send(b":w\r", b"Written")
    assert session.path.read_bytes() == b"\ndef\n"
    session.send(b"2.", b"3/3  col 1")
    session.send(b":w\r", b"Written")
    assert session.path.read_bytes() == b"\n\nf\n"
    session.send(b"3r", b"not enough characters")
    session.send(b"g", b"1/3")
    session.save_exit(b"\n\nf\n")


def quoted_r(session):
    session.send(b"3r\x16", b"quote: type one replacement character")
    fcntl.ioctl(session.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 150, 0, 0))
    receive(session.master, b"\x1b[6;1H")
    session.send(b"\x00", b"quoted NUL is unsupported")
    session.send(b"\x1b", b"^[^[^[def")
    session.send(b":w\r", b"Written")
    assert session.path.read_bytes() == b"\x1b\x1b\x1bdef\n"
    session.send(b"u0", b"abcdef")
    session.send(b"2r\x11\x03", b"^C^Ccdef")
    session.send(b"l2.", b"^C^C^C^Cef")
    session.send(b"u", b"^C^Ccdef")
    session.send(b"0r\x16", b"quote: type one replacement character")
    session.paste(b"ignored", b"nothing pasted")
    session.send(b"rZ", b"Z^Ccdef")
    session.save_exit(b"Z\x03cdef\n")


def backspace_at_limit(session):
    for mode in [b"i", b"R"]:
        session.send(b"500000" + mode + b"\r", b"2/2  col 1")
        session.send(b"\x7f", b"1/1  col 1")
        session.send(b"\x1b", b"[VIEW]")
        session.send(b":w\r", b"Written")
        assert session.path.read_bytes() == b"abcdef\n"
    session.save_exit(b"abcdef\n")


def line_undo(session):
    session.send(b"rXllrY", b"XbYdef")
    session.send(b"U", b"abcdef")
    session.send(b"U", b"no line changes to restore")
    session.send(b"u", b"XbYdef")
    session.send(b"U", b"abcdef")
    session.send(b"l.", b"aYcdef")
    session.send(b":w\r", b"Written")
    session.send(b"U", b"abcdef")
    session.send(b"u", b"aYcdef")
    session.send(b"j", b"2/2")
    session.send(b"kU", b"no line changes to restore")
    session.send(b"0rZ", b"ZYcdef")
    session.send(b"i\r\x7f", b"ZYcdef")
    session.send(b"\x1b", b"[VIEW]")
    session.send(b"U", b"no line changes to restore")
    session.send(b":e!\r", b"aYcdef")
    session.send(b"U", b"no line changes to restore")
    session.send(b"g0rQ", b"QYcdef")
    fcntl.ioctl(session.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 6, 150, 0, 0))
    receive(session.master, b"\x1b[6;1H[VIEW]")
    session.send(b"U", b"aYcdef")
    session.save_exit(b"aYcdef\nsecond\n")


def undo_cursor(session):
    session.send(b"llrX", b"abXdef")
    session.send(b"$u", b"col 3")
    session.send(b"u", b"abXdef")
    session.send(b":w\r", b"Written")
    session.send(b"0u", b"col 3")
    session.send(b"aZ", b"abcZdef")
    session.send(b"\x1b", b"[VIEW]")
    session.send(b"0u", b"col 4")
    session.send(b"2dh", b"adef")
    session.send(b"$u", b"col 2")
    session.send(b"U", b"no line changes to restore")
    session.save_exit(b"abcdef\n")


def line_undo_cursor(session):
    session.send(b"jdd", b"three")
    session.send(b"gu", b"2/3  col 3")
    session.send(b"Gu", b"1/2  col 3")
    session.send(b"u", b"2/3  col 3")
    session.send(b"yyP", b"2/4  col 3")
    session.send(b"Gu", b"1/3  col 3")
    session.send(b"u", b"2/4  col 3")
    session.send(b":w\r", b"Written")
    session.send(b"gu", b"1/3  col 3")
    session.send(b":u\r", b"1/4  col 1")
    session.send(b"u", b"1/3  col 3")
    session.save_exit(b"  one\n  two\nthree\n")


def open_undo_cursor(session):
    session.send(b"j3O  X\rY", b"[INSERT]")
    session.send(b"\x1b", b"[VIEW]")
    session.send(b"Gu", b"1/3  col 3")
    session.send(b"u", b"2/5  col 3")
    session.send(b":w\r", b"Written")
    session.send(b"u", b"1/3  col 3")
    session.send(b"j2o Z", b"[INSERT]")
    session.send(b"\x1b", b"[VIEW]")
    session.send(b"gu", b"2/3  col 3")
    session.send(b"u", b"3/4  col 2")
    session.save_exit(b"  one\n  two\n Z\nthree\n")


def net_unchanged(session):
    session.send(b"3cl", b"[INSERT]")
    session.send(b"ab", b"1/4  col 3")
    session.send(b"\x1b", b"[VIEW]")
    session.send(b":w\r", b"Written")
    session.send(b"Gyy", b"4/4  col 1")
    session.send(b"2G", b"2/4  col 1")
    # Replaying with count two changes neither text nor the saved count of three.
    session.send(b"2.", b"2/4  col 1")
    session.send(b"j.", b"abZ")
    session.send(b":w\r", b"Written")
    assert session.path.read_bytes() == b"ab\nabC\nabZ\nREG\n"
    session.send(b"u", b"ZZZZ")
    session.send(b"2G", b"2/4  col 1")
    session.send(b"Gyy", b"4/4  col 1")
    session.send(b"2G", b"2/4  col 1")
    session.send(b"2.", b"2/4  col 1")
    # The staged character register must not replace the previous line yank.
    session.send(b"P", b"2/5  col 1")
    session.send(b":w\r", b"Written")
    assert session.path.read_bytes() == b"ab\nREG\nabC\nZZZZ\nREG\n"
    session.send(b"u", b"1/4  col 1")
    session.send(b":w\r", b"Written")
    session.send(b"2G", b"2/4  col 1")
    # Direct change publishes the captured characters even when text is identical.
    session.send(b"2cl", b"[INSERT]")
    session.send(b"ab", b"2/4  col 3")
    session.send(b"\x1b", b"2/4  col 2")
    session.send(b"0P", b"ababC")
    session.save_exit(b"ab\nababC\nZZZZ\nREG\n")


def identical_r(session):
    session.send(b"3ra", b"aaadef")
    session.send(b":w\r", b"Written")
    session.send(b"02ra", b"1/1  col 2")
    session.send(b":q\r", b"unsaved changes")
    session.send(b"u", b"1/1  col 1")
    session.send(b":w\r", b"Written")
    assert session.path.read_bytes() == b"aaadef\n"
    session.send(b"ll.", b"aaaaef")
    # Escape cancels r and keeps the successful count=2 repeat.
    session.send(b"r\x1b", b"[VIEW]")
    session.send(b"l.", b"aaaaaa")
    session.save_exit(b"aaaaaa\n")


def crash(session):
    session.send(b"R", b"[REPLACE]")
    session.paste("中\n文".encode(), "文cdef".encode())
    session.process.kill()
    session.process.wait(timeout=5)
    snapshots = list(session.directory.glob(".vir-recovery-*"))
    assert len(snapshots) == 1
    target = session.directory / "recovered"
    result = subprocess.run([str(BINARY), "--ex", "--recover", str(snapshots[0])],
        input=f"w {target}\nq\n", text=True, capture_output=True, timeout=5,
        env=dict(os.environ, VIR_RECOVERY_DIR=str(session.directory)))
    assert result.returncode == 0, result.stderr
    assert target.read_text() == "中\n文cdef\n"
    assert session.path.read_bytes() == b"abcdef\n"


with tempfile.TemporaryDirectory(prefix="vir-paste-") as temporary:
    for action in [insert, replace, normal, pending_and_command, oversized, counted_open, counted_replace, quoted_input, special_r, quoted_r, backspace_at_limit, line_undo, undo_cursor, line_undo_cursor, open_undo_cursor, net_unchanged, identical_r, crash]:
        directory = Path(temporary) / action.__name__
        directory.mkdir()
        session = Session(directory, b"QQQ\nabC\nZZZZ\nREG\n" if action == net_unchanged else b"tail\n" if action == insert else b"abcdef\nsecond\n" if action == line_undo else b"  one\n  two\nthree\n" if action in [line_undo_cursor, open_undo_cursor] else b"abcdef\n")
        try:
            action(session)
        finally:
            session.close()
print("Paste: literal input, R/backspace, undo/dot, prefixes, command line, limits, resize, crash recovery and terminal cleanup passed")
