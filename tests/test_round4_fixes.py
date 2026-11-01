"""Tests for the Round 4 review fix obligations in CONTRACT.md.

Every fixture here is synthetic and written for this test file.
The traces are hand written strace lines, never captured from a
real session. The red team probes are ported here as regression
tests.
"""

import os

import pytest

from blindgate.tracing import parse_trace

needs_symlink = pytest.mark.skipif(
    not hasattr(os, "symlink"), reason="platform cannot create symlinks"
)


def _project(tmp_path):
    root = tmp_path / "project"
    docs = root / "docs"
    docs.mkdir(parents=True)
    (docs / "guide.md").write_text("guide", encoding="utf-8")
    return root.resolve()


def test_a_hex_spelled_failure_records_no_descriptor_and_no_read(
    tmp_path,
):
    root = _project(tmp_path)
    lines = "\n".join(
        [
            'openat(AT_FDCWD, "docs", O_RDONLY) = -0x1',
            'openat(0, "guide.md", O_RDONLY) = 4',
            'openat(AT_FDCWD, "docs/guide.md", O_RDONLY) = -0x1',
        ]
    )
    reads, outside = parse_trace(lines, root)
    # The failed opens count no read and record no descriptor, so
    # the read through the unrecorded number 0 falls back to the
    # directory in force and lands at the root, never inside docs.
    assert reads == ["guide.md"]
    assert outside == []


def test_a_hex_spelled_success_records_the_number_it_names(tmp_path):
    root = _project(tmp_path)
    lines = "\n".join(
        [
            'openat(AT_FDCWD, "docs", O_RDONLY|O_DIRECTORY) = 0x3',
            'openat(3, "guide.md", O_RDONLY) = 0x4',
        ]
    )
    reads, outside = parse_trace(lines, root)
    assert reads == ["docs/guide.md"]


@needs_symlink
def test_an_absolute_path_through_a_descriptor_is_not_named(tmp_path):
    outside_dir = tmp_path / "outside"
    outside_dir.mkdir()
    secret = outside_dir / "secret.txt"
    secret.write_text("secret", encoding="utf-8")
    root = _project(tmp_path)
    os.symlink(outside_dir, root / "link")
    lines = "\n".join(
        [
            'openat(AT_FDCWD, "link", O_RDONLY|O_DIRECTORY) = 3',
            f'openat(3, "{secret}", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(lines, root)
    assert reads == []
    assert outside == []


@needs_symlink
def test_a_relative_read_through_an_inside_handle_is_still_named(
    tmp_path,
):
    outside_dir = tmp_path / "outside"
    outside_dir.mkdir()
    (outside_dir / "secret.txt").write_text("secret", encoding="utf-8")
    root = _project(tmp_path)
    os.symlink(outside_dir, root / "link")
    lines = "\n".join(
        [
            'openat(AT_FDCWD, "link", O_RDONLY|O_DIRECTORY) = 3',
            'openat(3, "secret.txt", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(lines, root)
    assert reads == []
    assert outside == [str(outside_dir.resolve() / "secret.txt")]
