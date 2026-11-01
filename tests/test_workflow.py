from pathlib import Path

import pytest

from blindgate.errors import BlindgateError
from blindgate.workflow import load_workflow_jobs

FIXTURES = Path(__file__).resolve().parent / "fixtures"


def test_extracts_job_ids():
    jobs = load_workflow_jobs(FIXTURES / "workflow.yml")
    assert jobs == {"unit-tests", "docs-check"}


def test_workflow_without_jobs_is_an_error(tmp_path):
    workflow = tmp_path / "empty.yml"
    workflow.write_text("name: nothing runs here\non: push\n", encoding="utf-8")
    with pytest.raises(BlindgateError) as excinfo:
        load_workflow_jobs(workflow)
    assert "no jobs" in str(excinfo.value)


def test_missing_workflow_file_is_an_error(tmp_path):
    with pytest.raises(BlindgateError):
        load_workflow_jobs(tmp_path / "does-not-exist.yml")
