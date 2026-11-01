"""Tests for the slice 8 obligations in CONTRACT.md.

Slice 8 reads the flags of a clone3 line where the trace prints
them, inside the braces of its struct argument. A clone3 whose
flags carry CLONE_FILES holds one descriptor map with its
parent, and a clone3 whose flags carry CLONE_FS holds one
directory state with its parent, both by reference from the
fork line onward. A clone3 the trace does not decode keeps the
copy seeding of the earlier slices.

Every fixture here is synthetic and written for this test file.
The thread clone3 shape is verbatim from a strace capture made
on this machine, with fixture process ids in place of the
captured ones. The pointer form's argument spelling is verbatim
from a capture of a call the kernel rejected for its pointer,
with the return replaced by a success, because on this machine
the undecoded form was captured only on a failing call. The
independence shapes are derived by editing the flags list on
the verbatim struct skeleton, and the split clone3 halves are
derived from the verbatim whole line by the split the
unfinished and resumed markers name, the derivation the
earlier slices used for their halves, because a split clone3
cannot be captured on demand. The live capture test runs a
small python program under strace and is guarded on strace
availability.
"""

import shutil
import sys

import pytest

from blindgate.tracing import parse_trace, run_traced

THREAD_FLAGS = (
    "CLONE_VM|CLONE_FS|CLONE_FILES|CLONE_SIGHAND|CLONE_THREAD|"
    "CLONE_SYSVSEM|CLONE_SETTLS|CLONE_PARENT_SETTID|"
    "CLONE_CHILD_CLEARTID"
)


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


def clone3_line(flags, child, pid=100):
    line = (
        f"clone3({{flags={flags}, "
        "child_tid=0x729e32956990, parent_tid=0x729e32956990, "
        "exit_signal=0, stack=0x729e32156000, "
        "stack_size=0x7fff80, tls=0x729e329566c0} => "
        f"{{parent_tid=[{child}]}}, 88) = {child}"
    )
    return f"{pid} {line}"


def thread_clone3_line(child, pid=100):
    return clone3_line(THREAD_FLAGS, child, pid)


def plain_clone_line(child, pid=100):
    line = (
        "clone(child_stack=NULL, "
        "flags=CLONE_CHILD_CLEARTID|CLONE_CHILD_SETTID|SIGCHLD, "
        f"child_tidptr=0x7374cca3a350) = {child}"
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


def test_clone3_files_child_close_forgets_the_parent_entry(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            thread_clone3_line(101),
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_clone3_fs_moves_in_either_process_move_both(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            thread_clone3_line(101),
            chdir_line("docs", pid=101),
            open_line("guide.md", pid=100),
            chdir_line("../src", pid=100),
            open_line("app.py", pid=101, result=4),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_clone3_files_without_fs_does_not_share_the_directory(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            clone3_line("CLONE_VM|CLONE_FILES|CLONE_THREAD", 101),
            chdir_line("docs", pid=101),
            open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_clone3_fs_without_files_does_not_share_descriptors(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            clone3_line("CLONE_VM|CLONE_FS|CLONE_THREAD", 101),
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_clone3_second_fork_without_the_flag_copies_the_current_shared_state(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            thread_clone3_line(101),
            dir_open_line("src", 4, pid=101),
            plain_clone_line(102, pid=101),
            fd_open_line(4, "app.py", result=7, pid=102),
            close_line(4, pid=102),
            fd_open_line(4, "app.py", result=5, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_clone3_with_an_undecoded_struct_keeps_the_copy_reading(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 clone3(0x729e32956990, 88) = 101",
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_clone3_with_a_complete_flags_half_shares(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 clone3({flags=" + THREAD_FLAGS + ", "
            "child_tid=0x729e32956990 <unfinished ...>",
            "100 <... clone3 resumed>) = 101",
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_split_clone3_with_a_cut_flags_list_reads_as_a_copy(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 clone3({flags=CLONE_VM|CLONE_F <unfinished ...>",
            "100 <... clone3 resumed>) = 101",
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_resumed_clone3_without_a_kept_half_reads_as_a_copy(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 <... clone3 resumed>) = 101",
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_a_resumed_clone_does_not_spend_a_kept_clone3_half(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 clone3({flags=" + THREAD_FLAGS + ", "
            "child_tid=0x729e32956990 <unfinished ...>",
            "100 <... clone resumed>) = 101",
            close_line(3, pid=101),
            fd_open_line(3, "guide.md", result=5, pid=100),
            "100 <... clone3 resumed>) = 102",
            close_line(3, pid=102),
            fd_open_line(3, "guide.md", result=6, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_live_capture_of_a_thread_open_reads_through_the_shared_table(
    tmp_path,
):
    if shutil.which("strace") is None:
        pytest.skip("strace is not available")
    project = make_project(tmp_path)
    docs = str(project / "docs")
    program = (
        "import os, threading; "
        "box = []; "
        f"work = lambda: box.append(os.open({docs!r}, "
        "os.O_RDONLY | os.O_DIRECTORY)); "
        "t = threading.Thread(target=work); "
        "t.start(); t.join(); "
        "f = os.open('guide.md', os.O_RDONLY, dir_fd=box[0]); "
        "os.read(f, 10)"
    )
    reads, outside = run_traced(
        [sys.executable, "-c", program], project
    )
    assert "docs/guide.md" in reads
