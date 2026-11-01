"""Parsing of GitHub Actions workflow files.

Only one fact is needed from a workflow in slice 1, and that is the
set of job ids declared under the jobs key.
"""

from __future__ import annotations

from pathlib import Path

import yaml

from .errors import BlindgateError


def load_workflow_jobs(path):
    """Return the job ids declared in the workflow at path.

    Raises BlindgateError when the file cannot be read, is not a
    mapping, or declares no jobs.
    """
    workflow_path = Path(path)
    try:
        text = workflow_path.read_text(encoding="utf-8")
    except OSError as exc:
        raise BlindgateError(f"cannot read workflow file {workflow_path} ({exc})") from exc
    try:
        data = yaml.safe_load(text)
    except yaml.YAMLError as exc:
        raise BlindgateError(f"workflow file {workflow_path} is not valid YAML ({exc})") from exc
    if not isinstance(data, dict):
        raise BlindgateError(f"workflow file {workflow_path} does not hold a mapping")
    jobs = data.get("jobs")
    if not isinstance(jobs, dict) or not jobs:
        raise BlindgateError(f"workflow file {workflow_path} declares no jobs")
    return set(str(job_id) for job_id in jobs.keys())
