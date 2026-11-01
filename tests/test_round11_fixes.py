"""Tests for the Round 11 review fixes stated in CONTRACT.md.

The one pin here makes the print order limit executable.
The parse spends entry and exit prints in the order strace
prints them, so where a racing thread's close exit print
lands after another thread's split open exit print in a
shared descriptor table, the close deletes the entry the
open just recorded, and a later dup of that number records
nothing. The kernel's own effect order on the red team's
capture differed, and the contract states the limit rather
than promising the parse reorders prints it never saw in
effect order.

Every fixture here is synthetic and written for this test
file, distilled from the red team's saved failing capture
with fixture process ids in place of the captured ones.
"""

from blindgate.tracing import parse_trace


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text(
        "the guide\n", encoding="utf-8"
    )
    (project / "guide.md").write_text(
        "root guide\n", encoding="utf-8"
    )
    return project


def test_a_racing_close_exit_spends_in_print_order(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            "100 clone(child_stack=0x7623250e2000, "
            "flags=CLONE_FILES|SIGCHLD) = 101",
            '100 openat(AT_FDCWD, "docs", '
            "O_RDONLY|O_DIRECTORY <unfinished ...>",
            "101 close(3 <unfinished ...>",
            "100 <... openat resumed>) = 3",
            "101 <... close resumed>) = 0",
            "100 dup2(3, 20) = 20",
            '100 openat(20, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["guide.md"]
    assert outside == []
