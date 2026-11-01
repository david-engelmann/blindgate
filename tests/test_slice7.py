"""Tests for the slice 7 obligations in CONTRACT.md.

Slice 7 decides what a clone shares from the flags its line
prints. A clone carrying CLONE_FILES holds one descriptor map
with its parent, and a clone carrying CLONE_FS holds one
directory state with its parent, both by reference from the
fork line onward. Every other fork keeps the copy seeding of
the earlier slices.

Every fixture here is synthetic and written for this test file.
The clone argument texts are verbatim from strace captures made
on this machine, with fixture process ids in place of the
captured ones. The plain fork shape is the clone line a python
os.fork printed, the sharing shape is the clone line a small C
program printed when made with CLONE_FS and CLONE_FILES, the
clone3 shape is the line a python thread printed, and the vfork
shape is the line a python subprocess printed. The split clone
halves are derived from the verbatim whole line by the split
the unfinished and resumed markers name, the derivation slice 6
used for its halves, because a split clone cannot be captured
on demand. The live capture test runs a small python program
under strace and is guarded on strace availability.
"""

import os
import shutil
import sys

import pytest

from blindgate.tracing import parse_trace, run_traced


def dir_open_line(path, fd, pid=None):
    line = f'openat(AT_FDCWD, "{path}", O_RDONLY|O_DIRECTORY) = {fd}'
    return f"{pid} {line}" if pid is not None else line


def fd_open_line(fd, path, result=4, pid=None):
    line = f'openat({fd}, "{path}", O_RDONLY) = {result}'
    return f"{pid} {line}" if pid is not None else line


def open_line(path, pid=None, result=3):
    line = f'openat(AT_FDCWD, "{path}", O_RDONLY) = {result}'
    return f"{pid} {line}" if pid is not None else line


def chdir_line(path, pid=None, result=0):
    line = f'chdir("{path}") = {result}'
    return f"{pid} {line}" if pid is not None else line


def close_line(fd, pid=None, result=0):
    line = f"close({fd}) = {result}"
    return f"{pid} {line}" if pid is not None else line


def clone_line(flags, child, pid=100):
    line = (
        f"clone(child_stack=NULL, flags={flags}, "
        f"child_tidptr=0x7374cca3a350) = {child}"
    )
    return f"{pid} {line}"


def fork_clone_line(child, pid=100):
    return clone_line(
        "CLONE_CHILD_CLEARTID|CLONE_CHILD_SETTID|SIGCHLD", child, pid
    )


def share_clone_line(child, pid=100):
    line = (
        "clone(child_stack=0x7623250e2000, "
        f"flags=CLONE_FS|CLONE_FILES|SIGCHLD) = {child}"
    )
    return f"{pid} {line}"


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text(
        "the guide\n", encoding="utf-8"
    )
    (project / "src").mkdir()
    (project / "src" / "app.py").write_text(
        "x = 1\n", encoding="utf-8"
    )
    (project / "guide.md").write_text(
        "the root guide\n", encoding="utf-8"
    )
    return project


def test_clone_files_child_close_forgets_the_parent_entry(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            clone_line("CLONE_FILES|SIGCHLD", 101),
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_clone_files_child_open_is_visible_to_the_parent(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            clone_line("CLONE_FILES|SIGCHLD", 101),
            dir_open_line("docs", 3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_clone_files_parent_open_after_the_fork_is_visible_to_the_child(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            clone_line("CLONE_FILES|SIGCHLD", 101),
            dir_open_line("docs", 3, pid=100),
            fd_open_line(3, "guide.md", pid=101),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_clone_without_clone_files_keeps_the_copy_reading(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            fork_clone_line(101),
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_clone_fs_moves_in_either_process_move_both(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            clone_line("CLONE_FS|SIGCHLD", 101),
            chdir_line("docs", pid=101),
            open_line("guide.md", pid=100),
            chdir_line("../src", pid=100),
            open_line("app.py", pid=101, result=4),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_clone_without_clone_fs_diverges_after_the_fork(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            fork_clone_line(101),
            chdir_line("docs", pid=101),
            open_line("guide.md", pid=101),
            open_line("guide.md", pid=100, result=4),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_clone_files_without_clone_fs_does_not_share_the_directory(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            clone_line("CLONE_FILES|SIGCHLD", 101),
            chdir_line("docs", pid=101),
            open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_clone_fs_without_clone_files_does_not_share_descriptors(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            clone_line("CLONE_FS|SIGCHLD", 101),
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_a_second_fork_without_the_flag_copies_the_current_shared_state(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            clone_line("CLONE_FILES|SIGCHLD", 101),
            dir_open_line("src", 4, pid=101),
            fork_clone_line(102, pid=101),
            fd_open_line(4, "app.py", result=7, pid=102),
            close_line(4, pid=102),
            fd_open_line(4, "app.py", result=5, pid=100),
            fd_open_line(3, "guide.md", result=6, pid=102),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_a_second_fork_with_the_flag_joins_the_shared_state(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            clone_line("CLONE_FILES|SIGCHLD", 101),
            clone_line("CLONE_FILES|SIGCHLD", 102, pid=101),
            close_line(3, pid=102),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_clone3_keeps_the_copy_reading(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 clone3({flags=CLONE_VM|CLONE_FS|CLONE_FILES|"
            "CLONE_SIGHAND|CLONE_THREAD|CLONE_SYSVSEM|CLONE_SETTLS|"
            "CLONE_PARENT_SETTID|CLONE_CHILD_CLEARTID, "
            "child_tid=0x7976e800d990, parent_tid=0x7976e800d990, "
            "exit_signal=0, stack=0x7976e780d000, "
            "stack_size=0x7fff80, tls=0x7976e800d6c0} => "
            "{parent_tid=[101]}, 88) = 101",
            close_line(3, pid=101),
            chdir_line("docs", pid=101),
            fd_open_line(3, "guide.md", pid=100),
            open_line("guide.md", pid=100, result=5),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_vfork_keeps_the_copy_reading(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 vfork() = 101",
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_clone_without_a_flags_argument_keeps_the_copy_reading(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 clone() = 101",
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_clone_with_a_readable_flags_half_shares(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 clone(child_stack=NULL, "
            "flags=CLONE_FILES|CLONE_CHILD_CLEARTID|"
            "CLONE_CHILD_SETTID|SIGCHLD, "
            "child_tidptr=0x7374cca3a350 <unfinished ...>",
            "100 <... clone resumed>) = 101",
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_split_clone_with_a_cut_flags_list_reads_as_a_copy(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 clone(child_stack=NULL, flags=CLONE_FILES "
            "<unfinished ...>",
            "100 <... clone resumed>) = 101",
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_resumed_clone_without_a_kept_half_reads_as_a_copy(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 <... clone resumed>) = 101",
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_live_capture_of_a_plain_fork_reads_the_child_close_as_its_own(
    tmp_path,
):
    if shutil.which("strace") is None:
        pytest.skip("strace is not available")
    project = make_project(tmp_path)
    docs = str(project / "docs")
    program = (
        f"import os; fd = os.open({docs!r}, "
        "os.O_RDONLY | os.O_DIRECTORY); "
        "pid = os.fork(); "
        "pid == 0 and (os.close(fd), os._exit(0)); "
        "os.waitpid(pid, 0); "
        "f = os.open('guide.md', os.O_RDONLY, dir_fd=fd); "
        "os.read(f, 10)"
    )
    reads, outside = run_traced(
        [sys.executable, "-c", program], project
    )
    assert "docs/guide.md" in reads
