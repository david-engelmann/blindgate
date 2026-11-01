from blindgate import VERDICT_EMPTY, VERDICT_PROVEN, VERDICT_UNPROVEN
from blindgate.analysis import analyse
from blindgate.config import Check
from blindgate.report import render_json, render_text

UNIT = Check(name="unit tests", job="unit-tests", surface=("src/**", "tests/**"))
DOCS = Check(name="docs check", job="docs-check", surface=("docs/*.md",))


def test_proven_when_a_read_lands_in_the_surface():
    (result,) = analyse([UNIT], ["src/app.py", "README.md"])
    assert result.verdict == VERDICT_PROVEN
    assert result.matched == ("src/app.py",)
    assert result.read_set == ("src/app.py", "README.md")


def test_unproven_when_reads_miss_the_surface():
    (result,) = analyse([DOCS], ["src/app.py", "notes.txt"])
    assert result.verdict == VERDICT_UNPROVEN
    assert result.matched == ()
    assert result.read_set == ("src/app.py", "notes.txt")


def test_empty_when_the_read_set_is_empty():
    (result,) = analyse([DOCS], [])
    assert result.verdict == VERDICT_EMPTY
    assert result.verdict == "empty read set"
    assert result.matched == ()


def test_directory_prefix_surface_matches():
    check = Check(name="docs dir", job="docs-check", surface=("docs",))
    (result,) = analyse([check], ["docs/deep/guide.md"])
    assert result.verdict == VERDICT_PROVEN


def test_no_verdict_or_report_says_fraud():
    results = analyse([UNIT, DOCS], ["elsewhere.bin"])
    assert [result.verdict for result in results] == [VERDICT_UNPROVEN, VERDICT_UNPROVEN]
    empty_results = analyse([DOCS], [])
    for rendered in (render_text(results), render_json(results), render_text(empty_results)):
        assert "fraud" not in rendered.lower()
