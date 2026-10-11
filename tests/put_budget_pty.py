"""Reject counted put under a fixed Unix address-space limit; run after cargo build."""
import fcntl
import os
from pathlib import Path
import pty
import resource
import struct
import subprocess
import tempfile
import termios

from input_corrections_pty import receive

BINARY = Path(__file__).resolve().parents[1] / "target/debug/vir"


def limit_memory():
    resource.setrlimit(resource.RLIMIT_AS, (128 * 1024 * 1024,) * 2)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="vir-put-budget-") as name:
        path = Path(name) / "text"
        original = "x" * 65_536 + "\n"
        path.write_text(original)
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 100, 0, 0))
        attributes = termios.tcgetattr(slave)
        process = subprocess.Popen(
            [str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave,
            stderr=subprocess.PIPE, preexec_fn=limit_memory,
            env=dict(os.environ, VIR_RECOVERY_DIR=name),
        )
        try:
            receive(master, b"[VIEW]")
            os.write(master, b"yy100000p")
            receive(master, b"? paste too large")
            assert process.poll() is None
            os.write(master, b":wq\r")
            receive(master, b"\x1b[?1049l")
            assert process.wait(timeout=5) == 0
            assert process.stderr.read() == b""
            assert path.read_text() == original
            assert termios.tcgetattr(slave) == attributes
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)
    print("put budget under 128 MiB PTY passed")
