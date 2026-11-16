//! The slice 22 pins the contract names: explicit keys
//! read as S22-2 and S22-3 state, and the refusal wording
//! of S22-5 stands as stated.

use blindgate::{parse_yaml, IntVal, Value};

fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

#[test]
fn test_an_explicit_key_reads_a_block_entry() {
    let doc = parse_yaml("? key\n: value\n").expect("the document reads");
    assert_eq!(doc.get("key"), Some(&text("value")));
    let second = parse_yaml("a: 1\n? b\n: 2\n").expect("the document reads");
    assert_eq!(
        second.get("b"),
        Some(&Value::Int(IntVal::from_decimal("2")))
    );
}

#[test]
fn test_a_quoted_explicit_key_reads_its_text() {
    let doc = parse_yaml("? 'quoted key'\n: value\n").expect("the document reads");
    assert_eq!(doc.get("quoted key"), Some(&text("value")));
    let folded = parse_yaml("? 'a\n  b'\n: value\n").expect("the document reads");
    assert_eq!(folded.get("a b"), Some(&text("value")));
}

#[test]
fn test_a_typed_explicit_key_resolves_as_an_implicit_key() {
    let int_key = parse_yaml("? 1\n: a\n").expect("the document reads");
    assert_eq!(
        int_key,
        Value::Map(vec![(Value::Int(IntVal::from_decimal("1")), text("a"))])
    );
    let bool_key = parse_yaml("? yes\n: b\n").expect("the document reads");
    assert_eq!(bool_key, Value::Map(vec![(Value::Bool(true), text("b"))]));
    // The two keys collide exactly as they collide in
    // their implicit spelling, the arbiter's dictionary
    // holding one entry whose value is the later one.
    let both = parse_yaml("? 1\n: a\n? yes\n: b\n").expect("the document reads");
    assert_eq!(
        both,
        Value::Map(vec![(Value::Int(IntVal::from_decimal("1")), text("b"))])
    );
}

#[test]
fn test_an_explicit_key_with_no_value_reads_as_null() {
    let doc = parse_yaml("? key\n").expect("the document reads");
    assert_eq!(doc.get("key"), Some(&Value::Null));
    let followed = parse_yaml("? a\nb: 1\n").expect("the document reads");
    assert_eq!(followed.get("a"), Some(&Value::Null));
    assert_eq!(
        followed.get("b"),
        Some(&Value::Int(IntVal::from_decimal("1")))
    );
}

#[test]
fn test_an_empty_explicit_key_reads_as_null() {
    let doc = parse_yaml("?\n: value\n").expect("the document reads");
    assert_eq!(doc, Value::Map(vec![(Value::Null, text("value"))]));
}

#[test]
fn test_an_explicit_key_reads_in_a_nested_mapping_and_a_sequence_entry() {
    let nested = parse_yaml("outer:\n  ? k\n  : v\n").expect("the document reads");
    assert_eq!(
        nested.get("outer").and_then(|inner| inner.get("k")),
        Some(&text("v"))
    );
    let in_sequence = parse_yaml("- ? a\n  : 1\n- b\n").expect("the document reads");
    assert_eq!(
        in_sequence,
        Value::List(vec![
            Value::Map(vec![(text("a"), Value::Int(IntVal::from_decimal("1")))]),
            text("b"),
        ])
    );
}

#[test]
fn test_an_explicit_key_reads_in_a_flow_mapping() {
    let doc = parse_yaml("{? a : 1, b: 2}\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&Value::Int(IntVal::from_decimal("1"))));
    assert_eq!(doc.get("b"), Some(&Value::Int(IntVal::from_decimal("2"))));
    let abutting = parse_yaml("{?a: 1}\n").expect("the document reads");
    assert_eq!(
        abutting.get("a"),
        Some(&Value::Int(IntVal::from_decimal("1")))
    );
    let no_value = parse_yaml("{? a, b: 2}\n").expect("the document reads");
    assert_eq!(no_value.get("a"), Some(&Value::Null));
    let pair = parse_yaml("[? a, b]\n").expect("the document reads");
    assert_eq!(
        pair,
        Value::List(vec![Value::Map(vec![(text("a"), Value::Null)]), text("b"),])
    );
}

#[test]
fn test_a_block_scalar_explicit_key_reads_its_text() {
    let doc = parse_yaml("? |\n  text\n: value\n").expect("the document reads");
    assert_eq!(doc.get("text\n"), Some(&text("value")));
}

#[test]
fn test_an_explicit_merge_key_merges() {
    let doc = parse_yaml("base: &b\n  x: 1\nfoo:\n  ? <<\n  : *b\n").expect("the document reads");
    let foo = doc.get("foo").expect("the merged mapping stands");
    assert_eq!(foo.get("x"), Some(&Value::Int(IntVal::from_decimal("1"))));
}

#[test]
fn test_an_anchored_and_tagged_explicit_key_reads() {
    let anchored = parse_yaml("? &a key\n: value\nother: *a\n").expect("the document reads");
    assert_eq!(anchored.get("key"), Some(&text("value")));
    assert_eq!(anchored.get("other"), Some(&text("key")));
    let tagged = parse_yaml("? !!str 42\n: value\n").expect("the document reads");
    assert_eq!(tagged.get("42"), Some(&text("value")));
}

#[test]
fn test_a_collection_explicit_key_refuses() {
    for doc in ["? [a, b]\n: value\n", "? {a: 1}\n: value\n"] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(
            err,
            "a collection stands as an explicit key, which the subset does not carry"
        );
    }
}

#[test]
fn test_a_one_line_explicit_entry_refuses() {
    let err = parse_yaml("? key : value\n").expect_err("the document refuses");
    assert_eq!(
        err,
        "a collection stands as an explicit key, which the subset does not carry"
    );
}
