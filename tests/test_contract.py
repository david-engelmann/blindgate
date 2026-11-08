"""Tests that pin the setup obligations in CONTRACT.md."""

from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]


def test_seed_files_present_and_licence_intact():
    for name in ("README.md", "LICENSE", ".gitignore"):
        assert (ROOT / name).is_file(), name
    licence = (ROOT / "LICENSE").read_text(encoding="utf-8")
    assert licence.startswith("MIT License")
    assert "Permission is hereby granted" in licence


def test_readme_status_section_states_the_facts():
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    assert "## Status" in readme
    status = readme.split("## Status", 1)[1].split("## Licence", 1)[0]
    lowered = status.lower()
    for fact in (
        "local seed is done",
        "403",
        "contract.md",
        "slice 1",
        "17 of 100",
        "agents.md",
        "canonical template",
        "pinned formats stand at none",
    ):
        assert fact in lowered, fact


def test_ci_runs_the_local_gate_command():
    ci_path = ROOT / ".github" / "workflows" / "ci.yml"
    data = yaml.safe_load(ci_path.read_text(encoding="utf-8"))
    triggers = data.get("on", data.get(True))
    assert "push" in triggers
    assert "pull_request" in triggers
    steps = data["jobs"]["test"]["steps"]
    commands = [step.get("run", "") for step in steps]
    assert any("python -m pytest" in command for command in commands)


def test_agents_md_stands_from_the_canonical_template():
    agents = (ROOT / "AGENTS.md").read_text(encoding="utf-8")
    assert "<!-- repo annex below this line -->" in agents
    assert "## Repo annex" in agents
