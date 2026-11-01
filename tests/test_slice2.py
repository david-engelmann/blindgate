"""Tests for the slice 2 obligations in CONTRACT.md.

Every fixture here is synthetic and written for this test file. The
traces are hand written strace openat lines, never captured from a
real session.
"""

import json
import shutil
from pathlib import Path

import pytest

from blindgate.cli import main

ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / "examples"

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


def read_line(path):
    return f'99 openat(AT_FDCWD, "{path}", O_RDONLY) = 3'


EMPTY_LOG = "99 exit_group(0) = ?\n"


def write_workflow(tmp_path):
    workflow = tmp_path / "workflow.yml"
    workflow.write_text(WORKFLOW, encoding="utf-8")
    return workflow


def write_config(tmp_path, payload, name="config.json"):
    config = tmp_path / name
    config.write_text(json.dumps(payload), encoding="utf-8")
    return config


def write_log(tmp_path, name, text):
    log = tmp_path / name
    log.write_text(text, encoding="utf-8")
    return log


def two_check_config(unit_extra=None, docs_extra=None):
    unit = {"name": "unit tests", "job": "unit-tests", "surface": ["src/**", "tests/**"]}
    docs = {"name": "docs check", "job": "docs-check", "surface": ["docs/*.md"]}
    if unit_extra:
        unit.update(unit_extra)
    if docs_extra:
        docs.update(docs_extra)
    return {"checks": [unit, docs]}


def run_json(tmp_path, argv):
    return main(["--root", str(tmp_path), "--json", *argv])


def payload_from(capsys):
    return json.loads(capsys.readouterr().out)


def test_per_check_trace_is_judged_alone(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "unit.log", read_line("docs/guide.md") + "\n")
    write_log(tmp_path, "docs.log", read_line("docs/guide.md") + "\n")
    write_log(tmp_path, "shared.log", read_line("src/app.py") + "\n")
    config = write_config(
        tmp_path,
        two_check_config(unit_extra={"trace": "unit.log"}, docs_extra={"trace": "docs.log"}),
    )
    shared = tmp_path / "shared.log"
    code = run_json(
        tmp_path,
        ["--workflow", str(workflow), "--config", str(config), "--trace", str(shared)],
    )
    payload = payload_from(capsys)
    assert code == 0
    unit, docs = payload["checks"]
    # The shared trace read src/app.py, which sits inside the unit
    # surface, but the unit check is judged on its own trace alone and
    # that trace never read its surface.
    assert unit["verdict"] == "unproven"
    assert unit["read_set"] == ["docs/guide.md"]
    assert docs["verdict"] == "proven"
    assert docs["read_set"] == ["docs/guide.md"]


def test_mixed_verdicts_from_per_job_traces_in_one_run(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "unit.log", read_line("src/app.py") + "\n")
    write_log(tmp_path, "docs.log", EMPTY_LOG)
    config = write_config(
        tmp_path,
        two_check_config(unit_extra={"trace": "unit.log"}, docs_extra={"trace": "docs.log"}),
    )
    code = main(
        ["--workflow", str(workflow), "--config", str(config), "--root", str(tmp_path)]
    )
    out = capsys.readouterr().out
    assert code == 0
    assert "proven" in out
    assert "empty read set" in out
    assert "own trace file unit.log" in out
    assert "own trace file docs.log" in out


def test_check_without_own_trace_falls_back_to_shared_trace(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "unit.log", EMPTY_LOG)
    write_log(tmp_path, "shared.log", read_line("docs/guide.md") + "\n")
    config = write_config(tmp_path, two_check_config(unit_extra={"trace": "unit.log"}))
    shared = tmp_path / "shared.log"
    code = run_json(
        tmp_path,
        ["--workflow", str(workflow), "--config", str(config), "--trace", str(shared)],
    )
    payload = payload_from(capsys)
    assert code == 0
    unit, docs = payload["checks"]
    assert unit["verdict"] == "empty read set"
    assert unit["trace_source"]["kind"] == "own_trace"
    assert docs["verdict"] == "proven"
    assert docs["read_set"] == ["docs/guide.md"]
    assert docs["trace_source"]["kind"] == "shared_trace"
    assert docs["trace_source"]["detail"] == str(shared)


