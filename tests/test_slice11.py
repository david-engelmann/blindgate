"""Tests for the slice 11 obligations in CONTRACT.md.

Slice 11 reassembles an execve that strace split into an
unfinished line and a resumed line and judges the pair as
the one exec it is, so the close on exec closing of slice 10
lands for split pairs. A pair begun by a thread that is not
the leader of its process completes under the leader number,
and the pairing follows the link strace prints for that
renumbering, the superseded banner and the pid changed tail.

Every fixture here is synthetic and written for this test
file. The split shapes are the shapes strace 6.8 printed on
this machine for threaded captures in which spinner threads
kept the tracing busy, with fixture process ids and fixture
program paths in place of the captured ones. The live
capture test runs a small threaded python program under
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


FLAGGED = "O_RDONLY|O_DIRECTORY|O_CLOEXEC"
UNFLAGGED = "O_RDONLY|O_DIRECTORY"


def dir_open_line(path, fd, flags="O_RDONLY|O_DIRECTORY", pid=None):
    line = f'openat(AT_FDCWD, "{path}", {flags}) = {fd}'
    return f"{pid} {line}" if pid is not None else line


def fd_open_line(fd, path, result=4, pid=None):
    line = f'openat({fd}, "{path}", O_RDONLY) = {result}'
    return f"{pid} {line}" if pid is not None else line


def execve_half(pid):
    return (
        f"{pid} "
        'execve("/bin/true", ["/bin/true"], '
        "0x2db19d50 /* 55 vars */ <unfinished ...>"
    )


def execve_resumed(pid, tail=")             = 0"):
    return f"{pid} <... execve resumed>{tail}"


def clone_line(flags, child, pid=100):
    return (
        f"{pid} clone(child_stack=0x7623250e2000, "
        f"flags={flags}) = {child}"
    )


def test_split_execve_pair_closes_a_flagged_entry(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execve_half(100),
            execve_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_half_cut_inside_its_arguments_still_pairs(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            '100 execve("/bin/tr <unfinished ...>',
            '100 <... execve resumed>ue", ["/bin/true"], '
            "0x2db19d50 /* 55 vars */) = 0",
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_marker_text_inside_arguments_is_not_a_half(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            '100 execve("/bin/true", ["<unfinished ...>"], '
            "0x2db19d50 /* 55 vars */ <unfinished ...>",
            execve_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_a_pair_is_spent_once(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execve_half(100),
            execve_half(100),
            execve_resumed(100),
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execve_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_execve_with_a_failure_return_closes_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execve_half(100),
            execve_resumed(
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


def test_split_execve_leaves_the_directory_in_force_standing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            '100 chdir("src") = 0',
            execve_half(100),
            execve_resumed(100),
            '100 openat(AT_FDCWD, "app.py", O_RDONLY) = 3',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_resumed_execve_without_a_kept_half_closes_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execve_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_execve_from_a_thread_closes_under_the_leader_number(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            dir_open_line("src", 4, UNFLAGGED, pid=100),
            clone_line("CLONE_FILES|SIGCHLD", 101),
            execve_half(101),
            "100 +++ superseded by execve in pid 101 +++",
            execve_resumed(100),
            fd_open_line(3, "guide.md", result=5, pid=100),
            fd_open_line(4, "app.py", result=6, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md", "src/app.py"]
    assert outside == []


def test_a_pid_changed_half_pairs_with_its_resumed_line(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            clone_line("CLONE_FILES|SIGCHLD", 101),
            '101 execve("/bin/true", ["/bin/true"], '
            "0x2db19d50 /* 55 vars */ <pid changed to 100 ...>",
            execve_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_the_survivor_state_is_the_starting_threads_state(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 clone(child_stack=NULL, "
            "flags=CLONE_VM|SIGCHLD) = 101",
            dir_open_line("src", 4, UNFLAGGED, pid=101),
            execve_half(101),
            "100 +++ superseded by execve in pid 101 +++",
            execve_resumed(100),
            fd_open_line(4, "app.py", pid=100),
            fd_open_line(3, "guide.md", result=5, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_split_execve_under_a_shared_table_closes_for_both_holders(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            clone_line("CLONE_FILES|SIGCHLD", 101),
            execve_half(101),
            execve_resumed(101),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_kept_execve_half_dies_with_its_process(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execve_half(100),
            "100 +++ exited with 0 +++",
            execve_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_a_kept_execve_half_is_dropped_under_a_fork_child_number(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execve_half(100),
            "99 clone(child_stack=NULL, "
            "flags=CLONE_VM|SIGCHLD) = 100",
            dir_open_line("docs", 3, FLAGGED, pid=100),
            execve_resumed(100),
            fd_open_line(3, "guide.md", result=4, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_halves_of_two_processes_pair_with_their_own_process(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED, pid=100),
            dir_open_line("src", 4, UNFLAGGED, pid=200),
            execve_half(100),
            fd_open_line(4, "app.py", result=5, pid=200),
            execve_half(200),
            execve_resumed(200),
            execve_resumed(100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md", "src/app.py"]
    assert outside == []


def test_live_threaded_capture_reads_through_an_exec(tmp_path):
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
        "import os, sys, threading, time\n"
        "def spin():\n"
        "    while True:\n"
        f"        held = os.open({app!r}, os.O_RDONLY)\n"
        "        os.close(held)\n"
        "for _ in range(4):\n"
        "    threading.Thread(target=spin, daemon=True).start()\n"
        "time.sleep(0.3)\n"
        f"fd = os.open({docs!r}, os.O_RDONLY)\n"
        "os.dup2(fd, 20)\n"
        f"os.execv(sys.executable, [sys.executable, '-c', {child!r}, "
        "str(fd), '20'])\n"
    )
    reads, outside = run_traced(
        [sys.executable, "-c", program], project
    )
    assert "docs/guide.md" in reads
    assert "guide.md" not in reads
