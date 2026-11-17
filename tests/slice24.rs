//! The slice 24 pins the contract names: a value opening
//! in compact form on an explicit entry's value line reads
//! as S24-2 and S24-3 state, and the standings of S24-4
//! and S24-5 refuse as stated.

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

#[test]
fn test_a_block_sequence_on_the_value_line_reads() {
    let doc = parse_yaml("? k\n: - a\n  - b\n").expect("the document reads");
    assert_eq!(
        doc.get("k"),
        Some(&Value::List(vec![text("a"), text("b")]))
    );
    let single = parse_yaml("? k\n: - a\n").expect("the document reads");
    assert_eq!(single.get("k"), Some(&Value::List(vec![text("a")])));
    let empty = parse_yaml("? k\n: -\n").expect("the document reads");
    assert_eq!(empty.get("k"), Some(&Value::List(vec![Value::Null])));
    let nested = parse_yaml("? k\n: - - a\n").expect("the document reads");
    assert_eq!(
        nested.get("k"),
        Some(&Value::List(vec![Value::List(vec![text("a")])]))
    );
    let sibling = parse_yaml("? k\n: - a\n  - b\nnext: 1\n").expect("the document reads");
    assert_eq!(
        sibling.get("k"),
        Some(&Value::List(vec![text("a"), text("b")]))
    );
    assert_eq!(sibling.get("next"), Some(&int("1")));
}

#[test]
fn test_a_block_mapping_on_the_value_line_reads() {
    let doc = parse_yaml("? k\n: a: b\n  c: d\n").expect("the document reads");
    assert_eq!(
        doc.get("k"),
        Some(&map(vec![(text("a"), text("b")), (text("c"), text("d"))]))
    );
    let sibling = parse_yaml("? k\n: a: b\nnext: 1\n").expect("the document reads");
    assert_eq!(
        sibling.get("k"),
        Some(&map(vec![(text("a"), text("b"))]))
    );
    assert_eq!(sibling.get("next"), Some(&int("1")));
    let empty_value = parse_yaml("? k\n: a:\n").expect("the document reads");
    assert_eq!(
        empty_value.get("k"),
        Some(&map(vec![(text("a"), Value::Null)]))
    );
    let quoted = parse_yaml("? k\n: 'q key': b\n").expect("the document reads");
    assert_eq!(
        quoted.get("k"),
        Some(&map(vec![(text("q key"), text("b"))]))
    );
    let deeper = parse_yaml("? k\n: a:\n    b: c\n").expect("the document reads");
    assert_eq!(
        deeper.get("k"),
        Some(&map(vec![(text("a"), map(vec![(text("b"), text("c"))]))]))
    );
}

#[test]
fn test_a_further_explicit_entry_on_the_value_line_reads() {
    let doc = parse_yaml("? k\n: ? n\n  : v\n").expect("the document reads");
    assert_eq!(
        doc.get("k"),
        Some(&map(vec![(text("n"), text("v"))]))
    );
    let no_value = parse_yaml("? k\n: ? n\n").expect("the document reads");
    assert_eq!(
        no_value.get("k"),
        Some(&map(vec![(text("n"), Value::Null)]))
    );
    let empty_key = parse_yaml("? k\n: ?\n").expect("the document reads");
    assert_eq!(
        empty_key.get("k"),
        Some(&map(vec![(Value::Null, Value::Null)]))
    );
}

#[test]
fn test_an_explicit_chain_across_value_lines_reads() {
    let doc = parse_yaml("? a\n: ? b\n  : ? c\n    : v\n").expect("the document reads");
    assert_eq!(
        doc.get("a"),
        Some(&map(vec![(
            text("b"),
            map(vec![(text("c"), text("v"))])
        )]))
    );
}

#[test]
fn test_a_compact_value_reads_in_a_nested_mapping_and_a_sequence_entry() {
    let nested = parse_yaml("outer:\n  ? k\n  : - a\n    - b\n").expect("the document reads");
    assert_eq!(
        nested.get("outer").and_then(|outer| outer.get("k")),
        Some(&Value::List(vec![text("a"), text("b")]))
    );
    let entry = parse_yaml("- ? k\n  : a: b\n").expect("the document reads");
    assert_eq!(
        entry,
        Value::List(vec![map(vec![(text("k"), map(vec![(text("a"), text("b"))]))])])
    );
    let entry_seq = parse_yaml("- ? k\n  : - a\n    - b\n- z\n").expect("the document reads");
    assert_eq!(
        entry_seq,
        Value::List(vec![
            map(vec![(text("k"), Value::List(vec![text("a"), text("b")]))]),
            text("z"),
        ])
    );
}

