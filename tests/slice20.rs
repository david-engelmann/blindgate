//! The slice 20 pins the contract names outside the
//! recorded battery: the refusal wordings of S20-5 stand
//! as stated, and a document with no anchor reads as it
//! read before the slice.

use blindgate::{parse_yaml, IntVal, Value};

#[test]
fn test_the_slice20_refusal_wordings_stand_as_stated() {
    let cases: [(&str, &str); 9] = [
        (
            "a: &x 1\nb: &x 2\n",
            "the same anchor stands twice in the document",
        ),
        ("a: *x\n", "an alias names no anchor in the document"),
        (
            "a: &r\n  self: *r\n",
            "an alias names the anchor that holds it, which the subset does not carry",
        ),
        (
            "base: &b 5\nd:\n  <<: *b\n",
            "a merge key's value is not a mapping, which the subset does not carry",
        ),
        (
            "a: &x &y 5\n",
            "two anchors stand on one value, which the subset does not carry",
        ),
        (
            "a: &x 1\nb: &y *x\n",
            "an anchor stands on an alias, which the subset does not carry",
        ),
        (
            "a: & 5\n",
            "an anchor or alias stands with no name, which the subset does not carry",
        ),
        (
            "a: &a.b 5\n",
            "an anchor or alias name carries a character the subset does not read",
        ),
        (
            "a: &m {x: 1}\n*m : 5\n",
            "an alias names a collection standing as a key, which the subset does not carry",
        ),
    ];
    for (text, wording) in cases {
        assert_eq!(parse_yaml(text).unwrap_err(), wording, "document {text:?}");
    }
}

#[test]
fn test_a_document_with_no_anchor_reads_as_before() {
    let value = parse_yaml("a: 1\nb: [x, y]\nc:\n  d: true\n").expect("the document reads");
    assert_eq!(
        value,
        Value::Map(vec![
            (
                Value::Text("a".to_string()),
                Value::Int(IntVal::from_decimal("1"))
            ),
            (
                Value::Text("b".to_string()),
                Value::List(vec![
                    Value::Text("x".to_string()),
                    Value::Text("y".to_string())
                ])
            ),
            (
                Value::Text("c".to_string()),
                Value::Map(vec![(Value::Text("d".to_string()), Value::Bool(true))])
            ),
        ])
    );
}
