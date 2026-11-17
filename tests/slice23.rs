//! The slice 23 pins the contract names: directives
//! read as S23-2 and S23-3 state, and the refusal
//! wordings of S23-5 stand as stated.

use blindgate::{parse_yaml, IntVal, Value};

fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

#[test]
fn test_a_version_directive_reads_the_document() {
    for doc in [
        "%YAML 1.1\n---\na: 1\n",
        "%YAML 1.2\n---\na: 1\n",
        "%YAML 1.3\n---\na: 1\n",
        "%YAML 01.2\n---\na: 1\n",
    ] {
        let read = parse_yaml(doc).expect("the document reads");
        assert_eq!(
            read.get("a"),
            Some(&Value::Int(IntVal::from_decimal("1"))),
            "document {doc:?}"
        );
    }
}

#[test]
fn test_a_version_directive_changes_no_resolution() {
    for doc in ["%YAML 1.1\n---\non\n", "%YAML 1.2\n---\non\n"] {
        assert_eq!(parse_yaml(doc), Ok(Value::Bool(true)), "document {doc:?}");
    }
    for doc in ["%YAML 1.1\n---\n1:20\n", "%YAML 1.2\n---\n1:20\n"] {
        assert_eq!(
            parse_yaml(doc),
            Ok(Value::Int(IntVal::from_decimal("80"))),
            "document {doc:?}"
        );
    }
}

#[test]
fn test_a_tag_directive_reads_a_carried_type_through_a_named_handle() {
    let root =
        parse_yaml("%TAG !e! tag:yaml.org,2002:\n---\n!e!str 5\n").expect("the document reads");
    assert_eq!(root, text("5"));
    let value = parse_yaml("%TAG !e! tag:yaml.org,2002:\n---\na: !e!int '42'\n")
        .expect("the document reads");
    assert_eq!(
        value.get("a"),
        Some(&Value::Int(IntVal::from_decimal("42")))
    );
}

#[test]
fn test_a_tag_directive_redefines_the_primary_handle() {
    let root = parse_yaml("%TAG ! tag:yaml.org,2002:\n---\n!str 5\n").expect("the document reads");
    assert_eq!(root, text("5"));
    // The bare mark still reads as no tag at all, so the
    // scalar resolves as it resolves untagged.
    let bare = parse_yaml("%TAG ! tag:yaml.org,2002:\n---\n! 5\n").expect("the document reads");
    assert_eq!(bare, Value::Int(IntVal::from_decimal("5")));
}

#[test]
fn test_a_tag_directive_redefines_the_secondary_handle() {
    let restated =
        parse_yaml("%TAG !! tag:yaml.org,2002:\n---\n!!str 5\n").expect("the document reads");
    assert_eq!(restated, text("5"));
    let err = parse_yaml("%TAG !! tag:x/\n---\n!!str 5\n").expect_err("the document refuses");
    assert_eq!(err, "a tag the subset does not read stands in the document");
}

#[test]
fn test_a_tag_directive_stands_in_flow_and_on_a_key() {
    let flow = parse_yaml("%TAG !e! tag:yaml.org,2002:\n---\n{a: !e!str 5}\n")
        .expect("the document reads");
    assert_eq!(flow.get("a"), Some(&text("5")));
    let key = parse_yaml("%TAG !e! tag:yaml.org,2002:\n---\n? !e!str 42\n: v\n")
        .expect("the document reads");
    assert_eq!(key.get("42"), Some(&text("v")));
    let flow_key = parse_yaml("%TAG !e! tag:yaml.org,2002:\n---\n{!e!str 42: v}\n")
        .expect("the document reads");
    assert_eq!(flow_key.get("42"), Some(&text("v")));
}

#[test]
fn test_a_reserved_directive_is_skipped() {
    for doc in [
        "%FOO bar\n---\na: 1\n",
        "%FOO\n---\na: 1\n",
        "%yaml 1.2\n---\na: 1\n",
    ] {
        let read = parse_yaml(doc).expect("the document reads");
        assert_eq!(
            read.get("a"),
            Some(&Value::Int(IntVal::from_decimal("1"))),
            "document {doc:?}"
        );
    }
}

