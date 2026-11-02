"""Tests for the slice 14 obligations in CONTRACT.md.

Slice 14 reads a successful fcntl line in the F_DUPFD or
F_DUPFD_CLOEXEC shape as creating a descriptor entry at the
returned number. The entry copies the directory and inside
fact of its source entry, and its close on exec flag follows
the duplicating command rather than the source. The split
shapes are the shapes the threaded churn probe printed, with
the complete argument side on the unfinished half and only
the closing parenthesis and return on the resumed half.

Every fixture here is synthetic and written for this test
file. The live capture test runs a small python program under
strace and is guarded on strace availability.
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


def fcntl_dup_line(
    source, command, floor, result, pid=None
):
    line = f"fcntl({source}, {command}, {floor}) = {result}"
    return f"{pid} {line}" if pid is not None else line


def fcntl_set_line(fd, pid=None):
    line = f"fcntl({fd}, F_SETFD, FD_CLOEXEC) = 0"
    return f"{pid} {line}" if pid is not None else line


def fcntl_clear_line(fd, pid=None):
    line = f"fcntl({fd}, F_SETFD, 0) = 0"
    return f"{pid} {line}" if pid is not None else line


FLAGGED = "O_RDONLY|O_DIRECTORY|O_CLOEXEC"
UNFLAGGED = "O_RDONLY|O_DIRECTORY"


def test_fcntl_dupfd_creates_an_entry_at_the_returned_number(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            fcntl_dup_line(3, "F_DUPFD", 0, 7),
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fcntl_dupfd_cloexec_creates_an_entry_at_the_returned_number(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            fcntl_dup_line(3, "F_DUPFD_CLOEXEC", 0, 7),
            fd_open_line(7, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fcntl_dupfd_creates_at_the_returned_number_and_not_at_the_requested_floor(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            dir_open_line("src", 4, UNFLAGGED),
            fcntl_dup_line(3, "F_DUPFD", 4, 5),
            "fchdir(5) = 0",
            cwd_open_line("guide.md", result=8),
            "fchdir(4) = 0",
            cwd_open_line("app.py", result=9),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_fcntl_dupfd_copies_the_inside_fact_of_its_source_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    outside_dir = tmp_path / "outside"
    outside_dir.mkdir()
    (outside_dir / "secret.md").write_text(
        "outside\n", encoding="utf-8"
    )
    (project / "link").symlink_to(
        outside_dir, target_is_directory=True
    )
    log = "\n".join(
        [
            dir_open_line("link", 3, UNFLAGGED),
            fcntl_dup_line(3, "F_DUPFD", 0, 7),
            fd_open_line(7, "secret.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == []
    assert outside == [str(outside_dir / "secret.md")]


def test_fcntl_dupfd_from_a_flagged_source_creates_a_clear_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            fcntl_dup_line(3, "F_DUPFD", 0, 7),
            execve_line(),
            fd_open_line(7, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fcntl_dupfd_from_an_unflagged_source_creates_a_clear_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            fcntl_dup_line(3, "F_DUPFD", 0, 7),
            execve_line(),
            fd_open_line(7, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fcntl_dupfd_cloexec_from_an_unflagged_source_creates_a_flagged_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            fcntl_dup_line(3, "F_DUPFD_CLOEXEC", 0, 7),
            execve_line(),
            fd_open_line(7, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_dupfd_cloexec_from_a_flagged_source_creates_a_flagged_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            fcntl_dup_line(3, "F_DUPFD_CLOEXEC", 0, 7),
            execve_line(),
            fd_open_line(7, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_dupfd_after_the_source_is_reflagged_still_creates_a_clear_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            fcntl_set_line(3),
            fcntl_dup_line(3, "F_DUPFD", 0, 7),
            execve_line(),
            fd_open_line(7, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fcntl_dupfd_cloexec_after_the_source_is_cleared_still_creates_a_flagged_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            fcntl_clear_line(3),
            fcntl_dup_line(3, "F_DUPFD_CLOEXEC", 0, 7),
            execve_line(),
            fd_open_line(7, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_duplicated_entry_can_be_reflagged_on_its_own_number(
    tmp_path,
):
    project = make_project(tmp_path)
    set_after_plain = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            fcntl_dup_line(3, "F_DUPFD", 0, 7),
            fcntl_set_line(7),
            execve_line(),
            fd_open_line(7, "guide.md"),
        ]
    )
    reads, outside = parse_trace(set_after_plain, project)
    assert reads == ["guide.md"]
    assert outside == []
    clear_after_cloexec = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            fcntl_dup_line(3, "F_DUPFD_CLOEXEC", 0, 7),
            fcntl_clear_line(7),
            execve_line(),
            fd_open_line(7, "guide.md"),
        ]
    )
    reads, outside = parse_trace(clear_after_cloexec, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_closing_the_source_leaves_the_duplicate_standing(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            fcntl_dup_line(3, "F_DUPFD", 0, 7),
            "close(3) = 0",
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_failed_fcntl_dupfd_creates_no_entry(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(99, F_DUPFD, 7) = -1 EBADF "
            "(Bad file descriptor)",
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_dupfd_without_a_recorded_return_creates_no_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_DUPFD, 7)",
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_dupfd_from_an_unrecorded_source_creates_no_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            fcntl_dup_line(9, "F_DUPFD", 0, 7),
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_dupfd_cloexec_from_an_unrecorded_source_creates_no_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            fcntl_dup_line(9, "F_DUPFD_CLOEXEC", 0, 7),
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_dupfd_with_a_flags_value_is_refused_by_shape(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_DUPFD, FD_CLOEXEC) = 7",
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_dupfd_cloexec_with_a_flags_value_is_refused_by_shape(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_DUPFD_CLOEXEC, FD_CLOEXEC) = 7",
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_split_fcntl_dupfd_pair_creates_an_entry_at_the_returned_number(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 fcntl(3, F_DUPFD, 0 <unfinished ...>",
            "100 <... fcntl resumed>) = 7",
            "100 fchdir(7) = 0",
            cwd_open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_fcntl_dupfd_cloexec_pair_creates_a_flagged_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 fcntl(3, F_DUPFD_CLOEXEC, 0 <unfinished ...>",
            "100 <... fcntl resumed>) = 7",
            execve_line(pid=100),
            fd_open_line(7, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_split_fcntl_dupfd_pair_with_a_failure_return_creates_no_entry(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 fcntl(3, F_DUPFD, 0 <unfinished ...>",
            "100 <... fcntl resumed>) = -1 EBADF "
            "(Bad file descriptor)",
            "100 fchdir(7) = 0",
            cwd_open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_split_fcntl_dupfd_pair_is_spent_once(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_DUPFD, 0 <unfinished ...>",
            "<... fcntl resumed>) = 7",
            "<... fcntl resumed>) = 8",
            "fchdir(8) = 0",
            cwd_open_line("guide.md"),
            "fchdir(7) = 0",
            cwd_open_line("guide.md", result=5),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_split_fcntl_dupfd_pair_reads_the_source_at_the_resumed_position(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            "fcntl(3, F_DUPFD, 0 <unfinished ...>",
            dir_open_line("docs", 3, UNFLAGGED),
            "<... fcntl resumed>) = 7",
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_resumed_fcntl_dupfd_without_a_kept_half_creates_nothing(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "<... fcntl resumed>) = 7",
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_unfinished_fcntl_dupfd_half_that_never_resumes_creates_nothing(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_DUPFD, 0 <unfinished ...>",
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_unfinished_fcntl_dupfd_half_with_a_flags_floor_is_never_kept(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_DUPFD, FD_CLOEXEC <unfinished ...>",
            "<... fcntl resumed>) = 7",
            "fchdir(7) = 0",
            cwd_open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_dupfd_by_a_sharing_child_creates_an_entry_the_parent_holds(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 clone(child_stack=0x7623250e2000, "
            "flags=CLONE_FILES|SIGCHLD) = 101",
            fcntl_dup_line(3, "F_DUPFD", 0, 7, pid=101),
            "100 fchdir(7) = 0",
            cwd_open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fcntl_dupfd_cloexec_into_a_shared_table_is_closed_by_an_exec_from_the_other_process(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 clone(child_stack=0x7623250e2000, "
            "flags=CLONE_FILES|SIGCHLD) = 101",
            fcntl_dup_line(
                3, "F_DUPFD_CLOEXEC", 0, 7, pid=101
            ),
            execve_line(pid=100),
            fd_open_line(7, "guide.md", pid=101),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_dupfd_by_a_copy_child_creates_no_entry_for_the_parent(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 clone(child_stack=NULL, "
            "flags=CLONE_CHILD_CLEARTID|CLONE_CHILD_SETTID|SIGCHLD, "
            "child_tidptr=0x7374cca3a350) = 101",
            fcntl_dup_line(3, "F_DUPFD", 0, 7, pid=101),
            "100 fchdir(7) = 0",
            cwd_open_line("guide.md", pid=100),
            "101 fchdir(7) = 0",
            cwd_open_line("guide.md", result=5, pid=101),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_fcntl_dupfd_creates_again_at_a_number_an_exec_closed(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            execve_line(),
            dir_open_line("src", 4, UNFLAGGED),
            fcntl_dup_line(4, "F_DUPFD", 3, 3),
            "fchdir(3) = 0",
            cwd_open_line("app.py"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_live_capture_reads_a_duplicated_descriptor_through_an_exec(
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
        "dup = fcntl.fcntl(fd, fcntl.F_DUPFD, 20); "
        "os.execv(sys.executable, [sys.executable, '-c', "
        f"{child!r}, str(dup)])"
    )
    reads, outside = run_traced(
        [sys.executable, "-c", program], project
    )
    assert "docs/guide.md" in reads
    assert "guide.md" not in reads
