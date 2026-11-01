"""Tests for the Round 5 review fix obligations in CONTRACT.md.

Every fixture here is synthetic and written for this test file.
The red team probes are ported here as regression tests.
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


def test_split_dup3_with_its_flags_argument_copies_the_descriptor(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            'openat(AT_FDCWD, "docs", O_RDONLY|O_DIRECTORY) = 3',
            "dup3(3, 9, O_CLOEXEC <unfinished ...>",
            "<... dup3 resumed>) = 9",
            'openat(9, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_whole_line_dup3_with_its_flags_argument_copies_the_descriptor(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            'openat(AT_FDCWD, "docs", O_RDONLY|O_DIRECTORY) = 3',
            "dup3(3, 9, O_CLOEXEC) = 9",
            'openat(9, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_a_failed_completion_spends_the_kept_line(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            'openat(AT_FDCWD, "guide.md", O_RDONLY <unfinished ...>',
            "<... openat resumed>) = -1 EINTR (Interrupted system call)",
            "<... openat resumed>) = 3",
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == []
    assert outside == []


def test_marker_text_inside_a_path_spends_nothing(tmp_path):
    project = make_project(tmp_path)
    odd = project / "x<... chdir resumed>y"
    odd.write_text("odd\n", encoding="utf-8")
    log = "\n".join(
        [
            '111 chdir("docs" <unfinished ...>',
            '222 openat(AT_FDCWD, "x<... chdir resumed>y", O_RDONLY) = 3',
            '111 <... chdir resumed>) = 0',
            '111 openat(AT_FDCWD, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "x<... chdir resumed>y"]
    assert outside == []


def test_an_exit_line_clears_the_pending_line_of_its_process(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            '111 openat(AT_FDCWD, "src/app.py", O_RDONLY <unfinished ...>',
            "111 +++ exited with 0 +++",
            '111 <... openat resumed>) = 3',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == []
    assert outside == []


def test_a_fork_clears_pending_lines_held_under_the_child_number(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            '222 openat(AT_FDCWD, "src/app.py", O_RDONLY <unfinished ...>',
            "111 clone() = 222",
            '222 <... openat resumed>) = 3',
            '111 openat(AT_FDCWD, "guide.md", O_RDONLY) = 5',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []
