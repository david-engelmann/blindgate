"""Tests for the slice 12 obligations in CONTRACT.md.

Slice 12 reads execveat with the discipline slices 10 and
11 gave execve, so the close on exec closing lands for a
whole execveat line and for a split execveat pair alike,
including the renumbered pairs a thread that is not the
leader of its process produces. The banner that states
the renumbering names execve whatever call of the family
renumbered the process, and the parse follows it for both
calls.

Every fixture here is synthetic and written for this test
file. The execveat shapes are the shapes strace 6.8
printed on this machine for probes that called execveat
through the libc syscall, whole and split and renumbered
and failed, with fixture process ids and fixture program
paths in place of the captured ones. The live capture
test runs a small threaded python program under strace
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


FLAGGED = "O_RDONLY|O_DIRECTORY|O_CLOEXEC"
UNFLAGGED = "O_RDONLY|O_DIRECTORY"


def dir_open_line(path, fd, flags="O_RDONLY|O_DIRECTORY", pid=None):
    line = f'openat(AT_FDCWD, "{path}", {flags}) = {fd}'
    return f"{pid} {line}" if pid is not None else line


def fd_open_line(fd, path, result=4, pid=None):
    line = f'openat({fd}, "{path}", O_RDONLY) = {result}'
    return f"{pid} {line}" if pid is not None else line


def clone_line(flags, child, pid=100):
    return (
        f"{pid} clone(child_stack=0x7623250e2000, "
        f"flags={flags}) = {child}"
    )


def execveat_line(pid=None, flags="0", result="0"):
    line = (
        'execveat(3, "true", ["true"], '
        f"0x2db19d50 /* 55 vars */, {flags}) = {result}"
    )
    return f"{pid} {line}" if pid is not None else line


def execveat_half(pid):
    return (
        f"{pid} "
        'execveat(3, "true", ["true"], '
        "0x2db19d50 /* 55 vars */, 0 <unfinished ...>"
    )


def execveat_resumed(pid, tail=")             = 0"):
    return f"{pid} <... execveat resumed>{tail}"


def execve_half(pid):
    return (
        f"{pid} "
        'execve("/bin/true", ["/bin/true"], '
        "0x2db19d50 /* 55 vars */ <unfinished ...>"
    )


def execve_resumed(pid, tail=")             = 0"):
    return f"{pid} <... execve resumed>{tail}"


def test_execveat_closes_a_flagged_directory_descriptor(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            execveat_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_execveat_leaves_an_unflagged_descriptor_standing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            execveat_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_failed_execveat_closes_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            execveat_line(
                result="-1 ENOENT (No such file or directory)"
            ),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_execveat_without_a_recorded_return_closes_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            'execveat(3, "true", ["true"], '
            "0x2db19d50 /* 55 vars */, 0)",
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_execveat_leaves_the_directory_in_force_standing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            '100 chdir("src") = 0',
            execveat_line(pid=100),
            '100 openat(AT_FDCWD, "app.py", O_RDONLY) = 3',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_execveat_with_an_empty_path_and_the_empty_path_flag_closes(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            'execveat(3, "", ["true"], '
            "0x2db19d50 /* 55 vars */, AT_EMPTY_PATH) = 0",
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_execveat_with_named_flags_closes_like_a_zero_flags_success(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            execveat_line(flags="AT_SYMLINK_NOFOLLOW"),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fchdir_against_an_execveat_closed_number_moves_nothing(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            execveat_line(),
            "fchdir(3) = 0",
            'openat(AT_FDCWD, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_execveat_by_a_sharing_child_closes_the_flagged_entry_for_the_parent(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            clone_line("CLONE_FILES|SIGCHLD", 101),
            execveat_line(pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_execveat_by_a_copy_child_leaves_the_parent_entry_standing(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            clone_line("CLONE_VM|SIGCHLD", 101),
            execveat_line(pid=101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_execveat_pair_closes_a_flagged_entry(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execveat_half(100),
            execveat_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_an_execveat_half_cut_inside_its_arguments_still_pairs(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            '100 execveat(3, "tr <unfinished ...>',
            '100 <... execveat resumed>ue", ["true"], '
            "0x2db19d50 /* 55 vars */, 0) = 0",
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_marker_text_inside_execveat_arguments_is_not_a_half(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            '100 execveat(3, "<unfinished ...>", ["true"], '
            "0x2db19d50 /* 55 vars */, 0 <unfinished ...>",
            execveat_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_an_execveat_pair_is_spent_once(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execveat_half(100),
            execveat_half(100),
            execveat_resumed(100),
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execveat_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_a_resumed_execveat_does_not_spend_a_kept_execve_half(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execve_half(100),
            execveat_resumed(100),
            fd_open_line(3, "guide.md", result=5, pid=100),
            execve_resumed(100),
            fd_open_line(3, "guide.md", result=6, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_a_resumed_execve_does_not_spend_a_kept_execveat_half(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execveat_half(100),
            execve_resumed(100),
            fd_open_line(3, "guide.md", result=5, pid=100),
            execveat_resumed(100),
            fd_open_line(3, "guide.md", result=6, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_split_execveat_with_a_failure_return_closes_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execveat_half(100),
            execveat_resumed(
                100,
                ")             = -1 ENOENT "
                "(No such file or directory)",
            ),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_execveat_leaves_the_directory_in_force_standing(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            '100 chdir("src") = 0',
            execveat_half(100),
            execveat_resumed(100),
            '100 openat(AT_FDCWD, "app.py", O_RDONLY) = 3',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_resumed_execveat_without_a_kept_half_closes_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execveat_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_execveat_from_a_thread_closes_under_the_leader_number(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            dir_open_line("src", 4, UNFLAGGED, pid=100),
            clone_line("CLONE_FILES|SIGCHLD", 101),
            execveat_half(101),
            "100 +++ superseded by execve in pid 101 +++",
            execveat_resumed(100),
            fd_open_line(3, "guide.md", result=5, pid=100),
            fd_open_line(4, "app.py", result=6, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md", "src/app.py"]
    assert outside == []


def test_a_pid_changed_execveat_half_pairs_with_its_resumed_line(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            clone_line("CLONE_FILES|SIGCHLD", 101),
            '101 execveat(3, "true", ["true"], '
            "0x2db19d50 /* 55 vars */, 0 <pid changed to 100 ...>",
            execveat_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_execveat_survivor_state_is_the_starting_threads_state(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 clone(child_stack=NULL, "
            "flags=CLONE_VM|SIGCHLD) = 101",
            dir_open_line("src", 4, UNFLAGGED, pid=101),
            execveat_half(101),
            "100 +++ superseded by execve in pid 101 +++",
            execveat_resumed(100),
            fd_open_line(4, "app.py", pid=100),
            fd_open_line(3, "guide.md", result=5, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_the_banner_carries_the_starting_threads_directory_for_execveat(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 clone(child_stack=NULL, "
            "flags=CLONE_VM|SIGCHLD) = 101",
            '101 chdir("src") = 0',
            execveat_half(101),
            "100 +++ superseded by execve in pid 101 +++",
            execveat_resumed(100),
            '100 openat(AT_FDCWD, "app.py", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_an_unrecorded_starter_banner_empties_the_leader_for_execveat(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            execveat_half(102),
            "100 +++ superseded by execve in pid 102 +++",
            execveat_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_tail_only_execveat_pair_adopts_the_starting_threads_state(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 clone(child_stack=NULL, "
            "flags=CLONE_VM|SIGCHLD) = 101",
            dir_open_line("src", 4, UNFLAGGED, pid=101),
            '101 execveat(3, "true", ["true"], '
            "0x2db19d50 /* 55 vars */, 0 <pid changed to 100 ...>",
            execveat_resumed(100),
            fd_open_line(4, "app.py", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_split_execveat_under_a_shared_table_closes_for_both_holders(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            clone_line("CLONE_FILES|SIGCHLD", 101),
            execveat_half(101),
            execveat_resumed(101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_kept_execveat_half_dies_with_its_process(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execveat_half(100),
            "100 +++ exited with 0 +++",
            execveat_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_a_kept_execveat_half_is_dropped_under_a_fork_child_number(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execveat_half(100),
            "99 clone(child_stack=NULL, "
            "flags=CLONE_VM|SIGCHLD) = 100",
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execveat_resumed(100),
            fd_open_line(3, "guide.md", result=4, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_the_live_capture_set_records_execveat():
    from blindgate.tracing import _TRACED_CALLS

    assert "execveat" in _TRACED_CALLS.split(",")


def test_live_threaded_capture_reads_through_an_execveat(tmp_path):
    if shutil.which("strace") is None:
        pytest.skip("strace is not available")
    project = make_project(tmp_path)
    docs = str(project / "docs")
    app = str(project / "src" / "app.py")
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
        "import ctypes, os, sys, threading, time\n"
        "libc = ctypes.CDLL('libc.so.6', use_errno=True)\n"
        "def spin():\n"
        f"    held = os.open({app!r}, os.O_RDONLY)\n"
        "    while True:\n"
        "        os.fstat(held)\n"
        "for _ in range(4):\n"
        "    threading.Thread(target=spin, daemon=True).start()\n"
        "time.sleep(0.3)\n"
        f"fd = os.open({docs!r}, os.O_RDONLY)\n"
        "os.dup2(fd, 20)\n"
        "def work():\n"
        "    exe = sys.executable\n"
        "    folder = os.open(os.path.dirname(exe), "
        "os.O_RDONLY | os.O_DIRECTORY)\n"
        f"    argv = [exe, '-c', {child!r}, str(fd), '20']\n"
        "    cargv = (ctypes.c_char_p * (len(argv) + 1))(\n"
        "        *[item.encode() for item in argv], None)\n"
        "    env = [key + '=' + value for key, value "
        "in os.environ.items()]\n"
        "    cenv = (ctypes.c_char_p * (len(env) + 1))(\n"
        "        *[item.encode() for item in env], None)\n"
        "    libc.syscall(322, folder, "
        "os.path.basename(exe).encode(), cargv, cenv, 0)\n"
        "    os._exit(1)\n"
        "worker = threading.Thread(target=work)\n"
        "worker.start()\n"
        "worker.join()\n"
    )
    reads, outside = run_traced(
        [sys.executable, "-c", program], project
    )
    assert "docs/guide.md" in reads
    assert "guide.md" not in reads
