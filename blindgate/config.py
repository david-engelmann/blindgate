"""The claimed surface declaration.

A config file names each required check, the workflow job that runs
it, and the surface that check claims to certify, given as path globs.
JSON and YAML are both accepted, picked by file suffix.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

import yaml

from .errors import BlindgateError


@dataclass(frozen=True)
class Check:
    name: str
    job: str
    surface: tuple
    trace: str = None
    trace_path: Path | None = None


def _load_data(config_path):
    text = config_path.read_text(encoding="utf-8")
    suffix = config_path.suffix.lower()
    if suffix == ".json":
        try:
            return json.loads(text)
        except json.JSONDecodeError as exc:
            raise BlindgateError(f"config file {config_path} is not valid JSON ({exc})") from exc
    if suffix in (".yaml", ".yml"):
        try:
            return yaml.safe_load(text)
        except yaml.YAMLError as exc:
            raise BlindgateError(f"config file {config_path} is not valid YAML ({exc})") from exc
    raise BlindgateError(
        f"config file {config_path} has an unsupported suffix, use .json, .yaml, or .yml"
    )


def load_config(path):
    """Return the list of Check objects declared in the config at path."""
    config_path = Path(path)
    try:
        data = _load_data(config_path)
    except OSError as exc:
        raise BlindgateError(f"cannot read config file {config_path} ({exc})") from exc
    if not isinstance(data, dict) or not isinstance(data.get("checks"), list):
        raise BlindgateError(f"config file {config_path} must hold a checks list")
    checks = []
    for index, entry in enumerate(data["checks"]):
        if not isinstance(entry, dict):
            raise BlindgateError(f"config check number {index} is not a mapping")
        name = entry.get("name")
        job = entry.get("job")
        surface = entry.get("surface")
        if not name or not isinstance(name, str):
            raise BlindgateError(f"config check number {index} has no name")
        if not job or not isinstance(job, str):
            raise BlindgateError(f"check {name} has no job id")
        if not isinstance(surface, list) or not surface:
            raise BlindgateError(f"check {name} declares no claimed surface")
        if not all(isinstance(item, str) and item for item in surface):
            raise BlindgateError(f"check {name} has a claimed surface entry that is not text")
        trace = entry.get("trace")
        trace_path = None
        if trace is not None:
            if not isinstance(trace, str) or not trace:
                raise BlindgateError(
                    f"check {name} has a trace field that is not a non empty piece of text"
                )
            candidate = Path(trace)
            if not candidate.is_absolute():
                candidate = config_path.resolve().parent / candidate
            trace_path = candidate
        checks.append(
            Check(name=name, job=job, surface=tuple(surface), trace=trace, trace_path=trace_path)
        )
    if not checks:
        raise BlindgateError(f"config file {config_path} declares no checks")
    return checks


def unknown_jobs(checks, workflow_jobs):
    """Return the checks whose job id is missing from the workflow."""
    return [check for check in checks if check.job not in workflow_jobs]
