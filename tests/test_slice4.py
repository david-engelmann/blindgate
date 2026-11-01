"""Tests for the slice 4 obligations in CONTRACT.md.

Slice 4 tracks directory changes through the trace. A chdir record
moves the current directory of its process, and a relative open
resolves against the directory in force for its process at that
line instead of always resolving against the project root.

Every fixture here is synthetic and written for this test file.
The traces are hand written strace lines, never captured from a
real session, apart from the one live capture test which runs a
shell under strace and is guarded on strace availability.
"""

import json
import shutil
import sys
from pathlib import Path

import pytest

from blindgate.cli import main
from blindgate.tracing import parse_trace, run_traced

WORKFLOW = """name: synthetic gate runs
on: push
jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - run: python -m pytest
  docs-check:
    runs-on: ubuntu-latest
    steps:
      - run: python tools/check_docs.py
"""


def open_line(path, pid=None):
    line = f'openat(AT_FDCWD, "{path}", O_RDONLY) = 3'
    return f"{pid} {line}" if pid is not None else line


def chdir_line(path, pid=None, result=0):
    line = f'chdir("{path}") = {result}'
    return f"{pid} {line}" if pid is not None else line


def write_workflow(folder):
    workflow = Path(folder) / "workflow.yml"
    workflow.write_text(WORKFLOW, encoding="utf-8")
    return workflow


def write_config(folder, payload, name="config.json"):
    config = Path(folder) / name
    config.write_text(json.dumps(payload), encoding="utf-8")
    return config


def write_log(folder, name, text):
    log = Path(folder) / name
    log.write_text(text, encoding="utf-8")
    return log


def docs_config():
    return {
        "checks": [
            {
                "name": "docs check",
                "job": "docs-check",
                "surface": ["docs/**"],
            }
        ]
    }


def payload_from(capsys):
    return json.loads(capsys.readouterr().out)


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text(
        "the guide\n", encoding="utf-8"
    )
    (project / "src").mkdir()
    (project / "src" / "app.py").write_text("x = 1\n", encoding="utf-8")
    return project


def test_open_after_chdir_into_a_subdirectory_lands_there(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            chdir_line("docs"),
            open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_open_before_any_chdir_still_resolves_against_the_root(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            open_line("docs/guide.md"),
            chdir_line("src"),
            open_line("app.py"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_relative_chdir_chain_walks_step_by_step(tmp_path):
    project = make_project(tmp_path)
    (project / "docs" / "deep").mkdir()
    (project / "docs" / "deep" / "note.md").write_text(
        "deep\n", encoding="utf-8"
    )
    log = "\n".join(
        [
            chdir_line("docs"),
            chdir_line("deep"),
            open_line("note.md"),
            chdir_line(".."),
            open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/deep/note.md", "docs/guide.md"]
    assert outside == []


def test_failed_chdir_moves_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            'chdir("docs") = -1 ENOENT (No such file or directory)',
            open_line("docs/guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_unfinished_chdir_moves_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            'chdir("docs" <unfinished ...>',
            open_line("docs/guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_chdir_itself_is_not_a_read(tmp_path):
    project = make_project(tmp_path)
    log = chdir_line("docs")
    reads, outside = parse_trace(log, project)
    assert reads == []
    assert outside == []


def test_two_processes_keep_separate_directories(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            chdir_line("docs", pid=101),
            chdir_line("src", pid=202),
            open_line("guide.md", pid=101),
            open_line("app.py", pid=202),
            open_line("docs/guide.md", pid=303),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_unprefixed_lines_share_one_bucket(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            chdir_line("docs", pid=101),
            open_line("docs/guide.md"),
            chdir_line("src"),
            open_line("app.py"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_chdir_outside_the_root_cannot_prove_an_inside_surface(
    tmp_path, capsys
):
    project = make_project(tmp_path)
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    (elsewhere / "guide.md").write_text("other\n", encoding="utf-8")
    log = "\n".join(
        [
            chdir_line(str(elsewhere)),
            open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == []
    workflow = write_workflow(tmp_path)
    trace = write_log(tmp_path, "trace.log", log + "\n")
    config = write_config(tmp_path, docs_config())
    code = main(
        [
            "--root", str(project), "--json",
            "--workflow", str(workflow),
            "--config", str(config),
            "--trace", str(trace),
        ]
    )
    payload = payload_from(capsys)
    assert code == 0
    (docs,) = payload["checks"]
    assert docs["verdict"] == "empty read set"
    assert docs["read_set"] == []


def test_chdir_back_inside_the_root_restores_inside_reads(tmp_path):
    project = make_project(tmp_path)
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    log = "\n".join(
        [
            chdir_line(str(elsewhere)),
            open_line("guide.md"),
            chdir_line(str(project / "docs")),
            open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_fchdir_on_an_unrecorded_descriptor_moves_nothing(tmp_path):
    project = make_project(tmp_path)
    log = "\n".join(
        [
            chdir_line("docs"),
            "fchdir(3) = 0",
            open_line("guide.md"),
        ]
    )
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_run_capture_tracks_a_chdir(tmp_path):
    if shutil.which("strace") is None:
        pytest.skip("strace is not available")
    project = make_project(tmp_path)
    program = (
        f"import os; os.chdir({str(project / 'docs')!r}); "
        "open('guide.md').read()"
    )
    reads, outside = run_traced(
        [sys.executable, "-c", program], project
    )
    assert "docs/guide.md" in reads
