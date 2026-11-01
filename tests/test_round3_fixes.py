"""Tests for the Round 3 review fix obligations in CONTRACT.md.

Every fixture here is synthetic and written for this test file.
The traces are hand written strace lines, never captured from a
real session, apart from the one live capture test that runs a
synthetic command under strace on this machine.
"""

import os
import shutil
from pathlib import Path

import pytest

from blindgate.tracing import parse_trace, run_traced

needs_symlink = pytest.mark.skipif(
    not hasattr(os, "symlink"), reason="platform cannot create symlinks"
)


def open_line(path, pid=None, fd=3):
    prefix = f"{pid} " if pid is not None else ""
    return f'{prefix}openat(AT_FDCWD, "{path}", O_RDONLY) = {fd}'


def chdir_line(path, pid=None, result=0):
    prefix = f"{pid} " if pid is not None else ""
    return f'{prefix}chdir("{path}") = {result}'


def test_a_forked_child_inherits_its_parents_directory(tmp_path):
    root = tmp_path.resolve()
    lines = "\n".join(
        [
            chdir_line("docs", pid=100),
            "100 clone(child_stack=NULL, flags=CLONE_CHILD_CLEARTID) = 101",
            open_line("guide.md", pid=101),
            open_line("guide.md", pid=100),
        ]
    )
    reads, outside = parse_trace(lines, root)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_a_shell_and_child_capture_reads_at_the_moved_place(tmp_path):
    if shutil.which("strace") is None or shutil.which("sh") is None:
        pytest.skip("strace or sh is not available")
    root = tmp_path / "project"
    docs = root / "docs"
    docs.mkdir(parents=True)
    (docs / "guide.md").write_text("guide", encoding="utf-8")
    reads, outside = run_traced(
        ["sh", "-c", f"cd {root} && cd docs && cat guide.md"],
        root.resolve(),
    )
    assert "docs/guide.md" in reads
    assert "guide.md" not in reads


@needs_symlink
def test_a_chdir_through_a_symlink_and_back_out_lands_outside(tmp_path):
    outside_dir = tmp_path / "outside"
    outside_dir.mkdir()
    (outside_dir / "secret.txt").write_text("secret", encoding="utf-8")
    root = tmp_path / "project"
    root.mkdir()
    os.symlink(outside_dir, root / "linkdeep")
    lines = "\n".join(
        [
            chdir_line("linkdeep/.."),
            open_line("secret.txt"),
        ]
    )
    reads, outside = parse_trace(lines, root.resolve())
    assert reads == []
    lines = "\n".join(
        [
            chdir_line("linkdeep"),
            chdir_line(".."),
            open_line("secret.txt"),
        ]
    )
    reads, outside = parse_trace(lines, root.resolve())
    assert reads == []


def test_a_hex_spelled_failure_return_moves_nothing(tmp_path):
    root = tmp_path.resolve()
    lines = "\n".join(
        [
            '99 chdir("docs") = -0x1',
            open_line("guide.md", pid=99),
        ]
    )
    reads, outside = parse_trace(lines, root)
    assert reads == ["guide.md"]


def test_pid_spellings_share_one_bucket(tmp_path):
    root = tmp_path.resolve()
    lines = "\n".join(
        [
            chdir_line("docs", pid="007"),
            open_line("guide.md", pid=7),
        ]
    )
    reads, outside = parse_trace(lines, root)
    assert reads == ["docs/guide.md"]


def test_chdir_text_inside_an_open_path_moves_nothing(tmp_path):
    root = tmp_path.resolve()
    weird = 'weird chdir(\\"docs\\") name.txt'
    lines = "\n".join(
        [
            open_line(weird),
            open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(lines, root)
    assert "guide.md" in reads
    assert not any(name.startswith("docs/") for name in reads)


def test_an_open_of_the_directory_in_force_enters_neither_set(
    tmp_path,
):
    root = tmp_path.resolve()
    lines = "\n".join(
        [
            chdir_line("docs"),
            open_line("."),
            open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(lines, root)
    assert reads == ["docs/guide.md"]
    assert outside == []
