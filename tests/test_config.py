from pathlib import Path

import pytest

from blindgate.config import load_config, unknown_jobs
from blindgate.errors import BlindgateError

FIXTURES = Path(__file__).resolve().parent / "fixtures"


def test_loads_json_config():
    checks = load_config(FIXTURES / "config.json")
    assert [check.name for check in checks] == ["unit tests", "docs check"]
    assert checks[0].job == "unit-tests"
    assert checks[0].surface == ("src/**", "tests/**")
    assert checks[1].surface == ("docs/*.md",)


def test_loads_yaml_config():
    checks = load_config(FIXTURES / "config.yaml")
    assert len(checks) == 1
    assert checks[0].name == "unit tests"
    assert checks[0].job == "unit-tests"
    assert checks[0].surface == ("src/**", "tests/**")


def test_config_without_a_surface_is_an_error(tmp_path):
    config = tmp_path / "bad.json"
    config.write_text(
        '{"checks": [{"name": "x", "job": "unit-tests", "surface": []}]}',
        encoding="utf-8",
    )
    with pytest.raises(BlindgateError):
        load_config(config)


def test_unknown_jobs_are_found():
    checks = load_config(FIXTURES / "config.json")
    unknown = unknown_jobs(checks, {"unit-tests"})
    assert [check.job for check in unknown] == ["docs-check"]


def test_loads_a_per_check_trace_field(tmp_path):
    config = tmp_path / "config.json"
    config.write_text(
        '{"checks": [{"name": "unit tests", "job": "unit-tests",'
        ' "surface": ["src/**"], "trace": "logs/unit.log"}]}',
        encoding="utf-8",
    )
    (check,) = load_config(config)
    assert check.trace == "logs/unit.log"
    assert check.trace_path == (tmp_path / "logs" / "unit.log").resolve()


def test_config_without_a_trace_field_loads_with_no_trace():
    checks = load_config(FIXTURES / "config.json")
    assert all(check.trace is None and check.trace_path is None for check in checks)


def test_trace_field_must_be_text(tmp_path):
    config = tmp_path / "bad.json"
    config.write_text(
        '{"checks": [{"name": "x", "job": "unit-tests",'
        ' "surface": ["src/**"], "trace": 7}]}',
        encoding="utf-8",
    )
    with pytest.raises(BlindgateError):
        load_config(config)
