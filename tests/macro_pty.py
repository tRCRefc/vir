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
    definition_rows = sum(len(text.split("\n")) for _, text, _ in registers)
    initial_row = definition_rows + 1
    total_rows = definition_rows + len(initial.split("\n"))
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 8, 150, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([str(BINARY), "--vi", str(path)], stdin=slave, stdout=slave,
        stderr=subprocess.PIPE, env=dict(os.environ, VIR_RECOVERY_DIR=str(directory)))
    try:
        receive(master, b"[VIEW]")
        row = 1
        for register, text, linewise in registers:
            count = len(text.split("\n"))
            assert linewise or count == 1, "multiline macro fixtures require linewise registers"
            keys = f'{row}G0"{register}' + (f"{count}yy" if linewise else "y$")
            os.write(master, keys.encode())
            # A status command makes a distinct marker after the yank completes.
            os.write(master, b":f\r")
            receive(master, f"line {row} of {total_rows}".encode())
            row += count
        os.write(master, f"{initial_row}G0".encode())
        receive(master, f"{initial_row}/{total_rows}  col 1".encode())
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
    case(directory, "percent-jump", [('a', "%", False)], "a (x) b",
        [(b"3@a", b"2/2  col 5")], "a (x) b")
    case(directory, "percent-delete", [('a', "d%", False)], "a (x) b",
        [(b"@a", None)], " b")
    case(directory, "percent-backward", [('a', "$%", False)], "(x)",
        [(b"@a", b"2/2  col 1")], "(x)")
    case(directory, "percent-change-dot", [('a', '9c%X\x1b', False)], "(one) [two] end",
        [(b"@aw2.", None)], "X X end")
    case(directory, "percent-yank", [('a', '"by%"bP', False)], "(x)",
        [(b"@a", None)], "(x)(x)")
    case(directory, "percent-multiline-undo", [('a', "d%", False)], "{\nx\n}\ntail",
        [(b"@au", None)], "{\nx\n}\ntail")
    case(directory, "percent-error-stops-macro", [('a', "%rZ", False)], "plain",
        [(b"@a", b"no matching bracket"), (b"rX", None)], "Xlain")
    case(directory, "alias-X", [('a', 'lll"b2X"bP', False)], "abcdef",
        [(b"@a", None)], "abcdef")
    case(directory, "alias-s", [('a', '2sX\x1b', False)], "abcdef",
        [(b"@a3.", None)], "Xef")
    case(directory, "alias-S", [('a', '2SX\x1b', False)], "one\ntwo\nlast",
        [(b"@a", None)], "X\nlast")
    case(directory, "alias-S-undo", [('a', '2SX\x1b', False)], "one\ntwo\nlast",
        [(b"@au", None)], "one\ntwo\nlast")
    case(directory, "alias-Y", [('a', '"b2Y"bp', False)], "one\ntwo\nlast",
        [(b"@a", None)], "one\none\ntwo\ntwo\nlast")
    case(directory, "mark-exact-jump", [('a', "llmaG`arZ", False)], "abcde\nlast",
        [(b"@a", None)], "abZde\nlast")
    case(directory, "mark-line-jump", [('a', "lllmaG'arX", False)], "  abc\nlast",
        [(b"@a", None)], "  Xbc\nlast")
    case(directory, "mark-ex-address-after-move", [('a', "ma:.,.+1m$\n:'as/one/ONE/", True)], "one\ntwo\nlast",
        [(b"@a", None)], "last\nONE\ntwo")
    case(directory, "mark-macro-undo", [('a', "xx", False)], "abcdef",
        [(b"llma0@au`arZ", None)], "abZdef")
    case(directory, "mark-line-operator", [('a', "jmakd'a", False)], "one\ntwo\nlast",
        [(b"@a", None)], "last")
    case(directory, "mark-unset-stops-macro", [('a', "`zx", False)], "abc",
        [(b"@a", b"mark is not set")], "abc")
    source = directory / "read source"
    source.write_text("new\n")
    case(directory, "ex-move", [('a', ":.,.+1m$", True)], "one\ntwo\nlast",
        [(b"@a", None)], "last\none\ntwo")
    case(directory, "ex-copy", [('a', ":t$", True)], "one\ntwo",
        [(b"@a", None)], "one\ntwo\none")
    for command in ["m$", "t$"]:
        case(directory, f"ex-transfer-undo-{command[0]}", [('a', f"x:{command}", True)], "one\ntwo\nlast",
            [(b"@au", None)], "one\ntwo\nlast")
    case(directory, "ex-join", [('a', ":join", True)], "one.\n two\nlast",
        [(b"@a", b"one.  two")], "one.  two\nlast")
    case(directory, "ex-join-undo", [('a', "x:join", True)], "one\ntwo",
        [(b"@au", None)], "one\ntwo")
    case(directory, "ex-read", [('a', f":read {source}", True)], "tail",
        [(b"@a", b"new")], "tail\nnew")
    case(directory, "ex-read-undo", [('a', f"x:read {source}", True)], "tail",
        [(b"@au", None)], "tail")
    case(directory, "visual-ex-register-bridge", [('a', "X", False)], "tail",
        [(b':y b\r:put b\r', None)], "tail\ntail")
    case(directory, "ex-yank-promotes-before-nesting", [('a', "x", False), ('b', "x:y c\n@a", True)], "abcdef\nlast",
        [(b"@bu", None)], "abcdef\nlast")
    case(directory, "append-character-to-line", [('a', ":s/a/A/", False), ('A', ":s/b/B/", True)], "ab",
        [(b"@a", b"AB")], "AB")
    for linewise in [False, True]:
        case(directory, f"append-to-character-{linewise}", [('a', ":s/a/A/", linewise), ('A', "rZ", False)], "ab",
            [(b"@a", b"Zb")], "Zb")
    case(directory, "dot-explicit-register", [('a', "X", False), ('b', "Y", False)], "tail",
        [(b'"ap"b..', b'tXYYail')], "tXYYail")
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
    case(directory, "group-undo", [('a', "xx", False)], "abcdef",
        [(b"@au", None)], "abcdef")
    case(directory, "group-redo", [('a', "xx", False)], "abcdef",
        [(b"@auu", None)], "cdef")
    case(directory, "input-group", [('a', "A!\x1b0rZ", False)], "tail",
        [(b"@au", None)], "tail")
    case(directory, "open-line-group", [('a', "oX\x1bkx", False)], "abcdef",
        [(b"@au", None)], "abcdef")
    case(directory, "dot-in-macro", [('a', "x.", False)], "abcdef",
        [(b"@au", None)], "abcdef")
    case(directory, "no-change-history", [('a', "h", False)], "abc",
        [(b"rZ@au", None)], "abc")
    case(directory, "same-text-history", [('a', "rZrZ", False)], "abc",
        [(b"rZ@au", None)], "Zbc")
    case(directory, "nested-undo", [('a', "A!\x1b", False), ('b', "@a0rZ", False)], "tail",
        [(b"@bu", None)], "tail")
    case(directory, "nested-after-one", [('a', "xx", False), ('b', "x@a", False)], "abcdef",
        [(b"@bu", None)], "bcdef")
    case(directory, "nested-after-two", [('a', "x", False), ('b', "xx@a", False)], "abcdef",
        [(b"@bu", None)], "abcdef")
    case(directory, "yank-promotes-before-nesting", [('a', "x", False), ('b', "xyy@a", False)], "abcdef",
        [(b"@bu", None)], "abcdef")
    case(directory, "cancelled-operator-promotes", [('a', "x", False), ('b', "xd\x1b@a", False)], "abcdef",
        [(b"@bu", None)], "abcdef")
    case(directory, "yank-before-first-edit", [('a', "yyx", False)], "abcdef",
        [(b"@au", None)], "abcdef")
    case(directory, "yank-entry-keeps-existing-undo", [('a', "yyx", False)], "abcdef",
        [(b"rZ@au", None)], "abcdef")
    case(directory, "unchanged-ex-promotes-before-nesting", [('a', "x", False), ('b', "x:s/b/b/\n@a", True)], "abcdef\nlast",
        [(b"@bu", None)], "abcdef\nlast")
    case(directory, "ex-delete-promotes-before-nesting", [('a', "x", False), ('b', "x:d\n@a", True)], "abcdef\nsecond\nlast",
        [(b"@bu", None)], "abcdef\nsecond\nlast")
    case(directory, "error-group", [('a', "xxfzx", False)], "abcdef",
        [(b"@a", b"character not found"), (b"u", None)], "abcdef")
    for suffix, followup, expected in [
        ("keeps-edits", b"", "cdef"),
        ("undo", b"u", "abcdef"),
        ("next-edit-undo", b"rZu", "cdef"),
    ]:
        case(directory, f"ex-parse-error-{suffix}", [('a', "xx:bogus\nx", True)], "abcdef",
            [(b"@a", b"UnknownCommand"), (followup, None)], expected)
    case(directory, "undo-in-macro", [('a', "xxu", False)], "abcdef",
        [(b"@a", b"abcdef"), (b"u", None)], "cdef")
    case(directory, "undo-then-edit", [('a', "xxurZ", False)], "abcdef",
        [(b"@au", None)], "cdef")
    case(directory, "colon-undo-in-macro", [('u', ":u", True), ('a', "xx@u", False)], "abcdef",
        [(b"@au", None)], "cdef")
    case(directory, "save-in-macro", [('w', ":w", True), ('a', "xx@wx", False)], "abcdef",
        [(b"@au", None)], "abcdef")
    case(directory, "reload-in-macro", [('e', ":e!", True), ('a', "xx@exx", False)], "abcdef",
        [(b"@au", None)], "abcdef")
    case(directory, "dot-after-macro", [('a', "xx", False)], "abcdef",
        [(b"@au.", None)], "bcdef")
    case(directory, "put-after-macro", [('a', "xx", False)], "abcdef",
        [(b"@aup", None)], "abbcdef")
    case(directory, "unfinished-input", [('a', "xA!", False)], "abcdef",
        [(b"@a", b"[INSERT]"), (b"?", b"bcdef!?"),
         (b"\x1b", b"[VIEW]"), (b"u", None)], "abcdef")
    case(directory, "unfinished-replace", [('a', "xr", False)], "abcdef",
        [(b"@a", b"bcdef"), (b"Zu", None)], "bcdef")
    # Wait for Esc to be decoded before sending ':'; contiguous ESC ':' can
    # be decoded as Alt-: by the terminal input library.
    case(directory, "exhausted-insert-escape", [('a', "Aone", False)], "tail",
        [(b"@a", b"tailone"), (b"\x1b", b"[VIEW]")], "tailone")
    case(directory, "exhausted-multiline-insert-escape", [('a', "Aone\ntwo", True)], "tail",
        [(b"@a", b"5/5  col 1"), (b"\x1b", b"[VIEW]")], "tailone\ntwo\n")
    for suffix, keys, expected in [
        ("undo", b"u", "tail"),
        ("redo", b"uu", "tailone\ntwo\nend"),
    ]:
        case(directory, f"exhausted-multiline-{suffix}", [('a', "Aone\ntwo", True)], "tail",
            [(b"@a", b"5/5  col 1"), (b"end", b"end"),
             (b"\x1b", b"[VIEW]"), (keys, None)], expected)
    case(directory, "nested-exhausted-input-undo",
        [('a', "Aone", False), ('b', "xx@a", False)], "abcdef",
        [(b"@b", b"cdefone"), (b"!", b"cdefone!"),
         (b"\x1b", b"[VIEW]"), (b"u", None)], "abcdef")
    case(directory, "exhausted-replace-undo", [('a', "RXY", False)], "abcdef",
        [(b"@a", b"XYcdef"), (b"Z", b"XYZdef"),
         (b"\x1b", b"[VIEW]"), (b"u", None)], "abcdef")
    case(directory, "exhausted-change-undo", [('a', "cwX", False)], "one two",
        [(b"@a", b"X two"), (b"Y", b"XY two"),
         (b"\x1b", b"[VIEW]"), (b"u", None)], "one two")
    case(directory, "unfinished-colon", [('a', "xx:s/c/Z/", False)], "abcdef",
        [(b"@a", b":s/c/Z/"), (b"\ru", None)], "cdef")
    case(directory, "line-undo-after-one", [('a', "xU", False)], "abcdef",
        [(b"@au", None)], "bcdef")
    case(directory, "line-undo-in-group", [('a', "xxUu", False)], "abcdef",
        [(b"@au", None)], "abcdef")
    case(directory, "line-undo-after-group", [('a', "xx", False)], "abcdef",
        [(b"rZ@auU", None)], "Zbcdef")
    case(directory, "uppercase-name", [('a', "rZ", False)], "abcdef",
        [(b"@A", None)], "Zbcdef")
    case(directory, "count-first-command", [('a', "rZlrQ", False)], "abcdef",
        [(b"2@a", None)], "ZZQdef")
    case(directory, "count-overridden", [('a', "3rZ", False)], "abcdef",
        [(b"2@a", None)], "ZZZdef")
    case(directory, "count-before-home", [('a', "0rZ", False)], "abcdef",
        [(b"ll2@a", None)], "Zbcdef")
    case(directory, "count-nested", [('a', "rZ", False), ('b', "@a", False)], "abcdef",
        [(b"2@b", None)], "ZZcdef")
    case(directory, "register-prefix", [('a', "dw", False)], "abc def",
        [(b'"b@a"bP', None)], "abc def")
    case(directory, "register-overridden", [('a', '"cyw', False)], "abc def",
        [(b'"b@a"cP', None)], "abc abc def")
    case(directory, "operator-rejected", [('a', "d@ax", False)], "abcdef",
        [(b"@a", b"unsupported operator motion"), (b"rZ", None)], "Zbcdef")
    case(directory, "number-rejected", [('a', "rZ", False)], "abcdef",
        [(b"@a@1", b"invalid macro register"), (b"l@@", None)], "ZZcdef")
    case(directory, "empty-prefix-cleanup", [('a', "rZ", False)], "abcdef",
        [(b'2"b@z', b"macro register is empty"), (b"rZ", None)], "Zbcdef")
    case(directory, "previous-prefix-cleanup", [('a', "rZ", False)], "abcdef",
        [(b'2"b@@', b"no previous macro"), (b"rZ", None)], "Zbcdef")
    case(directory, "counted-x", [('a', "x", False)], "abcdef",
        [(b"2@a.", None)], "ef")
    case(directory, "named-x", [('a', "x", False)], "abcdef",
        [(b'"b3x"bP', None)], "abcdef")
    case(directory, "counted-D", [('a', "D", False)], "abcdef\nnext",
        [(b"3@a", None)], "\nnext")
    case(directory, "named-D", [('a', "D", False)], "abcdef",
        [(b'"b3@a"bP', None)], "abcdef")
    case(directory, "D-dot-count", [('a', "Dj2.", False)], "abc\ndef\nghi",
        [(b"@a", None)], "\n\nghi")
    case(directory, "counted-C", [('a', "CZ\x1b", False)], "abc\ndef\nghi",
        [(b"2@a", None)], "Z\nghi")
    case(directory, "empty-counted-C", [('a', "CX\x1b", False)], "\nnext",
        [(b"3@a", None)], "XXX\nnext")
    case(directory, "named-C", [('a', "CX\x1b", False)], "abcdef",
        [(b'"b@au"bP', None)], "abcdefabcdef")
    case(directory, "failed-C", [('a', "2CrQ", False)], "abc",
        [(b"@a", b"operator motion out of bounds"), (b"rZ", None)], "Zbc")
    case(directory, "empty-x", [('a', "xAboom\x1b", False)], "",
        [(b"@a", b"no character to delete")], "")
print("Macros: input stream, grouped undo/redo, nesting, errors, file boundaries and replay passed")
