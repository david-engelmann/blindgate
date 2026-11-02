"""Round 13 review fix pins for the slice 14 obligations.

The two axis review returned one Minor on its spec axis, that
the drop of a standing entry at a number an unrecorded source
duplicates onto was behaviour S14-3 did not state, and the red
team returned the same shape as a Note. S14-3 now states the
drop, and the pins below hold it for both duplicating
commands. Every fixture here is synthetic.
"""

from blindgate.tracing import parse_trace


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text(
        "the guide\n", encoding="utf-8"
    )
    (project / "guide.md").write_text("root guide\n", encoding="utf-8")
    return project


def test_a_dup_from_an_unrecorded_source_drops_the_entry_at_its_return(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            'openat(AT_FDCWD, "docs", O_RDONLY|O_DIRECTORY) = 7',
            "fcntl(9, F_DUPFD, 0) = 7",
            "fchdir(7) = 0",
            'openat(AT_FDCWD, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_a_dup_cloexec_from_an_unrecorded_source_drops_the_entry_at_its_return(
    tmp_path,
):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            'openat(AT_FDCWD, "docs", O_RDONLY|O_DIRECTORY) = 7',
            "fcntl(9, F_DUPFD_CLOEXEC, 0) = 7",
            "fchdir(7) = 0",
            'openat(AT_FDCWD, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []
