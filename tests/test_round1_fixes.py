"""Tests for the Round 1 review fix obligations in CONTRACT.md.

Every fixture here is synthetic and written for this test file. The
traces are hand written strace openat lines, never captured from a
real session.
"""

import json
from pathlib import Path

from blindgate.cli import main
from blindgate.tracing import parse_strace_log

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


def verdicts(payload):
    return {check["job"]: check["verdict"] for check in payload["checks"]}


def test_relative_dotdot_escape_is_dropped(tmp_path):
    log = '99 openat(AT_FDCWD, "src/../../outside/secret.txt", O_RDONLY) = 3\n'
    assert parse_strace_log(log, tmp_path) == []


def test_relative_dotdot_inside_root_normalises(tmp_path):
    log = '99 openat(AT_FDCWD, "src/../docs/guide.md", O_RDONLY) = 3\n'
    assert parse_strace_log(log, tmp_path) == ["docs/guide.md"]


def test_absolute_dotdot_lands_where_it_points(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    landed = tmp_path / "src" / ".." / "docs" / "guide.md"
    write_log(tmp_path, "shared.log", read_line(str(landed)) + "\n")
    config = write_config(tmp_path, two_check_config())
    code = run_json(
        tmp_path,
        ["--workflow", str(workflow), "--config", str(config),
         "--trace", str(tmp_path / "shared.log")],
    )
    payload = payload_from(capsys)
    assert code == 0
    assert verdicts(payload) == {"unit-tests": "unproven", "docs-check": "proven"}


def test_unfinished_open_counts_as_no_read(tmp_path):
    log = '99 openat(AT_FDCWD, "src/app.py", O_RDONLY <unfinished ...>\n'
    assert parse_strace_log(log, tmp_path) == []


def test_o_path_open_counts_as_no_read(tmp_path):
    log = '99 openat(AT_FDCWD, "src/app.py", O_RDONLY|O_PATH) = 3\n'
    assert parse_strace_log(log, tmp_path) == []


def test_directory_open_counts_as_no_read(tmp_path):
    log = '99 openat(AT_FDCWD, "src", O_RDONLY|O_DIRECTORY) = 3\n'
    assert parse_strace_log(log, tmp_path) == []


def test_trace_for_relative_path_resolves_against_the_config_folder(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "override.log", read_line("src/app.py") + "\n")
    write_log(tmp_path, "shared.log", read_line("docs/guide.md") + "\n")
    config = write_config(tmp_path, two_check_config())
    code = run_json(
        tmp_path,
        ["--workflow", str(workflow), "--config", str(config),
         "--trace", str(tmp_path / "shared.log"),
         "--trace-for", "unit-tests=override.log"],
    )
    payload = payload_from(capsys)
    assert code == 0
    assert verdicts(payload)["unit-tests"] == "proven"


def test_duplicate_trace_for_job_is_a_reported_error(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "a.log", read_line("src/app.py") + "\n")
    config = write_config(tmp_path, two_check_config())
    code = main(
        ["--root", str(tmp_path), "--workflow", str(workflow), "--config", str(config),
         "--trace", str(tmp_path / "a.log"),
         "--trace-for", "unit-tests=a.log", "--trace-for", "unit-tests=a.log"]
    )
    err = capsys.readouterr().err
    assert code == 2
    assert "unit-tests" in err


def test_both_trace_and_run_is_a_reported_error(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "a.log", read_line("src/app.py") + "\n")
    config = write_config(tmp_path, two_check_config())
    code = main(
        ["--root", str(tmp_path), "--workflow", str(workflow), "--config", str(config),
         "--trace", str(tmp_path / "a.log"), "--run", "cat", "x"]
    )
    err = capsys.readouterr().err
    assert code == 2
    assert "exactly one of --trace or --run" in err


def test_missing_shared_trace_errors_even_when_no_check_needs_it(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    write_log(tmp_path, "unit.log", read_line("src/app.py") + "\n")
    write_log(tmp_path, "docs.log", read_line("docs/guide.md") + "\n")
    config = write_config(
        tmp_path,
        two_check_config(unit_extra={"trace": "unit.log"}, docs_extra={"trace": "docs.log"}),
    )
    code = main(
        ["--root", str(tmp_path), "--workflow", str(workflow), "--config", str(config),
         "--trace", str(tmp_path / "does-not-exist.log")]
    )
    err = capsys.readouterr().err
    assert code == 2
    assert "does-not-exist.log" in err


def test_empty_trace_field_is_a_named_error_with_the_contract_wording(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    config = write_config(tmp_path, two_check_config(unit_extra={"trace": ""}))
    code = main(
        ["--root", str(tmp_path), "--workflow", str(workflow), "--config", str(config),
         "--trace", str(tmp_path / "a.log")]
    )
    err = capsys.readouterr().err
    assert code == 2
    assert "not a non empty piece of text" in err


def test_list_trace_field_is_a_named_error(tmp_path, capsys):
    workflow = write_workflow(tmp_path)
    config = write_config(tmp_path, two_check_config(unit_extra={"trace": ["unit.log"]}))
    code = main(
        ["--root", str(tmp_path), "--workflow", str(workflow), "--config", str(config),
         "--trace", str(tmp_path / "a.log")]
    )
    err = capsys.readouterr().err
    assert code == 2
    assert "not a non empty piece of text" in err
