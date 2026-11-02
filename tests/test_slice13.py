"""Tests for the slice 13 obligations in CONTRACT.md.

Slice 13 reads an fcntl line in the F_SETFD shape as moving
the close on exec flag of a recorded descriptor entry, so
the flag a later exec reads is the flag as last set by the
creating line or by a later fcntl. Every other fcntl
command is refused by shape, no fcntl line creates an
entry, and a split F_SETFD pair is reassembled through the
same pairing as the other split calls.

Every fixture here is synthetic and written for this test
file. The whole line shapes are the shapes strace 6.8
printed on this machine for a compiled probe that opened
flagged and unflagged directories and set and cleared their
flags by fcntl, and the split shapes are the shapes a
threaded capture of 4804 fcntl calls printed when 1814 of
them split between the entry print and the exit print. The
live capture test runs a small python program under strace
and is guarded on strace availability.
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


def cwd_open_line(path, result=4, pid=None):
    line = f'openat(AT_FDCWD, "{path}", O_RDONLY) = {result}'
    return f"{pid} {line}" if pid is not None else line


def execve_line(pid=None):
    line = (
        'execve("/usr/bin/reader", ["reader"], '
        "0x7ffd48416d98 /* 54 vars */) = 0"
    )
    return f"{pid} {line}" if pid is not None else line


def fcntl_set_line(fd, pid=None):
    line = f"fcntl({fd}, F_SETFD, FD_CLOEXEC) = 0"
    return f"{pid} {line}" if pid is not None else line


def fcntl_clear_line(fd, pid=None):
    line = f"fcntl({fd}, F_SETFD, 0) = 0"
    return f"{pid} {line}" if pid is not None else line


FLAGGED = "O_RDONLY|O_DIRECTORY|O_CLOEXEC"
UNFLAGGED = "O_RDONLY|O_DIRECTORY"


def test_fcntl_set_flags_an_unflagged_entry_so_exec_closes_it(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            fcntl_set_line(3),
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_clear_leaves_a_flagged_entry_standing_through_exec(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            fcntl_clear_line(3),
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_the_later_fcntl_stands_when_two_disagree(tmp_path):
    project = make_project(tmp_path)
    set_last = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            fcntl_set_line(3),
            fcntl_clear_line(3),
            fcntl_set_line(3),
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(set_last, project)
    assert reads == ["guide.md"]
    assert outside == []
    clear_last = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            fcntl_clear_line(3),
            fcntl_set_line(3),
            fcntl_clear_line(3),
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(clear_last, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_failed_fcntl_changes_no_flag(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            "fcntl(3, F_SETFD, 0) = -1 EBADF (Bad file descriptor)",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_without_a_recorded_return_changes_no_flag(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            "fcntl(3, F_SETFD, 0)",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fchdir_against_a_reflagged_then_exec_closed_number_moves_nothing(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            fcntl_set_line(3),
            execve_line(),
            "fchdir(3) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_setfl_changes_no_flag(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            "fcntl(3, F_SETFL, O_RDWR|O_NONBLOCK) = 0",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_getfd_changes_no_flag(tmp_path):
    project = make_project(tmp_path)
    flagged = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            "fcntl(3, F_GETFD) = 0x1 (flags FD_CLOEXEC)",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(flagged, project)
    assert reads == ["guide.md"]
    assert outside == []
    unflagged = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_GETFD) = 0",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(unflagged, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fcntl_dupfd_cloexec_creates_no_entry_and_changes_no_flag(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_DUPFD_CLOEXEC, 0) = 7",
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_on_an_unrecorded_number_creates_no_entry(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            fcntl_set_line(9),
            "fchdir(9) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_after_a_close_creates_no_entry(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "close(3) = 0",
            fcntl_set_line(3),
            "fchdir(3) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_split_fcntl_pair_sets_the_flag(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 fcntl(3, F_SETFD, FD_CLOEXEC <unfinished ...>",
            "100 <... fcntl resumed>) = 0",
            execve_line(pid=100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_split_fcntl_pair_clears_the_flag(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            "100 fcntl(3, F_SETFD, 0 <unfinished ...>",
            "100 <... fcntl resumed>) = 0",
            execve_line(pid=100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_fcntl_pair_is_spent_once(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_SETFD, FD_CLOEXEC <unfinished ...>",
            "<... fcntl resumed>) = 0",
            fcntl_clear_line(3),
            "<... fcntl resumed>) = 0",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_second_unfinished_fcntl_replaces_the_first(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            "fcntl(3, F_SETFD, FD_CLOEXEC <unfinished ...>",
            "fcntl(3, F_SETFD, 0 <unfinished ...>",
            "<... fcntl resumed>) = 0",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_resumed_fcntl_without_a_kept_half_changes_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            "<... fcntl resumed>) = 0",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_unfinished_fcntl_half_that_never_resumes_changes_nothing(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_SETFD, FD_CLOEXEC <unfinished ...>",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_fcntl_of_another_command_records_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            "fcntl(3, F_SETFL, O_RDWR|O_NONBLOCK <unfinished ...>",
            "<... fcntl resumed>) = 0",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_kept_fcntl_half_dies_with_its_process(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 fcntl(3, F_SETFD, FD_CLOEXEC <unfinished ...>",
            "100 +++ exited with 0 +++",
            "100 <... fcntl resumed>) = 0",
            execve_line(pid=100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fcntl_by_a_sharing_child_reflags_the_parents_entry(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 clone(child_stack=0x7623250e2000, "
            "flags=CLONE_FILES|SIGCHLD) = 101",
            fcntl_set_line(3, pid=101),
            execve_line(pid=100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_by_a_copy_child_leaves_the_parent_entry_flag_alone(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            "100 clone(child_stack=NULL, "
            "flags=CLONE_CHILD_CLEARTID|CLONE_CHILD_SETTID|SIGCHLD, "
            "child_tidptr=0x7374cca3a350) = 101",
            fcntl_clear_line(3, pid=101),
            execve_line(pid=101),
            fd_open_line(3, "guide.md", result=4, pid=101),
            execve_line(pid=100),
            fd_open_line(3, "guide.md", result=5, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_the_live_capture_set_records_fcntl():
    from blindgate.tracing import _TRACED_CALLS

    assert "fcntl" in _TRACED_CALLS.split(",")


def test_live_capture_reads_a_reflagged_descriptor_through_an_exec(
    tmp_path,
):
    if shutil.which("strace") is None:
        pytest.skip("strace is not available")
    project = make_project(tmp_path)
    docs = str(project / "docs")
    child = (
        "import os, sys\n"
        "held = os.open('guide.md', os.O_RDONLY, "
        "dir_fd=int(sys.argv[1]))\n"
        "os.read(held, 10)\n"
    )
    program = (
        "import fcntl, os, sys; "
        f"fd = os.open({docs!r}, os.O_RDONLY | os.O_CLOEXEC); "
        "fcntl.fcntl(fd, fcntl.F_SETFD, 0); "
        "os.execv(sys.executable, [sys.executable, '-c', "
        f"{child!r}, str(fd)])"
    )
    reads, outside = run_traced(
        [sys.executable, "-c", program], project
    )
    assert "docs/guide.md" in reads
    assert "guide.md" not in reads
