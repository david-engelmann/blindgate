"""Tests for the Round 2 review fix obligations in CONTRACT.md.

Every fixture here is synthetic and written for this test file.
"""

import json
import os
import shutil
from pathlib import Path

import pytest

from blindgate.analysis import analyse_judged
from blindgate.cli import main
from blindgate.errors import BlindgateError
from blindgate.tracing import (
    looks_like_strace,
    parse_trace,
    run_traced,
)

from test_slice3 import (
    WORKFLOW,
    read_line,
    two_check_config,
    write_config,
    write_log,
    write_workflow,
)

needs_symlink = pytest.mark.skipif(
    not hasattr(os, "symlink"), reason="platform cannot create symlinks"
)


def test_root_itself_is_never_a_read(tmp_path):
    root = tmp_path.resolve()
    lines = "\n".join(
        [
            read_line("."),
            read_line("./"),
            read_line(str(root)),
        ]
    )
    reads, outside = parse_trace(lines, root)
    assert reads == []
    assert outside == []


def test_root_itself_cannot_manufacture_a_proven_verdict(
    tmp_path, capsys
):
    config = write_config(
        tmp_path,
        {
            "checks": [
                {
                    "name": "unit tests",
                    "job": "unit-tests",
                    "surface": ["**"],
                }
            ]
        },
    )
    workflow = write_workflow(tmp_path)
    log = write_log(tmp_path, "trace.log", read_line(".") + "\n")
    code = main(
        [
            "--root",
            str(tmp_path),
            "--workflow",
            str(workflow),
            "--config",
            str(config),
            "--trace",
            str(log),
            "--json",
        ]
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out)
    (result,) = payload["checks"]
    assert result["verdict"] == "empty read set"
    assert result["read_set"] == []


@needs_symlink
def test_symlink_chain_escape_is_named(tmp_path):
    outside_dir = tmp_path / "outside"
    outside_dir.mkdir()
    (outside_dir / "secret.txt").write_text("outside", encoding="utf-8")
    root = tmp_path / "project"
    (root / "docs").mkdir(parents=True)
    os.symlink(outside_dir / "secret.txt", root / "docs" / "hop2")
    os.symlink(root / "docs" / "hop2", root / "docs" / "hop1")
    reads, outside = parse_trace(read_line("docs/hop1"), root.resolve())
    assert reads == []
    assert outside == [str((outside_dir / "secret.txt").resolve())]


@needs_symlink
def test_dangling_symlink_to_outside_is_named(tmp_path):
    root = tmp_path / "project"
    (root / "docs").mkdir(parents=True)
    missing = tmp_path / "outside" / "missing.txt"
    os.symlink(missing, root / "docs" / "dangling")
    reads, outside = parse_trace(read_line("docs/dangling"), root.resolve())
    assert reads == []
    assert outside == [str(missing)]


@needs_symlink
def test_escape_spelled_through_a_root_alias_is_named(tmp_path):
    outside_dir = tmp_path / "outside"
    outside_dir.mkdir()
    secret = outside_dir / "secret.txt"
    secret.write_text("outside", encoding="utf-8")
    root = tmp_path / "project"
    root.mkdir()
    os.symlink(secret, root / "escape.txt")
    alias = tmp_path / "rootlink"
    os.symlink(root, alias)
    spelled = str(alias / "escape.txt")
    reads, outside = parse_trace(read_line(spelled), root.resolve())
    assert reads == []
    assert outside == [str(secret.resolve())]


def test_source_shaped_garbage_is_refused():
    assert not looks_like_strace("result = compute(value=42)\n")
    assert not looks_like_strace(
        "the function read(file) = 3 times today\n"
    )
    assert looks_like_strace(
        'openat(AT_FDCWD, "src/app.py", O_RDONLY) = 3\n'
    )
    assert looks_like_strace(
        '99 openat(AT_FDCWD, "docs/x.md", O_RDONLY) = -1 ENOENT '
        "(No such file or directory)\n"
    )


@needs_symlink
def test_run_capture_names_its_outside_reads(tmp_path):
    if shutil.which("strace") is None:
        pytest.skip("strace is not available")
    outside_file = tmp_path / "outside.txt"
    outside_file.write_text("outside", encoding="utf-8")
    root = tmp_path / "project"
    root.mkdir()
    os.symlink(outside_file, root / "escape.txt")
    reads, outside = run_traced(
        ["cat", str(root / "escape.txt")], root
    )
    assert reads == []
    assert outside == [str(outside_file.resolve())]


def test_judgements_must_be_uniform_triples():
    from blindgate.config import Check

    check = Check(name="unit tests", job="unit-tests", surface=("src/**",))
    with pytest.raises(ValueError, match="triple"):
        analyse_judged([check], [(["src/app.py"], None)])
