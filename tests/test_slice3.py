"""Tests for the slice 3 obligations in CONTRACT.md.

Slice 3 retires the two trace limits Round 1 accepted in writing.
Traced paths are resolved canonically, so a symlink that points
outside the project can never support a proven verdict and the
outside read is named in the report. A trace file that is not
strace output at all is refused with a named error instead of
reading as a genuine trace with no reads.

Every fixture here is synthetic and written for this test file.
The traces are hand written strace lines, never captured from a
real session. Symlink fixtures are built by the tests themselves
in temp dirs and skipped when the platform cannot create them.
"""

import json
import os
import shutil
from pathlib import Path

import pytest

from blindgate.analysis import analyse_with_outside
from blindgate.cli import main
from blindgate.config import load_config
from blindgate.tracing import parse_strace_log, parse_trace

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

GARBAGE = (
    "Weekly notes from the docs meeting.\n"
    "The guide needs a new owner and the screenshots are stale.\n"
    "Nothing in this file came from a tracing tool.\n"
)


def read_line(path):
    return f'99 openat(AT_FDCWD, "{path}", O_RDONLY) = 3'


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


def two_check_config(unit_extra=None, docs_extra=None):
    unit = {"name": "unit tests", "job": "unit-tests", "surface": ["src/**", "tests/**"]}
    docs = {"name": "docs check", "job": "docs-check", "surface": ["docs/**"]}
    if unit_extra:
        unit.update(unit_extra)
    if docs_extra:
        docs.update(docs_extra)
    return {"checks": [unit, docs]}


def payload_from(capsys):
    return json.loads(capsys.readouterr().out)


def check_for(payload, job):
    return next(check for check in payload["checks"] if check["job"] == job)


def make_symlink(target, link):
    try:
        os.symlink(target, link)
    except (OSError, NotImplementedError) as exc:
        pytest.skip(f"cannot create symlinks on this platform, {exc}")


def build_symlink_project(tmp_path):
    """A project with a real docs file, an alias to it, and an escape.

    docs/real.md is a real file inside the project. docs/alias.md is
    a symlink to it. docs/escape.txt is a symlink to a secret file
    that lives outside the project, and linkdir is a symlink to the
    whole outside folder.
    """
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    real = project / "docs" / "real.md"
    real.write_text("the real guide\n", encoding="utf-8")
    outside = tmp_path / "outside"
    outside.mkdir()
    secret = outside / "secret.txt"
    secret.write_text("outside the project\n", encoding="utf-8")
    make_symlink(secret, project / "docs" / "escape.txt")
    make_symlink(real, project / "docs" / "alias.md")
    make_symlink(outside, project / "linkdir")
    return project, secret


def resolved(path):
    return Path(os.path.realpath(path)).as_posix()


def test_symlink_pointing_outside_never_proves_and_is_named(tmp_path, capsys):
    project, secret = build_symlink_project(tmp_path)
    log = read_line("docs/escape.txt") + "\n"
    reads, outside = parse_trace(log, project)
    assert reads == []
    assert outside == [resolved(secret)]
    workflow = write_workflow(tmp_path)
    trace = write_log(tmp_path, "escape.log", log)
    config = write_config(tmp_path, two_check_config())
    code = main(
        ["--root", str(project), "--json", "--workflow", str(workflow),
         "--config", str(config), "--trace", str(trace)]
    )
    payload = payload_from(capsys)
    assert code == 0
    docs = check_for(payload, "docs-check")
    assert docs["verdict"] == "empty read set"
    assert docs["read_set"] == []
    assert docs["outside_reads"] == [resolved(secret)]


