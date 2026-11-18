//! The slice 26 pins the contract names: a shorthand tag
//! spelling's scan carries the characters S26-2 states, a
//! widened spelling refuses whole under S26-3, the plain
//! and escaped roads to one name refuse alike under
//! S26-4, and the limits of S26-5 hold.

use blindgate::{parse_yaml, IntVal, Value};

fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

fn int(value: &str) -> Value {
    Value::Int(IntVal::from_decimal(value))
}

const UNREAD: &str = "a tag the subset does not read stands in the document";

#[test]
fn test_a_dotted_suffix_refuses_whole() {
    let err = parse_yaml("a: !!str.foo 42\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_a_semicolon_suffix_refuses() {
    let err = parse_yaml("a: !!str;x 42\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_a_slash_suffix_refuses() {
    let err = parse_yaml("a: !!str/x 42\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_a_colon_suffix_refuses() {
    let err = parse_yaml("a: !!str:x 42\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_an_ampersand_suffix_refuses() {
    let err = parse_yaml("a: !!str&x 42\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_a_tilde_suffix_refuses() {
    let err = parse_yaml("a: !!str~x 42\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_an_apostrophe_suffix_refuses() {
    let err = parse_yaml("a: !!str'x 42\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_a_question_suffix_refuses_in_flow() {
    let err = parse_yaml("{a: !!str?x 42}\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_a_trailing_comma_suffix_refuses_in_flow() {
    let err = parse_yaml("[!!str, 42]\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_a_closing_bracket_reads_into_the_suffix_in_flow() {
    let err = parse_yaml("[!!str]x 42]\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_a_named_handle_dotted_suffix_refuses() {
    let err = parse_yaml("%TAG !e! tag:yaml.org,2002:\n--- !e!str.x 42\n")
        .expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_a_dotted_tag_on_a_key_refuses() {
    let err = parse_yaml("? !!str.x k\n: v\n").expect_err("the document refuses");
    assert_eq!(err, UNREAD);
}

#[test]
fn test_a_plain_character_and_its_escape_refuse_alike() {
    for doc in ["a: !!str.foo 42\n", "a: !!str%2Efoo 42\n"] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(err, UNREAD, "document {doc:?}");
    }
}

#[test]
fn test_a_carried_spelling_still_reads() {
    let doc = parse_yaml("a: !!str 42\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&text("42")));
    let doc = parse_yaml("{a: !!int 5}\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&int("5")));
}

#[test]
fn test_a_hyphen_underscore_suffix_still_refuses_as_today() {
    for doc in ["a: !!str-x 42\n", "a: !!str_x 42\n"] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(err, UNREAD, "document {doc:?}");
    }
}