@pytest.mark.skipif(shutil.which("strace") is None, reason="strace not available")
def test_check_without_own_trace_falls_back_to_shared_run(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "unit.log", EMPTY_LOG)
    config = write_config(tmp_path, two_check_config(unit_extra={"trace": "unit.log"}))
    surface_file = tmp_path / "docs" / "guide.md"
    surface_file.parent.mkdir()
    surface_file.write_text("synthetic guide\n", encoding="utf-8")
    code = run_json(
        tmp_path,
        ["--workflow", str(workflow), "--config", str(config),
         "--run", "cat", str(surface_file)],
    )
    payload = payload_from(capsys)
    assert code == 0
    unit, docs = payload["checks"]
    assert unit["verdict"] == "empty read set"
    assert unit["trace_source"]["kind"] == "own_trace"
    assert docs["verdict"] == "proven"
    assert docs["trace_source"]["kind"] == "shared_run"


def test_trace_for_overrides_the_config_trace(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "unit.log", EMPTY_LOG)
    write_log(tmp_path, "shared.log", read_line("docs/guide.md") + "\n")
    override = write_log(tmp_path, "override.log", read_line("src/app.py") + "\n")
    config = write_config(tmp_path, two_check_config(unit_extra={"trace": "unit.log"}))
    code = run_json(
        tmp_path,
        ["--workflow", str(workflow), "--config", str(config),
         "--trace", str(tmp_path / "shared.log"),
         "--trace-for", f"unit-tests={override}"],
    )
    payload = payload_from(capsys)
    assert code == 0
    (unit,) = [check for check in payload["checks"] if check["job"] == "unit-tests"]
    assert unit["verdict"] == "proven"
    assert unit["trace_source"]["kind"] == "own_trace"
    assert unit["trace_source"]["detail"] == str(override)


def test_trace_for_with_an_unknown_job_is_a_reported_error(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "shared.log", read_line("src/app.py") + "\n")
    config = write_config(tmp_path, two_check_config())
    shared = tmp_path / "shared.log"
    code = main(
        ["--workflow", str(workflow), "--config", str(config),
         "--trace", str(shared), "--root", str(tmp_path),
         "--trace-for", "no-such-job=whatever.log"]
    )
    captured = capsys.readouterr()
    assert code == 2
    assert "no-such-job" in captured.err
    assert "proven" not in captured.out


