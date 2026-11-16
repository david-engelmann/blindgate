//! The slice 21 pins the contract names: the carried
//! tags read as S21-3 states, and the refusal wordings
//! of S21-5 stand as stated.

use blindgate::{parse_yaml, IntVal, Value};

fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

#[test]
fn test_a_str_tag_reads_an_int_spelling_as_text() {
    let doc = parse_yaml("a: !!str 42\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&text("42")));
    let root = parse_yaml("!!str 42\n").expect("the document reads");
    assert_eq!(root, text("42"));
}

#[test]
fn test_an_int_tag_reads_a_quoted_number() {
    let doc = parse_yaml("a: !!int '42'\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&Value::Int(IntVal::from_decimal("42"))));
    let hex = parse_yaml("a: !!int 0x2A\n").expect("the document reads");
    assert_eq!(hex.get("a"), Some(&Value::Int(IntVal::from_decimal("42"))));
}

#[test]
fn test_a_float_tag_reads_a_bare_exponent() {
    let doc = parse_yaml("a: !!float 1e3\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&Value::Float(1000.0)));
    let signed = parse_yaml("a: !!float -1e3\n").expect("the document reads");
    assert_eq!(signed.get("a"), Some(&Value::Float(-1000.0)));
}

#[test]
fn test_a_bool_tag_reads_the_bool_spellings() {
    let doc = parse_yaml("a: !!bool yes\nb: !!bool OFF\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&Value::Bool(true)));
    assert_eq!(doc.get("b"), Some(&Value::Bool(false)));
}

#[test]
fn test_a_null_tag_reads_any_scalar_as_null() {
    let doc = parse_yaml("a: !!null x\nb: !!null ''\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&Value::Null));
    assert_eq!(doc.get("b"), Some(&Value::Null));
}

#[test]
fn test_a_timestamp_tag_reads_a_date() {
    let doc = parse_yaml("a: !!timestamp 2024-01-02\n").expect("the document reads");
    assert_eq!(
        doc.get("a"),
        Some(&Value::Timestamp("2024-01-02".to_string()))
    );
}

#[test]
fn test_a_seq_tag_and_a_map_tag_read_their_collections() {
    let doc = parse_yaml("a: !!seq [1, 2]\nb: !!map\n  c: 1\n").expect("the document reads");
    assert_eq!(
        doc.get("a"),
        Some(&Value::List(vec![
            Value::Int(IntVal::from_decimal("1")),
            Value::Int(IntVal::from_decimal("2")),
        ]))
    );
    assert_eq!(
        doc.get("b"),
        Some(&Value::Map(vec![(
            text("c"),
            Value::Int(IntVal::from_decimal("1")),
        )]))
    );
}

#[test]
fn test_a_tag_stands_on_a_key_and_in_flow_and_before_a_block_scalar() {
    let keyed = parse_yaml("!!str 42: v\n").expect("the document reads");
    assert_eq!(keyed.get("42"), Some(&text("v")));
    let flow = parse_yaml("{a: !!str 42}\n").expect("the document reads");
    assert_eq!(flow.get("a"), Some(&text("42")));
    let blocked = parse_yaml("a: !!str |\n  42\n").expect("the document reads");
    assert_eq!(blocked.get("a"), Some(&text("42\n")));
}

#[test]
fn test_a_tag_and_an_anchor_stand_together_in_either_order() {
    let first = parse_yaml("a: &x !!str 42\nb: *x\n").expect("the document reads");
    assert_eq!(first.get("b"), Some(&text("42")));
    let second = parse_yaml("a: !!str &x 42\nb: *x\n").expect("the document reads");
    assert_eq!(second.get("b"), Some(&text("42")));
}

#[test]
fn test_an_unread_tag_refuses() {
    for text in [
        "a: !!unknown x\n",
        "a: !custom x\n",
        "a: !!binary aGk=\n",
        "a: !!set {x: null}\n",
        "a: !!omap [x: 1]\n",
        "a: !<tag:yaml.org,2002:unknown> x\n",
    ] {
        assert_eq!(
            parse_yaml(text).unwrap_err(),
            "a tag the subset does not read stands in the document",
            "document {text:?}"
        );
    }
}

#[test]
fn test_a_tag_on_a_value_of_another_kind_refuses() {
    for text in [
        "a: !!str [1]\n",
        "a: !!seq hello\n",
        "a: !!null [1]\n",
        "a: !!map [1]\n",
    ] {
        assert_eq!(
            parse_yaml(text).unwrap_err(),
            "a tag stands on a value of another kind, which the subset does not carry",
            "document {text:?}"
        );
    }
}

#[test]
fn test_a_tagged_scalar_that_does_not_resolve_refuses() {
    for text in [
        "a: !!int hello\n",
        "a: !!bool maybe\n",
        "a: !!timestamp hello\n",
    ] {
        assert_eq!(
            parse_yaml(text).unwrap_err(),
            "a tagged scalar does not resolve as its tag names, which the subset does not carry",
            "document {text:?}"
        );
    }
}

#[test]
fn test_two_tags_and_a_tag_on_an_alias_refuse() {
    assert_eq!(
        parse_yaml("a: !!str !!int 5\n").unwrap_err(),
        "two tags stand on one value, which the subset does not carry"
    );
    assert_eq!(
        parse_yaml("a: &x 42\nb: !!str *x\n").unwrap_err(),
        "a tag stands on an alias, which the subset does not carry"
    );
}