def test_symlink_pointing_inside_is_judged_on_the_resolved_path(tmp_path, capsys):
    project, _secret = build_symlink_project(tmp_path)
    log = read_line("docs/alias.md") + "\n"
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/real.md"]
    assert outside == []
    workflow = write_workflow(tmp_path)
    trace = write_log(tmp_path, "alias.log", log)
    config = write_config(tmp_path, two_check_config())
    code = main(
        ["--root", str(project), "--json", "--workflow", str(workflow),
         "--config", str(config), "--trace", str(trace)]
    )
    payload = payload_from(capsys)
    assert code == 0
    docs = check_for(payload, "docs-check")
    assert docs["verdict"] == "proven"
    assert docs["read_set"] == ["docs/real.md"]
    assert docs["reads_in_surface"] == ["docs/real.md"]
    assert "outside_reads" not in docs


def test_symlink_in_a_directory_component_pointing_outside_is_named(tmp_path):
    project, secret = build_symlink_project(tmp_path)
    log = read_line("linkdir/secret.txt") + "\n"
    reads, outside = parse_trace(log, project)
    assert reads == []
    assert outside == [resolved(secret)]


def test_outside_reads_are_named_in_text_and_json(tmp_path, capsys):
    project, secret = build_symlink_project(tmp_path)
    log = read_line("docs/escape.txt") + "\n" + read_line("docs/real.md") + "\n"
    workflow = write_workflow(tmp_path)
    trace = write_log(tmp_path, "mixed.log", log)
    config = write_config(tmp_path, two_check_config())
    code = main(
        ["--root", str(project), "--workflow", str(workflow),
         "--config", str(config), "--trace", str(trace)]
    )
    text = capsys.readouterr().out
    assert code == 0
    assert "outside reads" in text
    assert resolved(secret) in text
    code = main(
        ["--root", str(project), "--json", "--workflow", str(workflow),
         "--config", str(config), "--trace", str(trace)]
    )
    payload = payload_from(capsys)
    assert code == 0
    docs = check_for(payload, "docs-check")
    assert docs["verdict"] == "proven"
    assert docs["read_set"] == ["docs/real.md"]
    assert docs["outside_reads"] == [resolved(secret)]


def test_outside_reads_never_change_a_verdict(tmp_path):
    config = write_config(tmp_path, two_check_config())
    checks = load_config(config)
    outside = ["/resolved/elsewhere/secret.txt"]
    empty_results = analyse_with_outside(checks, [], outside)
    assert [result.verdict for result in empty_results] == [
        "empty read set",
        "empty read set",
    ]
    assert all(result.outside_reads == tuple(outside) for result in empty_results)
    proven_results = analyse_with_outside(checks, ["docs/real.md"], outside)
    docs = next(result for result in proven_results if result.job == "docs-check")
    assert docs.verdict == "proven"
    assert docs.matched == ("docs/real.md",)


def test_written_outside_paths_are_dropped_unnamed(tmp_path):
    project, secret = build_symlink_project(tmp_path)
    log = "\n".join(
        [
            read_line("/etc/hostname"),
            read_line(str(secret)),
            read_line("docs/../../outside/secret.txt"),
            read_line("docs/real.md"),
        ]
    ) + "\n"
    reads, outside = parse_trace(log, project)
    assert reads == ["docs/real.md"]
    assert outside == []


