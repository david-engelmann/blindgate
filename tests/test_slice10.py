"""Tests for the slice 10 obligations in CONTRACT.md.

Slice 10 tracks the close on exec flag on the descriptor
entries the parse records and reads a successful execve as
closing the flagged entries of its process. The flag is set
by an open whose flags name O_CLOEXEC and by a dup3 whose
flags argument names it, and a product of dup or of dup2
never carries it.

Every fixture here is synthetic and written for this test
file. The execve argument texts are the shapes strace 6.8
printed on this machine for a compiled probe that opened and
duplicated flagged descriptors and execed a reader, and for
a threaded capture that split an execve three times out of
three, with fixture process ids and fixture program paths in
place of the captured ones. The live capture test runs a
small python program under strace and is guarded on strace
availability.
"""

import shutil
import sys

import pytest

from blindgate.tracing import parse_trace, run_traced


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


def dir_open_line(path, fd, flags="O_RDONLY|O_DIRECTORY", pid=None):
    line = f'openat(AT_FDCWD, "{path}", {flags}) = {fd}'
    return f"{pid} {line}" if pid is not None else line


def fd_open_line(fd, path, result=4, pid=None):
    line = f'openat({fd}, "{path}", O_RDONLY) = {result}'
    return f"{pid} {line}" if pid is not None else line


def chdir_line(path, pid=None, result=0):
    line = f'chdir("{path}") = {result}'
    return f"{pid} {line}" if pid is not None else line


def execve_line(pid=None):
    line = (
        'execve("/usr/bin/reader", ["reader"], '
        "0x7ffd48416d98 /* 54 vars */) = 0"
    )
    return f"{pid} {line}" if pid is not None else line


def failed_execve_line(pid=None):
    line = (
        'execve("/nonexistent/no-such-program", '
        '["no-such-program"], 0x7ffd48416d98 /* 54 vars */) '
        "= -1 ENOENT (No such file or directory)"
    )
    return f"{pid} {line}" if pid is not None else line


FLAGGED = "O_RDONLY|O_DIRECTORY|O_CLOEXEC"
UNFLAGGED = "O_RDONLY|O_DIRECTORY"


def test_execve_closes_a_flagged_directory_descriptor(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_execve_leaves_an_unflagged_descriptor_standing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_failed_execve_closes_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            failed_execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_execve_without_a_recorded_return_closes_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            'execve("/usr/bin/reader", ["reader"], '
            "0x7ffd48416d98 /* 54 vars */)",
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_execve_leaves_the_directory_in_force_standing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            chdir_line("src"),
            execve_line(),
            'openat(AT_FDCWD, "app.py", O_RDONLY) = 3',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_fchdir_against_an_exec_closed_number_moves_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            execve_line(),
            "fchdir(3) = 0",
            'openat(AT_FDCWD, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_dup_of_a_flagged_entry_survives_exec(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            "dup(3) = 4",
            execve_line(),
            fd_open_line(4, "guide.md", result=5),
            fd_open_line(3, "guide.md", result=6),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_dup2_of_a_flagged_entry_survives_exec(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            "dup2(3, 4) = 4",
            execve_line(),
            fd_open_line(4, "guide.md", result=5),
            fd_open_line(3, "guide.md", result=6),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_dup3_with_the_flag_closes_at_exec(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "dup3(3, 9, O_CLOEXEC) = 9",
            execve_line(),
            fd_open_line(9, "guide.md", result=5),
            fd_open_line(3, "guide.md", result=6),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_dup3_without_the_flag_survives_exec(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            "dup3(3, 9, 0) = 9",
            execve_line(),
            fd_open_line(9, "guide.md", result=5),
            fd_open_line(3, "guide.md", result=6),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_dup3_onto_a_held_number_replaces_its_flag(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            dir_open_line("src", 4, UNFLAGGED),
            "dup3(4, 3, O_CLOEXEC) = 3",
            execve_line(),
            fd_open_line(3, "app.py", result=5),
            fd_open_line(4, "app.py", result=6),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["app.py", "src/app.py"]
    assert outside == []


def test_exec_by_a_sharing_child_closes_the_flagged_entry_for_the_parent(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            "100 clone(child_stack=0x7623250e2000, "
            "flags=CLONE_FILES|SIGCHLD) = 101",
            execve_line(pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_exec_by_a_copy_child_leaves_the_parent_entry_standing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            "100 clone(child_stack=NULL, "
            "flags=CLONE_CHILD_CLEARTID|CLONE_CHILD_SETTID|SIGCHLD, "
            "child_tidptr=0x7374cca3a350) = 101",
            execve_line(pid=101),
            fd_open_line(3, "guide.md", result=4, pid=101),
            fd_open_line(3, "guide.md", result=5, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_split_execve_pair_closes_the_flagged_entry(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            '100 execve("/bin/true", ["/bin/true"], '
            "0x2db19d50 /* 55 vars */ <unfinished ...>",
            "100 <... execve resumed>)             = 0",
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_live_capture_reads_through_an_exec(tmp_path):
    if shutil.which("strace") is None:
        pytest.skip("strace is not available")
    from blindgate.tracing import _TRACED_CALLS

    assert "execve" in _TRACED_CALLS.split(",")
    project = make_project(tmp_path)
    docs = str(project / "docs")
    child = (
        "import os, sys\n"
        "flagged, keep = int(sys.argv[1]), int(sys.argv[2])\n"
        "try:\n"
        "    os.open('guide.md', os.O_RDONLY, dir_fd=flagged)\n"
        "except OSError:\n"
        "    pass\n"
        "held = os.open('guide.md', os.O_RDONLY, dir_fd=keep)\n"
        "os.read(held, 10)\n"
    )
    program = (
        "import os, sys; "
        f"fd = os.open({docs!r}, os.O_RDONLY); "
        "os.dup2(fd, 20); "
        f"os.execv(sys.executable, [sys.executable, '-c', {child!r}, "
        "str(fd), '20'])"
    )
    reads, outside = run_traced(
        [sys.executable, "-c", program], project
    )
    assert "docs/guide.md" in reads
    assert "guide.md" not in reads
