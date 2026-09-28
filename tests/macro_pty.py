"""Register macro acceptance on a real Unix terminal; run after cargo build."""
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
        if select.select([master], [], [], 0.1)[0]:
            data += os.read(master, 65536)
    assert marker in data, (marker, data[-4000:])
    return data


def case(directory, name, registers, initial, actions, expected):
    path = directory / name
    definitions = "\n".join(text for _, text, _ in registers) + "\n"
    path.write_text(definitions + initial + "\n")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 150, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave,
        stderr=subprocess.PIPE, env=dict(os.environ, VIR_RECOVERY_DIR=str(directory)))
    try:
        receive(master, b"[VIEW]")
        for row, (register, _, linewise) in enumerate(registers, 1):
            keys = f'{row}G0"{register}' + ("yy" if linewise else "y$")
            os.write(master, keys.encode())
            # A status command makes a distinct marker after the yank completes.
            os.write(master, b":f\r")
            receive(master, f"line {row} of {len(registers) + 1}".encode())
        os.write(master, b"G0")
        receive(master, f"{len(registers) + 1}/{len(registers) + 1}  col 1".encode())
        for keys, marker in actions:
            os.write(master, keys)
            if marker:
                receive(master, marker)
        os.write(master, b":wq\r")
        assert process.wait(timeout=5) == 0
        receive(master, b"\x1b[?1049l")
        assert path.read_text() == definitions + expected + "\n", (name, path.read_text())
        assert termios.tcgetattr(slave) == original
        assert process.stderr.read() == b""
        assert not list(directory.glob(".vir-recovery-*"))
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


with tempfile.TemporaryDirectory(prefix="vir-macro-") as temporary:
    directory = Path(temporary)
    case(directory, "insert-repeat", [('a', "A中!\x1b", False)], "tail",
        [(b"@a", "tail中!".encode()), (b"@@", "tail中!中!".encode())], "tail中!中!")
    case(directory, "replace-correction", [('a', "0RXY\x7fZ\x1b", False)], "abcdef",
        [(b"@a", b"XZcdef")], "XZcdef")
    case(directory, "change", [('a', "0cwQ\x1b", False)], "abc def",
        [(b"@a", b"Q def")], "Q def")
    case(directory, "quote", [('a', "r\x16\x1b", False)], "abc",
        [(b"@a", b"^[bc")], "\x1bbc")
    case(directory, "control-c", [('a', "A!\x03", False)], "tail",
        [(b"@a", b"tail!")], "tail!")
    case(directory, "nested", [('a', "A!\x1b", False), ('b', "@a0rZ", False)], "tail",
        [(b"@b", b"Zail!")], "Zail!")
    case(directory, "colon-linewise", [('a', ":s/abc/DEF/", True)], "abc",
        [(b"@a", b"DEF")], "DEF")
    case(directory, "motion-error", [('a', "fzx", False)], "abc",
        [(b"@a", b"character not found")], "abc")
    case(directory, "empty-error", [('a', "@zx", False)], "abc",
        [(b"@a", b"macro register is empty")], "abc")
    case(directory, "recursion", [('r', "@r", False), ('a', "A!\x1b", False)], "tail",
        [(b"@r", b"macro expansion limit"), (b"@a", b"tail!")], "tail!")
print("Macros: input modes, quoting, nested expansion, linewise ex, errors and recursion passed")