#[test]
fn test_an_explicit_merge_key_reads_a_block_mapping_on_the_value_line() {
    let doc = parse_yaml("? <<\n: a: b\nc: 1\n").expect("the document reads");
    assert_eq!(doc.get("a"), Some(&text("b")));
    assert_eq!(doc.get("c"), Some(&int("1")));
    assert_eq!(doc.as_map().map(|entries| entries.len()), Some(2));
    let through_seq = parse_yaml("? <<\n: - a: b\n    c: d\ne: 1\n").expect("the document reads");
    assert_eq!(through_seq.get("a"), Some(&text("b")));
    assert_eq!(through_seq.get("c"), Some(&text("d")));
    assert_eq!(through_seq.get("e"), Some(&int("1")));
}

#[test]
fn test_a_block_mapping_on_the_value_line_reads_first_key_properties() {
    let typed = parse_yaml("? k\n: !!int 5: b\n").expect("the document reads");
    assert_eq!(
        typed.get("k"),
        Some(&map(vec![(int("5"), text("b"))]))
    );
    let spelled = parse_yaml("? k\n: !!str a: b\n").expect("the document reads");
    assert_eq!(
        spelled.get("k"),
        Some(&map(vec![(text("a"), text("b"))]))
    );
    let anchored = parse_yaml("? k\n: &x a: b\nj: *x\n").expect("the document reads");
    assert_eq!(
        anchored.get("k"),
        Some(&map(vec![(text("a"), text("b"))]))
    );
    // The anchor stands on the opening's first key, so the
    // alias resolves to the key it names.
    assert_eq!(anchored.get("j"), Some(&text("a")));
}

#[test]
fn test_the_nested_entry_reads_a_block_value_on_its_own_value_line() {
    let seq = parse_yaml("? k\n: ? n\n  : - a\n    - b\n").expect("the document reads");
    assert_eq!(
        seq.get("k"),
        Some(&map(vec![(
            text("n"),
            Value::List(vec![text("a"), text("b")])
        )]))
    );
    let mapping = parse_yaml("? k\n: ? n\n  : a: b\n").expect("the document reads");
    assert_eq!(
        mapping.get("k"),
        Some(&map(vec![(
            text("n"),
            map(vec![(text("a"), text("b"))])
        )]))
    );
}

#[test]
fn test_a_collection_as_the_nested_explicit_key_refuses() {
    for doc in [
        "? k\n: ? [a, b]\n  : v\n",
        "? k\n: ?\n    a: b\n  : v\n",
    ] {
        let err = parse_yaml(doc).expect_err("the document refuses");
        assert_eq!(
            err, "a collection stands as an explicit key, which the subset does not carry",
            "document {doc:?}"
        );
    }
}

#[test]
fn test_a_one_line_nested_explicit_entry_refuses() {
    let err = parse_yaml("? k\n: ? n : v\n").expect_err("the document refuses");
    assert_eq!(
        err, "a collection stands as an explicit key, which the subset does not carry"
    );
}

#[test]
fn test_an_anchor_before_a_block_sequence_on_the_value_line_refuses() {
    assert!(parse_yaml("? k\n: &x - a\n  - b\n").is_err());
}

#[test]
fn test_a_tag_before_a_block_sequence_on_the_value_line_refuses() {
    assert!(parse_yaml("? k\n: !!seq - a\n  - b\n").is_err());
}

#[test]
fn test_an_anchor_before_a_further_explicit_entry_refuses() {
    assert!(parse_yaml("? k\n: &x ? n\n  : v\n").is_err());
}

#[test]
fn test_block_sequence_entries_at_the_entry_indentation_refuse() {
    assert!(parse_yaml("? k\n: - a\n- b\n").is_err());
}

#[test]
fn test_a_double_colon_on_the_value_line_refuses() {
    assert!(parse_yaml("? k\n: a: b: c\n").is_err());
}
