"""Tests for the Round 12 review fixes on the slice 13 obligations.

The two axis review and the red team found the slice sound in
behaviour and short on pins at three edges, and two standing
pins that proved less than their names claimed. The pins below
close those edges. A value carrying an extra bit beside the
flag name, a reflag landing on an entry adopted through an
exec banner, and a split fcntl pair whose entry half precedes
the line that creates its entry are pinned as they behave,
and the F_GETFD refusal and the no entry rule are pinned
where a wrong build would show.

Every fixture here is synthetic and written for this test
file, in the shapes the slice 13 fixtures established.
"""

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


def execveat_half(pid):
    return (
        f"{pid} "
        'execveat(3, "true", ["true"], '
        "0x2db19d50 /* 55 vars */, 0 <unfinished ...>"
    )


def execveat_resumed(pid, tail=")             = 0"):
    return f"{pid} <... execveat resumed>{tail}"


FLAGGED = "O_RDONLY|O_DIRECTORY|O_CLOEXEC"
UNFLAGGED = "O_RDONLY|O_DIRECTORY"


def test_an_extra_bit_beside_the_flag_name_still_sets_the_flag(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_SETFD, FD_CLOEXEC|0x2) = 0",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_value_of_only_an_extra_bit_clears_the_flag(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, FLAGGED),
            "fcntl(3, F_SETFD, 0x2) = 0",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_a_reflag_lands_on_an_entry_adopted_through_the_banner(
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
            "100 fcntl(3, F_SETFD, FD_CLOEXEC) = 0",
            execve_line(pid=100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_split_fcntl_is_judged_after_a_later_creation_line(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            "100 fcntl(3, F_SETFD, FD_CLOEXEC <unfinished ...>",
            dir_open_line("docs", 3, UNFLAGGED, pid=100),
            "100 <... fcntl resumed>) = 0",
            execve_line(pid=100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_fcntl_getfd_return_claiming_the_flag_sets_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            "fcntl(3, F_GETFD) = 0x1 (flags FD_CLOEXEC)",
            execve_line(),
            fd_open_line(3, "guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fcntl_on_an_unrecorded_number_creates_no_entry_off_root(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, UNFLAGGED),
            dir_open_line("src", 4, UNFLAGGED),
            "fchdir(3) = 0",
            "fcntl(9, F_SETFD, FD_CLOEXEC) = 0",
            "fchdir(4) = 0",
            "fchdir(9) = 0",
            cwd_open_line("app.py"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []
