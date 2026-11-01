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
