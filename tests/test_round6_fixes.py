"""Tests for the Round 6 review fixes in CONTRACT.md.

FIX6-1 anchors the resumed fork reading to the call position of
its line. FIX6-2 spends a kept clone half only on the resumed
clone line it belongs to.
"""

from blindgate.tracing import parse_trace


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text("guide\n")
    return project


def _sharing_kept_half(pid=100):
    return (
        f"{pid} clone(child_stack=NULL, "
        "flags=CLONE_FILES|CLONE_CHILD_CLEARTID|"
        "CLONE_CHILD_SETTID|SIGCHLD, "
        "child_tidptr=0x7374cca3a350 <unfinished ...>"
    )


def test_marker_text_inside_a_path_is_not_a_resumed_fork(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            '100 openat(AT_FDCWD, "docs", O_RDONLY|O_DIRECTORY) = 3',
            '100 openat(3, "x<... clone resumed>y", O_RDONLY) = 4',
            '100 openat(3, "guide.md", O_RDONLY) = 5',
            '200 openat(3, "guide.md", O_RDONLY) = 6',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == [
        "docs/guide.md",
        "docs/x<... clone resumed>y",
        "guide.md",
    ]
    assert outside == []


def test_a_resumed_clone3_does_not_spend_a_kept_clone_half(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            '100 openat(AT_FDCWD, "docs", O_RDONLY|O_DIRECTORY) = 3',
            _sharing_kept_half(),
            "100 <... clone3 resumed>) = 101",
            "101 close(3) = 0",
            '100 openat(3, "guide.md", O_RDONLY) = 5',
            "100 <... clone resumed>) = 102",
            "102 close(3) = 0",
            '100 openat(3, "guide.md", O_RDONLY) = 6',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_a_resumed_fork_does_not_spend_a_kept_clone_half(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            '100 openat(AT_FDCWD, "docs", O_RDONLY|O_DIRECTORY) = 3',
            _sharing_kept_half(),
            "100 <... fork resumed>) = 101",
            "101 close(3) = 0",
            '100 openat(3, "guide.md", O_RDONLY) = 5',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []
