//! The setup pins, re-expressed in Rust at the retirement.
//!
//! These four tests carried the setup obligations in the
//! Python contract suite and stood in tests/contract.rs for
//! one commit, and at the retirement they take their
//! standing home here, on the Wave B pattern. The closed
//! round count stands pinned against the README line, and
//! the two move together at a closure and nowhere else. The
//! CI pin reads the retired shape, the cargo job alone, and
//! the annex pin reads the reduced gate sentence.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|_| panic!("the repo file {relative} stands"))
}

#[test]
fn test_seed_files_present_and_licence_intact() {
    for name in ["README.md", "LICENSE", ".gitignore"] {
        assert!(repo_root().join(name).is_file(), "{name} stands");
    }
    let licence = read_repo_file("LICENSE");
    assert!(licence.starts_with("MIT License"));
    assert!(licence.contains("Permission is hereby granted"));
}

#[test]
fn test_readme_status_section_states_the_facts() {
    let readme = read_repo_file("README.md");
    assert!(readme.contains("## Status"));
    let status = readme
        .split("## Status")
        .nth(1)
        .expect("the status section stands")
        .split("## Licence")
        .next()
        .expect("the status section ends at the licence heading");
    let lowered = status.to_lowercase();
    for fact in [
        "local seed is done",
        "403",
        "contract.md",
        "slice 1",
        "21 of 100",
        "agents.md",
        "canonical template",
        "pinned formats stand at blindgate-evidence/1",
    ] {
        assert!(lowered.contains(fact), "the status section states {fact}");
    }
}

#[test]
fn test_ci_runs_the_local_gate_command() {
    let ci = read_repo_file(".github/workflows/ci.yml");
    assert!(ci.contains("push"), "the workflow runs on push");
    assert!(
        ci.contains("pull_request"),
        "the workflow runs on pull request"
    );
    assert!(
        ci.contains("cargo test"),
        "the cargo job runs the local gate command"
    );
    assert!(
        !ci.contains("python -m pytest"),
        "the Python job is retired and its gate command is gone"
    );
}

#[test]
fn test_agents_md_stands_from_the_canonical_template() {
    let agents = read_repo_file("AGENTS.md");
    assert!(agents.contains("<!-- repo annex below this line -->"));
    assert!(agents.contains("## Repo annex"));
    let annex = agents
        .split("<!-- repo annex below this line -->")
        .nth(1)
        .expect("the annex stands below its marker");
    assert!(
        annex.contains("The local gate is cargo test."),
        "the annex gate sentence stands reduced to cargo test alone"
    );
}
