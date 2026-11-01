"""Tests for the Round 9 review fixes in CONTRACT.md.

A whole execve line whose quoted argument text holds the
unfinished marker as data is judged as the whole line it is,
because the marker only has meaning at the end of a line.
"""

from blindgate.tracing import parse_trace


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text("guide\n")
    (project / "guide.md").write_text("root guide\n")
    return project


def test_an_execve_argument_holding_the_marker_text_still_acts(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            '100 openat(AT_FDCWD, "docs", '
            "O_RDONLY|O_DIRECTORY|O_CLOEXEC) = 3",
            '100 execve("/usr/bin/reader", '
            '["reader", "<unfinished ...>"], '
            "0x7ffd48416d98 /* 54 vars */) = 0",
            '100 openat(3, "guide.md", O_RDONLY) = 5',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []
