"""Tests for the slice 9 obligations in CONTRACT.md.

Slice 9 derives the keep patterns for split lines from the whole
line patterns of their calls. Both readings of a covered call are
built from the one argument pattern that call holds, so the two
readings cannot drift apart the way the hand written twins did
when the dup3 keep pattern narrowed to exactly two numbers.

Every fixture here is synthetic and written for this test file.
The traces are hand written strace lines in the documented split
format, and the shapes in the tables are the shapes the earlier
slices already use in their tests, with the dup3 flags shape
first among them.
"""

from pathlib import Path

from blindgate import tracing
from blindgate.tracing import parse_trace


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text(
        "the guide\n", encoding="utf-8"
    )
    (project / "src").mkdir()
    (project / "src" / "app.py").write_text("x = 1\n", encoding="utf-8")
    (project / "guide.md").write_text("root guide\n", encoding="utf-8")
    return project


def open_line(path, pid=None, result=3):
    line = f'openat(AT_FDCWD, "{path}", O_RDONLY) = {result}'
    return f"{pid} {line}" if pid is not None else line


def dir_open_line(path, fd, pid=None):
    line = f'openat(AT_FDCWD, "{path}", O_RDONLY|O_DIRECTORY) = {fd}'
    return f"{pid} {line}" if pid is not None else line


def fd_open_line(fd, path, pid=None, result=4):
    line = f'openat({fd}, "{path}", O_RDONLY) = {result}'
    return f"{pid} {line}" if pid is not None else line


def unfinished(call_text, pid=None):
    line = f"{call_text} <unfinished ...>"
    return f"{pid} {line}" if pid is not None else line


def resumed(name, tail, pid=None):
    line = f"<... {name} resumed> {tail}"
    return f"{pid} {line}" if pid is not None else line


_WHOLE_RES = {
    "chdir": tracing._CHDIR_RE,
    "fchdir": tracing._FCHDIR_RE,
    "close": tracing._CLOSE_RE,
    "dup": tracing._DUP_RE,
    "dup2": tracing._DUP2_RE,
    "dup3": tracing._DUP3_RE,
}


def test_every_covered_call_holds_one_argument_pattern():
    covered = {"chdir", "fchdir", "close", "dup", "dup2", "dup3"}
    assert set(tracing._CALL_ARGUMENTS) == covered
    assert set(tracing._SPLIT_PREFIX_RES) == covered
    for name, (arguments, _tail) in tracing._CALL_ARGUMENTS.items():
        assert arguments in tracing._SPLIT_PREFIX_RES[name].pattern
        assert arguments in _WHOLE_RES[name].pattern
    dup3_arguments = tracing._CALL_ARGUMENTS["dup3"][0]
    assert r"(?:\s*,\s*[A-Za-z0-9_|]+)?" in dup3_arguments


def test_the_keep_and_whole_patterns_are_built_from_the_one_holding():
    for name, (arguments, tail) in tracing._CALL_ARGUMENTS.items():
        whole = tracing._LINE_PREFIX + name + r"\(" + arguments + tail
        keep = tracing._LINE_PREFIX + name + r"\(" + arguments + r"\s*$"
        assert _WHOLE_RES[name].pattern == whole
        assert tracing._SPLIT_PREFIX_RES[name].pattern == keep
    source = Path(tracing.__file__).read_text(encoding="utf-8")
    assert source.count("_CALL_ARGUMENTS = {") == 1


_KEEP_TABLE = [
    ("chdir", 'chdir("docs"', True),
    ("chdir", 'chdir("do', False),
    ("chdir", "chdir(", False),
    ("chdir", 'chdir("docs")', False),
    ("chdir", 'chdir("docs",', False),
    ("fchdir", "fchdir(3", True),
    ("fchdir", "fchdir(", False),
    ("fchdir", "fchdir(x", False),
    ("fchdir", "fchdir(3)", False),
    ("close", "close(3", True),
    ("close", "close(", False),
    ("close", "close(3, 4", False),
    ("close", "close(3)", False),
    ("dup", "dup(3", True),
    ("dup", "dup(", False),
    ("dup", "dup(3, 4", False),
    ("dup", "dup(3)", False),
    ("dup2", "dup2(3, 9", True),
    ("dup2", "dup2(3", False),
    ("dup2", "dup2(3,", False),
    ("dup2", "dup2(3, 9, 1", False),
    ("dup2", "dup2(3, 9)", False),
    ("dup3", "dup3(3, 9, O_CLOEXEC", True),
    ("dup3", "dup3(3, 9", True),
    ("dup3", "dup3(3, 9, O_CLOEXEC|O_NONBLOCK", True),
    ("dup3", "dup3(3", False),
    ("dup3", "dup3(3, 9,", False),
    ("dup3", "dup3(3, 9, O_CLOEXEC, 1", False),
    ("dup3", "dup3(3, 9)", False),
    ("open", 'open("docs", O_RDONLY', True),
    ("open", 'open("docs"', False),
    ("open", 'open("do', False),
    ("open", 'open("docs", ', False),
    ("openat", 'openat(AT_FDCWD, "src/app.py", O_RDONLY', True),
    ("openat", 'openat(AT_FDCWD, "src/app.py"', False),
    ("openat", 'openat(AT_FDCWD, "src/app', False),
    ("openat", 'openat(AT_FDCWD, "src/app.py", ', False),
]


def test_keep_verdicts_for_the_covered_calls():
    for name, half, expected in _KEEP_TABLE:
        line = f"100 {half} <unfinished ...>"
        kept = tracing._kept_unfinished(line)
        if expected:
            assert kept is not None, line
            assert kept[0] == name, line
        else:
            assert kept is None, line


def test_split_dup3_with_its_flags_argument_still_pairs(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            'openat(AT_FDCWD, "docs", O_RDONLY|O_DIRECTORY) = 3',
            "dup3(3, 9, O_CLOEXEC <unfinished ...>",
            "<... dup3 resumed>) = 9",
            'openat(9, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


_WHOLE_TABLE = [
    (
        ['100 chdir("docs") = 0', open_line("guide.md", pid=100)],
        ["docs/guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            "100 fchdir(3) = 0",
            open_line("guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            "100 close(3) = 0",
            fd_open_line(3, "guide.md", pid=100),
        ],
        ["guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            "100 dup(3) = 7",
            fd_open_line(7, "guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            "100 dup2(3, 9) = 9",
            fd_open_line(9, "guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            "100 dup3(3, 9, O_CLOEXEC) = 9",
            fd_open_line(9, "guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            "100 dup3(3, 9) = 9",
            fd_open_line(9, "guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            '100 open("docs", O_RDONLY|O_DIRECTORY) = 3',
            fd_open_line(3, "guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [open_line("src/app.py", pid=100)],
        ["src/app.py"],
    ),
]


def test_whole_line_shapes_are_judged_as_their_calls(tmp_path):
    project = make_project(tmp_path)
    for lines, expected in _WHOLE_TABLE:
        reads, outside = parse_trace("\n".join(lines), project)
        assert reads == expected, lines
        assert outside == [], lines


_PAIR_TABLE = [
    (
        [
            unfinished('chdir("docs"', pid=100),
            resumed("chdir", ") = 0", pid=100),
            open_line("guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            unfinished("fchdir(3", pid=100),
            resumed("fchdir", ") = 0", pid=100),
            open_line("guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            unfinished("close(3", pid=100),
            resumed("close", ") = 0", pid=100),
            fd_open_line(3, "guide.md", pid=100),
        ],
        ["guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            unfinished("dup(3", pid=100),
            resumed("dup", ") = 7", pid=100),
            fd_open_line(7, "guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            unfinished("dup2(3, 9", pid=100),
            resumed("dup2", ") = 9", pid=100),
            fd_open_line(9, "guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            unfinished("dup3(3, 9, O_CLOEXEC", pid=100),
            resumed("dup3", ") = 9", pid=100),
            fd_open_line(9, "guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            dir_open_line("docs", 3, pid=100),
            unfinished("dup3(3, 9", pid=100),
            resumed("dup3", ") = 9", pid=100),
            fd_open_line(9, "guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            unfinished('open("docs", O_RDONLY', pid=100),
            resumed("open", ") = 3", pid=100),
            fd_open_line(3, "guide.md", pid=100),
        ],
        ["docs/guide.md"],
    ),
    (
        [
            unfinished(
                'openat(AT_FDCWD, "src/app.py", O_RDONLY', pid=100
            ),
            resumed("openat", ") = 3", pid=100),
        ],
        ["src/app.py"],
    ),
]


def test_kept_prefixes_pair_end_to_end(tmp_path):
    project = make_project(tmp_path)
    for lines, expected in _PAIR_TABLE:
        reads, outside = parse_trace("\n".join(lines), project)
        assert reads == expected, lines
        assert outside == [], lines