def test_trace_for_without_an_equals_sign_is_a_reported_error(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    config = write_config(tmp_path, two_check_config())
    code = main(
        ["--workflow", str(workflow), "--config", str(config), "--root", str(tmp_path),
         "--trace-for", "unit-tests"]
    )
    captured = capsys.readouterr()
    assert code == 2
    assert "equals sign" in captured.err


def test_missing_per_check_trace_is_a_named_error(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    config = write_config(
        tmp_path, two_check_config(unit_extra={"trace": "missing-unit.log"})
    )
    code = main(
        ["--workflow", str(workflow), "--config", str(config), "--root", str(tmp_path)]
    )
    captured = capsys.readouterr()
    assert code == 2
    assert "unit tests" in captured.err
    assert "missing-unit.log" in captured.err


def test_missing_trace_for_file_is_a_named_error(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "unit.log", read_line("src/app.py") + "\n")
    config = write_config(tmp_path, two_check_config(unit_extra={"trace": "unit.log"}))
    code = main(
        ["--workflow", str(workflow), "--config", str(config), "--root", str(tmp_path),
         "--trace-for", "docs-check=nope.log"]
    )
    captured = capsys.readouterr()
    assert code == 2
    assert "docs-check" in captured.err
    assert "nope.log" in captured.err


def test_uncovered_check_without_a_shared_source_is_a_named_error(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "unit.log", read_line("src/app.py") + "\n")
    config = write_config(tmp_path, two_check_config(unit_extra={"trace": "unit.log"}))
    code = main(
        ["--workflow", str(workflow), "--config", str(config), "--root", str(tmp_path)]
    )
    captured = capsys.readouterr()
    assert code == 2
    assert "docs check" in captured.err
    assert "no shared trace or run" in captured.err


def test_all_checks_with_own_traces_need_no_shared_source(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "unit.log", read_line("src/app.py") + "\n")
    write_log(tmp_path, "docs.log", read_line("docs/guide.md") + "\n")
    config = write_config(
        tmp_path,
        two_check_config(unit_extra={"trace": "unit.log"}, docs_extra={"trace": "docs.log"}),
    )
    code = main(
        ["--workflow", str(workflow), "--config", str(config), "--root", str(tmp_path)]
    )
    out = capsys.readouterr().out
    assert code == 0
    assert out.count("proven") == 2


def test_report_states_each_trace_source(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "unit.log", read_line("src/app.py") + "\n")
    write_log(tmp_path, "shared.log", read_line("docs/guide.md") + "\n")
    config = write_config(tmp_path, two_check_config(unit_extra={"trace": "unit.log"}))
    argv = ["--workflow", str(workflow), "--config", str(config),
            "--trace", str(tmp_path / "shared.log"), "--root", str(tmp_path)]
    code = main(argv)
    out = capsys.readouterr().out
    assert code == 0
    assert "trace source     own trace file unit.log" in out
    assert "trace source     shared trace" in out


def test_mixed_report_still_never_says_fraud(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "unit.log", EMPTY_LOG)
    write_log(tmp_path, "shared.log", read_line("elsewhere.bin") + "\n")
    config = write_config(tmp_path, two_check_config(unit_extra={"trace": "unit.log"}))
    argv = ["--workflow", str(workflow), "--config", str(config),
            "--trace", str(tmp_path / "shared.log"), "--root", str(tmp_path)]
    for extra in ((), ("--json",)):
        code = main([*argv, *extra])
        rendered = capsys.readouterr().out
        assert code == 0
        assert "fraud" not in rendered.lower()


def _slice_one_example_argv(extra=()):
    return [
        "--workflow", str(EXAMPLES / "workflow.yml"),
        "--config", str(EXAMPLES / "blindgate.yml"),
        "--trace", str(EXAMPLES / "trace.log"),
        "--root", str(EXAMPLES),
        *extra,
    ]


def test_slice_one_example_reports_regenerate_byte_identical(capsys):
    code = main(_slice_one_example_argv())
    out = capsys.readouterr().out
    assert code == 0
    assert "trace source" not in out
    assert out == (EXAMPLES / "report.txt").read_text(encoding="utf-8")


def test_slice_one_example_json_regenerates_byte_identical(capsys):
    code = main(_slice_one_example_argv(("--json",)))
    out = capsys.readouterr().out
    assert code == 0
    assert "trace_source" not in out
    assert out == (EXAMPLES / "report.json").read_text(encoding="utf-8")


def _per_job_example_argv(extra=()):
    folder = EXAMPLES / "per-job"
    return [
        "--workflow", str(folder / "workflow.yml"),
        "--config", str(folder / "blindgate.yml"),
        "--root", str(folder),
        *extra,
    ]


def test_per_job_example_reports_regenerate_byte_identical(capsys):
    folder = EXAMPLES / "per-job"
    code = main(_per_job_example_argv())
    out = capsys.readouterr().out
    assert code == 0
    assert "own trace file trace-unit.log" in out
    assert "empty read set" in out
    assert out == (folder / "report.txt").read_text(encoding="utf-8")
    code = main(_per_job_example_argv(("--json",)))
    out = capsys.readouterr().out
    assert code == 0
    payload = json.loads(out)
    assert [check["verdict"] for check in payload["checks"]] == ["proven", "empty read set"]
    assert out == (folder / "report.json").read_text(encoding="utf-8")
