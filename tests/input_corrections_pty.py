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


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="vir-input-regression-") as name:
        directory = Path(name)
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
