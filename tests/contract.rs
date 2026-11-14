//! The contract audit pins of the closing slice.
//!
//! Every test name the contract carries stands disposed in
//! tests/contract_name_map.txt as a Rust test standing in the
//! tree, as a recorded section of one of the conformance
//! batteries, or as a name the contract itself records as
//! renamed, and each disposition stands as claimed. The four
//! setup pins the Python contract suite carried stood here
//! in Rust for one commit and stand in tests/setup.rs from
//! the retirement on. The contract, the readme, and the
//! agents file carry no forbidden dash.
//!
//! The names are read out of the contract text itself. A
//! word standing as the stem of a suite file path,
//! tests/test_cli.py and its siblings, names a file and not
//! a test, and the reading leaves those out. The census
//! stands pinned at the count the contract carries at this
//! slice, so a name the contract gains moves the pin
//! deliberately with its disposition.
//!
//! A recorded section stands in two readings. A recordings
//! file holding a sections object names the section as a key
//! with a nonempty list, and a recordings file holding a
//! flat case list names it as the kind field of its cases,
//! or as the family field of the command line battery's
//! cases, with at least one case standing under it.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use blindgate::{parse_json, Value};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|_| panic!("the repo file {relative} stands"))
}

fn contract_names() -> BTreeSet<String> {
    let contract = read_repo_file("CONTRACT.md");
    let mut names = BTreeSet::new();
    for token in
        contract.split(|ch: char| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'))
    {
        if token.len() > 5 && token.starts_with("test_") {
            if contract.contains(&format!("tests/{token}.py")) {
                continue;
            }
            names.insert(token.to_string());
        }
    }
    names
}

fn rust_test_stands(file: &str, name: &str) -> bool {
    let Ok(text) = fs::read_to_string(repo_root().join(file)) else {
        return false;
    };
    text.contains(&format!("fn {name}("))
}

fn recording_section_stands(file: &str, wanted: &str) -> bool {
    let Ok(text) = fs::read_to_string(repo_root().join(file)) else {
        return false;
    };
    let Ok(document) = parse_json(&text) else {
        return false;
    };
    if let Some(Value::Map(sections)) = document.get("sections") {
        let stands = sections.iter().any(|(name, value)| {
            matches!(name, Value::Text(name) if name == wanted)
                && matches!(value, Value::List(items) if !items.is_empty())
        });
        if stands {
            return true;
        }
    }
    if let Some(Value::List(cases)) = document.get("cases") {
        return cases.iter().any(|case| {
            matches!(case.get("kind"), Some(Value::Text(kind)) if kind == wanted)
                || matches!(case.get("family"), Some(Value::Text(family)) if family == wanted)
        });
    }
    false
}

#[test]
fn test_every_contract_name_is_disposed() {
    let names = contract_names();
    assert_eq!(
        names.len(),
        296,
        "the contract carries the census of names this slice disposes"
    );
    let map = read_repo_file("tests/contract_name_map.txt");
    let mut disposed: BTreeSet<String> = BTreeSet::new();
    let mut map_names: BTreeSet<String> = BTreeSet::new();
    for line in map.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let name = line.split(' ').next().expect("a map line names its test");
        map_names.insert(name.to_string());
    }
    for line in map.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let name = parts.next().expect("a map line names its test");
        let kind = parts.next().expect("a map line names its disposition");
        let rest = parts.next().expect("a map line names its target");
        assert!(
            names.contains(name),
            "the map disposes a name the contract does not carry, {name}"
        );
        let stands = match kind {
            "rust-test" => rust_test_stands(rest, name),
            "recording-section" => {
                let (file, section) = rest
                    .split_once(' ')
                    .expect("a section disposition names its file and section");
                recording_section_stands(file, section)
            }
            "re-expressed" => map_names.contains(rest),
            other => panic!("the map carries an unknown disposition {other}"),
        };
        assert!(stands, "the disposition of {name} stands as claimed");
        disposed.insert(name.to_string());
    }
    for name in &names {
        assert!(
            disposed.contains(name),
            "the contract name {name} stands disposed"
        );
    }
}

#[test]
fn test_contract_has_no_forbidden_dashes() {
    for file in ["CONTRACT.md", "README.md", "AGENTS.md"] {
        let text = read_repo_file(file);
        assert!(!text.contains('\u{2014}'), "{file} carries no em dash");
        assert!(!text.contains('\u{2013}'), "{file} carries no en dash");
    }
}
