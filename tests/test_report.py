import json

from blindgate.analysis import analyse
from blindgate.config import Check
from blindgate.report import render_json, render_text

UNIT = Check(name="unit tests", job="unit-tests", surface=("src/**", "tests/**"))
READS = ["src/app.py", "src/util.py", "notes.txt"]


def test_text_report_pairs_check_surface_reads_and_verdict():
    (result,) = analyse([UNIT], READS)
    text = render_text([result])
    assert "unit tests" in text
    assert "unit-tests" in text
    assert "proven" in text
    for glob in ("src/**", "tests/**"):
        assert glob in text
    for path in READS:
        assert path in text


def test_json_report_carries_the_same_facts():
    (result,) = analyse([UNIT], READS)
    payload = json.loads(render_json([result]))
    assert payload["summary"] == {"proven": 1}
    (check,) = payload["checks"]
    assert check["name"] == "unit tests"
    assert check["job"] == "unit-tests"
    assert check["verdict"] == "proven"
    assert check["claimed_surface"] == ["src/**", "tests/**"]
    assert check["read_set"] == READS
    assert check["reads_in_surface"] == ["src/app.py", "src/util.py"]
