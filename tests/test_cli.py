import json
import shutil
from pathlib import Path

import pytest

from blindgate.cli import main

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


def write_inputs(tmp_path, config_text, log_text):
    workflow = tmp_path / "workflow.yml"
    workflow.write_text(WORKFLOW, encoding="utf-8")
    config = tmp_path / "config.json"
    config.write_text(config_text, encoding="utf-8")
    trace = tmp_path / "trace.log"
    trace.write_text(log_text, encoding="utf-8")
    return workflow, config, trace


def run_cli(tmp_path, config_text, log_text, extra=()):
    workflow, config, trace = write_inputs(tmp_path, config_text, log_text)
    argv = [
        "--workflow", str(workflow),
        "--config", str(config),
        "--trace", str(trace),
        "--root", str(tmp_path),
        *extra,
    ]
    return main(argv)


UNIT_CONFIG = (
    '{"checks": [{"name": "unit tests", "job": "unit-tests",'
    ' "surface": ["src/**"]}]}'
)
DOCS_CONFIG = (
    '{"checks": [{"name": "docs check", "job": "docs-check",'
    ' "surface": ["docs/*.md"]}]}'
)


def read_line(path):
    return f'99 openat(AT_FDCWD, "{path}", O_RDONLY) = 3'


def test_proven_run_exits_zero_and_reports(tmp_path, capsys):
    code = run_cli(tmp_path, UNIT_CONFIG, read_line("src/app.py"))
    out = capsys.readouterr().out
    assert code == 0
    assert "unit tests" in out
    assert "proven" in out
    assert "src/app.py" in out


def test_unproven_verdict_still_exits_zero(tmp_path, capsys):
    code = run_cli(tmp_path, DOCS_CONFIG, read_line("src/app.py"))
    out = capsys.readouterr().out
    assert code == 0
    assert "unproven" in out
    assert "fraud" not in out.lower()


def test_empty_read_set_is_named(tmp_path, capsys):
    code = run_cli(tmp_path, DOCS_CONFIG, "99 exit_group(0) = ?\n")
    out = capsys.readouterr().out
    assert code == 0
    assert "empty read set" in out


def test_json_output_parses(tmp_path, capsys):
    code = run_cli(tmp_path, UNIT_CONFIG, read_line("src/app.py"), extra=("--json",))
    payload = json.loads(capsys.readouterr().out)
    assert code == 0
    assert payload["checks"][0]["verdict"] == "proven"
    assert payload["checks"][0]["reads_in_surface"] == ["src/app.py"]


def test_unknown_job_is_a_reported_error(tmp_path, capsys):
    config = (
        '{"checks": [{"name": "ghost check", "job": "no-such-job",'
        ' "surface": ["src/**"]}]}'
    )
    code = run_cli(tmp_path, config, read_line("src/app.py"))
    captured = capsys.readouterr()
    assert code == 2
    assert "no-such-job" in captured.err
    assert "proven" not in captured.out


def test_missing_trace_and_run_is_an_error(tmp_path, capsys):
    workflow, config, _trace = write_inputs(tmp_path, UNIT_CONFIG, "")
    code = main(
        ["--workflow", str(workflow), "--config", str(config), "--root", str(tmp_path)]
    )
    captured = capsys.readouterr()
    assert code == 2
    assert "exactly one of --trace or --run" in captured.err


@pytest.mark.skipif(shutil.which("strace") is None, reason="strace not available")
def test_run_mode_captures_reads_from_a_live_command(tmp_path, capsys):
    surface_file = tmp_path / "surface" / "thing.txt"
    surface_file.parent.mkdir()
    surface_file.write_text("synthetic payload\n", encoding="utf-8")
    workflow = tmp_path / "workflow.yml"
    workflow.write_text(WORKFLOW, encoding="utf-8")
    config = tmp_path / "config.json"
    config.write_text(
        '{"checks": [{"name": "surface reader", "job": "unit-tests",'
        ' "surface": ["surface/**"]}]}',
        encoding="utf-8",
    )
    code = main(
        [
            "--workflow", str(workflow),
            "--config", str(config),
            "--root", str(tmp_path),
            "--run", "cat", str(surface_file),
        ]
    )
    out = capsys.readouterr().out
    assert code == 0
    assert "proven" in out
    assert "surface/thing.txt" in out
    assert not list(Path(tmp_path).glob(".blindgate-trace-*"))
