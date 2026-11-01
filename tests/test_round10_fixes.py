"""Tests for the Round 10 review fixes in CONTRACT.md.

From the banner onward the state under the leader number is
the starting number's state, whether the starting number
held recorded state or none, and the same identity follows
a pair that spends through the pid changed link alone.
"""

from blindgate.tracing import parse_trace


FLAGGED = "O_RDONLY|O_DIRECTORY|O_CLOEXEC"
UNFLAGGED = "O_RDONLY|O_DIRECTORY"


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text("guide\n")
    (project / "src").mkdir()
    (project / "src" / "app.py").write_text("x = 1\n")
    (project / "guide.md").write_text("root guide\n")
    return project


def dir_open_line(path, fd, flags, pid):
    return f'{pid} openat(AT_FDCWD, "{path}", {flags}) = {fd}'


def half(pid):
    return (
        f"{pid} "
        'execve("/bin/true", ["/bin/true"], '
        "0x2db19d50 /* 55 vars */ <unfinished ...>"
    )


def resumed(pid):
    return f"{pid} <... execve resumed>)             = 0"


def test_the_banner_carries_the_starting_threads_directory(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, 100),
            "100 clone(child_stack=NULL, "
            "flags=CLONE_VM|SIGCHLD) = 101",
            '101 chdir("src") = 0',
            half(101),
            "100 +++ superseded by execve in pid 101 +++",
            resumed(100),
            '100 openat(AT_FDCWD, "app.py", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_a_banner_over_an_unrecorded_starter_empties_the_leader(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, 100),
            half(102),
            "100 +++ superseded by execve in pid 102 +++",
            resumed(100),
            '100 openat(3, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_tail_only_pair_adopts_the_starting_threads_state(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED, 100),
            "100 clone(child_stack=NULL, "
            "flags=CLONE_VM|SIGCHLD) = 101",
            dir_open_line("src", 4, UNFLAGGED, 101),
            "101 execve(\"/bin/true\", [\"/bin/true\"], "
            "0x2db19d50 /* 55 vars */ <pid changed to 100 ...>",
            resumed(100),
            '100 openat(4, "app.py", O_RDONLY) = 5',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []
