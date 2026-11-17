//! The slice 25 pins the contract names: a percent escape
//! in a tag suffix or a verbatim tag name decodes under
//! S25-2, the decoded name reads as S25-3 states, the
//! spelling stands whole under S25-4, and the limits of
//! S25-5 hold.

use blindgate::{parse_yaml, IntVal, Value};

fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

fn int(value: &str) -> Value {
    Value::Int(IntVal::from_decimal(value))
}

fn map(entries: Vec<(Value, Value)>) -> Value {
    Value::Map(entries)
}

const UNREAD: &str = "a tag the subset does not read stands in the document";

#[test]
fn test_an_escaped_suffix_reads_its_carried_type() {
    let doc = parse_yaml("a: !!%73tr 42\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&text("42")));
    let doc = parse_yaml("a: !!in%74 '42'\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&int("42")));
}

#[test]
fn test_a_wholly_escaped_suffix_reads() {
    let doc = parse_yaml("a: !!%73%74%72 42\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&text("42")));
}

#[test]
fn test_lowercase_hex_escapes_read() {
    let doc = parse_yaml("a: !!%69%6et 42\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&int("42")));
}

#[test]
fn test_a_verbatim_escaped_name_reads() {
    let doc = parse_yaml("a: !<tag:yaml.org,2002:%73tr> 42\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&text("42")));
    let doc = parse_yaml("a: !<%74ag:yaml.org,2002:str> 42\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&text("42")));
}

#[test]
fn test_a_named_handle_suffix_escape_reads() {
    let doc = parse_yaml("%TAG !e! tag:yaml.org,2002:\n--- !e!%73tr 42\n")
        .expect("the document reads");
    assert_eq!(doc, text("42"));
}

#[test]
fn test_an_escaped_tag_reads_on_a_key_and_in_flow() {
    let doc = parse_yaml("? !!%73tr k\n: v\n").expect("the document reads");
    assert_eq!(doc.get("k"), Some(&text("v")));
    let doc = parse_yaml("{a: !!%69nt 5}\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&int("5")));
}

#[test]
fn test_a_collection_tag_spelled_by_escape_reads() {
    let doc = parse_yaml("a: !!%73eq [1, 2]\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&Value::List(vec![int("1"), int("2")])));
    let doc = parse_yaml("a: !!%6dap {b: 1}\n").expect("the document reads");
    assert_eq!(
        doc.get("a"),
        Some(&map(vec![(text("b"), int("1"))]))
    );
}

#[test]
fn test_an_escape_decoding_outside_the_carried_set_refuses() {
    for doc in [
        "a: !!str%20x 42\n",
        "a: !!foo%20bar 1\n",
        "a: !!str%3Ax 1\n",
    ] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(err, UNREAD, "document {doc:?}");
    }
}

#[test]
fn test_a_double_escape_decodes_once_and_refuses() {
    let err = parse_yaml("a: !!str%2520 1\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_an_escaped_spelling_stands_whole() {
    let err = parse_yaml("a: !!str%41 1\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_a_malformed_escape_refuses() {
    for doc in [
        "a: !!str%2 1\n",
        "a: !!str%zz 1\n",
        "a: !!str% 1\n",
        "a: !<tag:yaml.org,2002:str%2> 1\n",
    ] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(err, UNREAD, "document {doc:?}");
    }
}

#[test]
fn test_an_escape_decoding_to_invalid_utf8_refuses() {
    for doc in ["a: !!foo%FFbar 1\n", "a: !!foo%C3bar 1\n"] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(err, UNREAD, "document {doc:?}");
    }
}

#[test]
fn test_a_percent_in_a_plain_scalar_stays_literal() {
    let doc = parse_yaml("a: 100%25\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&text("100%25")));
}

#[test]
fn test_an_escape_in_a_handle_place_refuses() {
    let err = parse_yaml("a: !%65!x 1\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}