def test_garbage_trace_is_refused_on_the_shared_trace(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    trace = write_log(tmp_path, "garbage.log", GARBAGE)
    config = write_config(tmp_path, two_check_config())
    code = main(
        ["--root", str(tmp_path), "--workflow", str(workflow),
         "--config", str(config), "--trace", str(trace)]
    )
    err = capsys.readouterr().err
    assert code == 2
    assert "does not look like strace output" in err


def test_garbage_trace_is_refused_on_a_per_job_trace(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "garbage.log", GARBAGE)
    write_log(tmp_path, "good.log", read_line("src/app.py") + "\n")
    config = write_config(
        tmp_path,
        two_check_config(unit_extra={"trace": "garbage.log"}, docs_extra={"trace": "good.log"}),
    )
    code = main(
        ["--root", str(tmp_path), "--workflow", str(workflow), "--config", str(config)]
    )
    err = capsys.readouterr().err
    assert code == 2
    assert "unit tests" in err
    assert "does not look like strace output" in err


def test_garbage_trace_is_refused_on_a_trace_for_override(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "garbage.log", GARBAGE)
    write_log(tmp_path, "docs.log", read_line("docs/guide.md") + "\n")
    config = write_config(tmp_path, two_check_config(docs_extra={"trace": "docs.log"}))
    code = main(
        ["--root", str(tmp_path), "--workflow", str(workflow), "--config", str(config),
         "--trace-for", "unit-tests=garbage.log"]
    )
    err = capsys.readouterr().err
    assert code == 2
    assert "unit-tests" in err
    assert "does not look like strace output" in err


def test_one_recognizable_line_accepts_the_whole_trace(tmp_path, capsys):
    text = (
        "Operator notes before the capture started.\n"
        "The gate ran on the small runner again.\n"
        "99 getpid() = 99\n"
        "More notes after the capture ended.\n"
    )
    assert parse_strace_log(text, tmp_path) == []
    workflow = write_workflow(tmp_path)
    trace = write_log(tmp_path, "notes.log", text)
    config = write_config(tmp_path, two_check_config())
    code = main(
        ["--root", str(tmp_path), "--json", "--workflow", str(workflow),
         "--config", str(config), "--trace", str(trace)]
    )
    payload = payload_from(capsys)
    assert code == 0
    assert check_for(payload, "docs-check")["verdict"] == "empty read set"


def test_empty_trace_file_keeps_its_slice_one_meaning(tmp_path, capsys):
    assert parse_strace_log("", tmp_path) == []
    workflow = write_workflow(tmp_path)
    trace = write_log(tmp_path, "empty.log", "")
    config = write_config(tmp_path, two_check_config())
    code = main(
        ["--root", str(tmp_path), "--json", "--workflow", str(workflow),
         "--config", str(config), "--trace", str(trace)]
    )
    payload = payload_from(capsys)
    assert code == 0
    assert check_for(payload, "unit-tests")["verdict"] == "empty read set"
    assert check_for(payload, "docs-check")["verdict"] == "empty read set"


def test_strace_with_no_file_reads_still_yields_an_empty_read_set(tmp_path, capsys):
    text = "\n".join(
        [
            '99 execve("/bin/true", ["/bin/true"], 0x0) = 0',
            '99 read(3, "payload", 7) = 7',
            '99 openat(AT_FDCWD, "docs/missing.md", O_RDONLY) = -1 ENOENT (No such file or directory)',
            "99 exit_group(0) = ?",
        ]
    ) + "\n"
    assert parse_strace_log(text, tmp_path) == []
    workflow = write_workflow(tmp_path)
    trace = write_log(tmp_path, "noreads.log", text)
    config = write_config(tmp_path, two_check_config())
    code = main(
        ["--root", str(tmp_path), "--json", "--workflow", str(workflow),
         "--config", str(config), "--trace", str(trace)]
    )
    payload = payload_from(capsys)
    assert code == 0
    assert check_for(payload, "docs-check")["verdict"] == "empty read set"


def test_parse_trace_can_skip_the_shape_check_for_captures(tmp_path):
    reads, outside = parse_trace(GARBAGE, tmp_path, check_shape=False)
    assert reads == []
    assert outside == []


@pytest.mark.skipif(shutil.which("strace") is None, reason="strace is not available")
def test_run_capture_is_not_shape_checked(tmp_path, capsys):
    target = tmp_path / "docs"
    target.mkdir()
    (target / "guide.md").write_text("a guide\n", encoding="utf-8")
    workflow = write_workflow(tmp_path)
    config = write_config(tmp_path, two_check_config())
    code = main(
        ["--root", str(tmp_path), "--json", "--workflow", str(workflow),
         "--config", str(config), "--run", "cat", str(target / "guide.md")]
    )
    payload = payload_from(capsys)
    assert code == 0
    assert check_for(payload, "docs-check")["verdict"] == "proven"
