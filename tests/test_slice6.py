"""Tests for the slice 6 obligations in CONTRACT.md.

Slice 6 reassembles the calls strace splits into an unfinished
line and a resumed line. A kept unfinished line and its resumed
partner are judged as the whole line the two halves read as, for
the state changing calls and for reads alike.

Every fixture here is synthetic and written for this test file.
The traces are hand written strace lines in the documented split
format, apart from one test whose two lines are kept verbatim
from a real strace capture made on this machine, of a threaded
python process whose open of a fifo blocked until a writer
arrived. That capture is the shape proof for the pairing, and
the file it names is a plain file in the synthetic project.
"""

from blindgate.tracing import parse_trace


def open_line(path, pid=None, result=3):
    line = f'openat(AT_FDCWD, "{path}", O_RDONLY) = {result}'
    return f"{pid} {line}" if pid is not None else line


def dir_open_line(path, fd, pid=None):
    line = f'openat(AT_FDCWD, "{path}", O_RDONLY|O_DIRECTORY) = {fd}'
    return f"{pid} {line}" if pid is not None else line


def fd_open_line(fd, path, pid=None, result=4):
    line = f'openat({fd}, "{path}", O_RDONLY) = {result}'
    return f"{pid} {line}" if pid is not None else line


def unfinished(call_text, pid=None):
    line = f"{call_text} <unfinished ...>"
    return f"{pid} {line}" if pid is not None else line


def resumed(name, tail, pid=None):
    line = f"<... {name} resumed> {tail}"
    return f"{pid} {line}" if pid is not None else line


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text(
        "the guide\n", encoding="utf-8"
    )
    (project / "src").mkdir()
    (project / "src" / "app.py").write_text("x = 1\n", encoding="utf-8")
    (project / "fifo1").write_text("not a fifo\n", encoding="utf-8")
    return project


def test_split_openat_read_is_counted(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('openat(AT_FDCWD, "src/app.py", O_RDONLY', pid=100),
            resumed("openat", ") = 3", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_real_capture_split_openat_pair_is_counted(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            '164766 openat(AT_FDCWD, "fifo1", O_RDONLY|O_CLOEXEC <unfinished ...>',
            "164766 <... openat resumed>)            = 3",
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["fifo1"]
    assert outside == []


def test_split_openat_failure_counts_no_read(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('openat(AT_FDCWD, "src/app.py", O_RDONLY', pid=100),
            resumed(
                "openat",
                ") = -1 EINTR (Interrupted system call)",
                pid=100,
            ),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == []
    assert outside == []


def test_split_hex_failure_on_the_resumed_half_records_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('openat(AT_FDCWD, "src/app.py", O_RDONLY', pid=100),
            resumed("openat", ") = -0x1", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == []
    assert outside == []


def test_split_chdir_moves_the_directory_in_force(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('chdir("docs"', pid=100),
            resumed("chdir", ") = 0", pid=100),
            open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_chdir_failure_moves_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('chdir("docs"', pid=100),
            resumed(
                "chdir",
                ") = -1 ENOENT (No such file or directory)",
                pid=100,
            ),
            open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_split_directory_open_records_its_descriptor(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished(
                'openat(AT_FDCWD, "docs", O_RDONLY|O_DIRECTORY', pid=100
            ),
            resumed("openat", ") = 3", pid=100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_plain_open_records_its_descriptor(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('open("docs", O_RDONLY', pid=100),
            resumed("open", ") = 3", pid=100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_fchdir_moves_the_directory_in_force(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            unfinished("fchdir(3", pid=100),
            resumed("fchdir", ") = 0", pid=100),
            open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_close_forgets_the_descriptor(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            unfinished("close(3", pid=100),
            resumed("close", ") = 0", pid=100),
            fd_open_line(3, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_split_dup_carries_the_descriptor_to_the_returned_number(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            unfinished("dup(3", pid=100),
            resumed("dup", ") = 7", pid=100),
            fd_open_line(7, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_split_dup2_carries_the_descriptor(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            dir_open_line("docs", 3, pid=100),
            unfinished("dup2(3, 9", pid=100),
            resumed("dup2", ") = 9", pid=100),
            fd_open_line(9, "guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fork_between_the_halves_seeds_the_state_at_the_fork_line(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('chdir("docs"', pid=100),
            "100 clone(child_stack=NULL, flags=CLONE_CHILD_CLEARTID) = 101",
            resumed("chdir", ") = 0", pid=100),
            open_line("guide.md", pid=101),
            open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_unfinished_line_with_no_resumed_partner_records_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('openat(AT_FDCWD, "src/app.py", O_RDONLY', pid=100),
            unfinished('chdir("docs"', pid=100),
            open_line("docs/guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_resumed_line_with_no_pending_unfinished_records_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            resumed("openat", ") = 3", pid=100),
            resumed("chdir", ") = 0", pid=100),
            open_line("src/app.py", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["src/app.py"]
    assert outside == []


def test_second_unfinished_replaces_the_pending_line(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('openat(AT_FDCWD, "src/app.py", O_RDONLY', pid=100),
            unfinished(
                'openat(AT_FDCWD, "docs/guide.md", O_RDONLY', pid=100
            ),
            resumed("openat", ") = 3", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_resumed_line_pairs_only_with_its_own_process(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('chdir("docs"', pid=100),
            resumed("chdir", ") = 0", pid=200),
            open_line("guide.md", pid=100),
            resumed("chdir", ") = 0", pid=100),
            open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_interleaved_processes_complete_their_own_pairs(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('openat(AT_FDCWD, "src/app.py", O_RDONLY', pid=100),
            unfinished(
                'openat(AT_FDCWD, "docs/guide.md", O_RDONLY', pid=200
            ),
            resumed("openat", ") = 3", pid=200),
            resumed("openat", ") = 4", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_truncated_unfinished_line_records_nothing_even_when_resumed(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            unfinished('openat(AT_FDCWD, "docs/gui', pid=100),
            resumed("openat", ") = 3", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == []
    assert outside == []
    log = "\n".join(
        [
            unfinished('chdir("do', pid=100),
            resumed("chdir", ") = 0", pid=100),
            open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []
