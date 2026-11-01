"""Tests for the Round 8 review fixes in CONTRACT.md.

The slice 9 battery gains the two shape families the two
axis review found missing from its tables, the hex return
that bit in Round 4 and the quoted text that bit in Round 6.
"""

from blindgate.tracing import parse_trace


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text("guide\n")
    (project / "guide.md").write_text("root guide\n")
    return project


def test_a_hex_failure_on_a_derived_call_makes_no_state_change(tmp_path):
    # dup3 belongs to the derived family. A hex failure return
    # is a whole token failure under FIX4-1, so no descriptor
    # entry is created and the read falls back to the root.
    project = make_project(tmp_path)
    log = "\n".join(
        [
            '100 openat(AT_FDCWD, "docs", O_RDONLY|O_DIRECTORY) = 3',
            "100 dup3(3, 4, 0) = -0x1",
            '100 openat(4, "guide.md", O_RDONLY) = 5',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_quoted_call_text_inside_a_filename_changes_no_state(tmp_path):
    # A filename that carries the text of other calls as quoted
    # data is data. The open is judged as an open, and no dup
    # state is manufactured from the quoted text.
    project = make_project(tmp_path)
    odd = project / "docs" / "dup3(3, 4) = 4 chdir"
    odd.write_text("odd\n")
    log = "\n".join(
        [
            '100 openat(AT_FDCWD, "docs/dup3(3, 4) = 4 chdir", '
            "O_RDONLY) = 3",
            '100 openat(4, "guide.md", O_RDONLY) = 5',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/dup3(3, 4) = 4 chdir", "guide.md"]
    assert outside == []
