"""Tests for the Round 7 review fixes in CONTRACT.md.

FIX7-1 reads the flags field at the struct's own level of a
clone3 line, so a flags list inside a nested field names no
sharing, and the struct span is read to its matching brace.
The brace closed split clone3 half of S8-3 is pinned here.
"""

from blindgate.tracing import parse_trace


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text("guide\n")
    (project / "guide.md").write_text("root guide\n")
    return project


def _docs_open(pid=100):
    return (
        f'{pid} openat(AT_FDCWD, "docs", '
        "O_RDONLY|O_DIRECTORY) = 3"
    )


def test_a_nested_field_flags_list_names_no_sharing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            _docs_open(),
            "100 clone3({nested={flags=CLONE_FILES|CLONE_FS}, "
            "flags=0, exit_signal=SIGCHLD} => "
            "{parent_tid=[101]}, 88) = 101",
            "101 close(3) = 0",
            '100 openat(3, "guide.md", O_RDONLY) = 5',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_the_flags_field_after_a_nested_field_is_honoured(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            _docs_open(),
            "100 clone3({nested={size=88}, "
            "flags=CLONE_FILES|SIGCHLD, exit_signal=SIGCHLD} => "
            "{parent_tid=[101]}, 88) = 101",
            "101 close(3) = 0",
            '100 openat(3, "guide.md", O_RDONLY) = 5',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_brace_closed_split_clone3_half_shares(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            _docs_open(),
            "100 clone3({flags=CLONE_FILES|SIGCHLD} "
            "<unfinished ...>",
            "100 <... clone3 resumed>) = 101",
            "101 close(3) = 0",
            '100 openat(3, "guide.md", O_RDONLY) = 5',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_nested_field_in_a_split_half_is_not_kept(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            _docs_open(),
            "100 clone3({nested={flags=CLONE_FILES}, flags=0, "
            "exit_signal=SIGCHLD <unfinished ...>",
            "100 <... clone3 resumed>) = 101",
            "101 close(3) = 0",
            '100 openat(3, "guide.md", O_RDONLY) = 5',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []
