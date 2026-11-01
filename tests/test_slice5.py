"""Tests for the slice 5 obligations in CONTRACT.md.

Slice 5 tracks directory file descriptors through the trace. A
descriptor is recorded when an open returns it for a path that
resolves to a directory on disk, a relative openat against a
recorded descriptor resolves at that directory, an fchdir against
a recorded descriptor moves the directory in force, and close,
dup, and fork lines keep the map honest.

Every fixture here is synthetic and written for this test file.
The traces are hand written strace lines, never captured from a
real session, apart from the live capture tests which run a small
python program under strace and are guarded on strace
availability. Symlink fixtures are built by the tests themselves
in temp dirs and skipped when the platform cannot create them.
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


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text(
        "the guide\n", encoding="utf-8"
    )
    (project / "src").mkdir()
    (project / "src" / "app.py").write_text("x = 1\n", encoding="utf-8")
    return project


def make_symlink(target, link):
    try:
        os.symlink(target, link)
    except OSError as exc:
        pytest.skip(f"cannot create symlinks on this platform, {exc}")


def test_tracked_descriptor_resolves_a_relative_open(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3),
            chdir_line("src"),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_unconfirmable_directory_records_no_descriptor(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("gone", 3),
            chdir_line("docs"),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_plain_open_line_records_a_descriptor(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            'open("docs", O_RDONLY) = 3',
            chdir_line("src"),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_outside_descriptor_read_is_named_when_the_handle_came_from_inside(
    tmp_path,
):
    project = make_project(tmp_path)
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    (elsewhere / "secret.txt").write_text("outside\n", encoding="utf-8")
    make_symlink(elsewhere, project / "escape")
    log = "\n".join(
        [
            dir_open_line("escape", 3),
            fd_open_line(3, "secret.txt"),
        ]
    )
    reads, outside = parse_trace(log, project)
    expected = (elsewhere.resolve() / "secret.txt").as_posix()
    assert reads == []
    assert outside == [expected]


def test_outside_descriptor_opened_outside_drops_unnamed(tmp_path):
    project = make_project(tmp_path)
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    (elsewhere / "secret.txt").write_text("outside\n", encoding="utf-8")
    log = "\n".join(
        [
            dir_open_line(str(elsewhere), 3),
            fd_open_line(3, "secret.txt"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == []
    assert outside == []


def test_untracked_descriptor_falls_back_to_the_directory_in_force(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            chdir_line("docs"),
            fd_open_line(7, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fchdir_round_trip_lands_at_the_descriptor_directory(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3),
            "fchdir(3) = 0",
            open_line("guide.md", result=4),
            chdir_line(str(project)),
            open_line("src/app.py", result=5),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_fchdir_against_an_untracked_descriptor_moves_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            chdir_line("docs"),
            "fchdir(9) = 0",
            open_line("guide.md", result=4),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_failed_fchdir_moves_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3),
            "fchdir(3) = -1 EBADF (Bad file descriptor)",
            open_line("docs/guide.md", result=4),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_close_and_reuse_forgets_the_stale_directory(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3),
            chdir_line("src"),
            "close(3) = 0",
            "fchdir(3) = 0",
            open_line("app.py", result=4),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []
    log = "\n".join(
        [
            dir_open_line("docs", 3),
            "close(3) = 0",
            dir_open_line("src", 3),
            fd_open_line(3, "app.py"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_reopening_a_number_on_a_file_replaces_the_entry(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3),
            open_line("src/app.py", result=3),
            chdir_line("src"),
            fd_open_line(3, "app.py"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_fork_seeds_a_copy_of_the_descriptor_map(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            "100 clone(child_stack=NULL, flags=CLONE_CHILD_CLEARTID) = 101",
            fd_open_line(3, "guide.md", pid=101),
            "101 close(3) = 0",
            fd_open_line(3, "guide.md", result=5, pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_dup_carries_the_directory_to_the_new_number(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3),
            "dup(3) = 5",
            fd_open_line(5, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_dup2_carries_the_directory_to_the_new_number(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3),
            "dup2(3, 9) = 9",
            "close(3) = 0",
            "fchdir(9) = 0",
            open_line("guide.md", result=4),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_dup2_from_an_untracked_source_forgets_the_target(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3),
            "dup2(7, 3) = 3",
            chdir_line("src"),
            fd_open_line(3, "app.py"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_run_capture_reads_through_a_descriptor(tmp_path):
    if shutil.which("strace") is None:
        pytest.skip("strace is not available")
    project = make_project(tmp_path)
    docs = str(project / "docs")
    program = (
        f"import os; fd = os.open({docs!r}, os.O_RDONLY); "
        "f = os.open('guide.md', os.O_RDONLY, dir_fd=fd); "
        "os.read(f, 10)"
    )
    reads, outside = run_traced([sys.executable, "-c", program], project)
    assert "docs/guide.md" in reads


def test_run_capture_follows_a_dup2_and_fchdir(tmp_path):
    if shutil.which("strace") is None:
        pytest.skip("strace is not available")
    project = make_project(tmp_path)
    docs = str(project / "docs")
    program = (
        f"import os; fd = os.open({docs!r}, os.O_RDONLY); "
        "os.dup2(fd, 9); os.fchdir(9); "
        "f = open('guide.md'); f.read()"
    )
    reads, outside = run_traced([sys.executable, "-c", program], project)
    assert "docs/guide.md" in reads