#[test]
fn test_directives_stand_after_comments_and_together() {
    let read = parse_yaml("# note\n\n%YAML 1.2\n%TAG !e! tag:yaml.org,2002:\n---\n!e!str hi\n")
        .expect("the document reads");
    assert_eq!(read, text("hi"));
    let reversed = parse_yaml("%TAG !e! tag:yaml.org,2002:\n%YAML 1.1\n---\n!e!int '7'\n")
        .expect("the document reads");
    assert_eq!(reversed, Value::Int(IntVal::from_decimal("7")));
}

#[test]
fn test_a_percent_decoded_tag_prefix_reads() {
    let read =
        parse_yaml("%TAG !e! tag:yaml.org%2C2002:\n---\n!e!str 5\n").expect("the document reads");
    assert_eq!(read, text("5"));
}

#[test]
fn test_a_malformed_directive_refuses() {
    for doc in [
        "%YAML\n---\na: 1\n",
        "%YAML x.y\n---\na: 1\n",
        "%YAML 1.2 extra\n---\na: 1\n",
        "%YAML 1.2.3\n---\na: 1\n",
        "%YAML\t1.2\n---\na: 1\n",
        "%\n---\na: 1\n",
        "%TAG !e!\n---\na: 1\n",
        "%TAG e! tag:x/\n---\na: 1\n",
        "%TAG !e! tag:x/ extra\n---\na: 1\n",
        "%TAG !e! tag:x%zz/\n---\na: 1\n",
    ] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(
            err, "a malformed directive stands in the document, which the subset does not carry",
            "document {doc:?}"
        );
    }
}

#[test]
fn test_an_incompatible_version_directive_refuses() {
    for doc in ["%YAML 2.0\n---\na: 1\n", "%YAML 0.9\n---\na: 1\n"] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(
            err, "a directive names a version the subset does not carry",
            "document {doc:?}"
        );
    }
}

#[test]
fn test_a_duplicate_version_directive_refuses() {
    for doc in [
        "%YAML 1.1\n%YAML 1.2\n---\na: 1\n",
        "%YAML 1.2\n%YAML 1.2\n---\na: 1\n",
    ] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(
            err,
            "the version directive stands twice in the document, which the subset does not carry",
            "document {doc:?}"
        );
    }
}

#[test]
fn test_a_duplicate_tag_handle_refuses() {
    for doc in [
        "%TAG !e! tag:x/\n%TAG !e! tag:y/\n---\na: 1\n",
        "%TAG !e! tag:x/\n%TAG !e! tag:x/\n---\na: 1\n",
        "%TAG !! tag:a/\n%TAG !! tag:b/\n---\na: 1\n",
    ] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(
            err, "a tag handle stands twice in the document, which the subset does not carry",
            "document {doc:?}"
        );
    }
}

#[test]
fn test_a_directive_with_no_document_start_refuses() {
    for doc in [
        "%YAML 1.2\na: 1\n",
        "%YAML 1.2\n",
        "%YAML 1.2\n...\n",
        "%TAG !e! tag:x/\na: 1\n",
    ] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(
            err,
            "a directive stands with no document start after it, which the subset does not carry",
            "document {doc:?}"
        );
    }
}

#[test]
fn test_a_tag_expanding_to_an_unread_tag_refuses() {
    let root = parse_yaml("%TAG !e! tag:example.com,2000:app/\n---\n!e!foo bar\n")
        .expect_err("the document refuses");
    assert_eq!(
        root,
        "a tag the subset does not read stands in the document"
    );
    let value =
        parse_yaml("%TAG !e! tag:x/\n---\na: !e!int '42'\n").expect_err("the document refuses");
    assert_eq!(
        value,
        "a tag the subset does not read stands in the document"
    );
}

#[test]
fn test_an_undeclared_tag_handle_refuses() {
    let block = parse_yaml("%TAG !f! tag:yaml.org,2002:\n---\n!e!str 5\n")
        .expect_err("the document refuses");
    assert_eq!(
        block,
        "a tag the subset does not read stands in the document"
    );
    let flow = parse_yaml("%TAG !f! tag:yaml.org,2002:\n---\n{a: !e!str 5}\n")
        .expect_err("the document refuses");
    assert_eq!(
        flow,
        "a tag the subset does not read stands in the document"
    );
}
