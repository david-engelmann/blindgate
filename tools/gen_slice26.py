# Regenerate tests/conformance/slice26_cases.json.
# Run: python3 tools/gen_slice26.py tests/conformance/slice26_cases.json
# Requires PyYAML 6.0.1, the arbiter slice 26 records against.
# Each case names the slice 26 obligation it stands under in
# its source field. A case marked "python_error" is one
# PyYAML itself refuses, at its scanner or under its
# constructor for a tag naming no type it carries; the runner
# then asserts the port refuses it too. Every other case
# records the value PyYAML reads, and the runner asserts the
# port reads the same.
import datetime
import json
import sys

import yaml

CASES = [
    ("a_semicolon_suffix_refuses", "S26-2", "a: !!str;x 42\n", "python_error"),
    ("a_slash_suffix_refuses", "S26-2", "a: !!str/x 42\n", "python_error"),
    ("a_question_suffix_refuses", "S26-2", "a: !!str?x 42\n", "python_error"),
    ("a_colon_suffix_refuses", "S26-2", "a: !!str:x 42\n", "python_error"),
    ("an_at_suffix_refuses", "S26-2", "a: !!str@x 42\n", "python_error"),
    ("an_ampersand_suffix_refuses", "S26-2", "a: !!str&x 42\n", "python_error"),
    ("an_equals_suffix_refuses", "S26-2", "a: !!str=x 42\n", "python_error"),
    ("a_plus_suffix_refuses", "S26-2", "a: !!str+x 42\n", "python_error"),
    ("a_dollar_suffix_refuses", "S26-2", "a: !!str$x 42\n", "python_error"),
    ("a_comma_suffix_refuses", "S26-2", "a: !!str,x 42\n", "python_error"),
    ("a_dotted_suffix_refuses", "S26-3", "a: !!str.foo 42\n", "python_error"),
    ("a_tilde_suffix_refuses", "S26-2", "a: !!str~x 42\n", "python_error"),
    ("an_asterisk_suffix_refuses", "S26-2", "a: !!str*x 42\n", "python_error"),
    ("an_apostrophe_suffix_refuses", "S26-2", "a: !!str'x 42\n", "python_error"),
    ("an_open_paren_suffix_refuses", "S26-2", "a: !!str(x 42\n", "python_error"),
    ("a_close_paren_suffix_refuses", "S26-2", "a: !!str)x 42\n", "python_error"),
    ("an_open_bracket_suffix_refuses", "S26-2", "a: !!str[x 42\n", "python_error"),
    ("a_close_bracket_suffix_refuses", "S26-2", "a: !!str]x 42\n", "python_error"),
    ("a_hyphen_suffix_refuses_as_today", "S26-3", "a: !!str-x 42\n", "python_error"),
    ("an_underscore_suffix_refuses_as_today", "S26-3", "a: !!str_x 42\n", "python_error"),
    ("a_trailing_comma_suffix_refuses_in_flow", "S26-2", "[!!str, 42]\n", "python_error"),
    ("a_closing_bracket_suffix_refuses_in_flow", "S26-2", "[!!str]x 42]\n", "python_error"),
    ("a_colon_suffix_on_a_flow_key_refuses", "S26-2", "{!!str: 42}\n", "python_error"),
    ("a_named_handle_dotted_suffix_refuses", "S26-3", "%TAG !e! tag:yaml.org,2002:\n--- !e!str.x 42\n", "python_error"),
    ("a_dotted_tag_on_a_key_refuses", "S26-3", "? !!str.x k\n: v\n", "python_error"),
    ("an_escaped_dot_suffix_refuses_alike", "S26-4", "a: !!str%2Efoo 42\n", "python_error"),
    ("an_escaped_colon_suffix_refuses_alike", "S26-4", "a: !!str%3Ax 42\n", "python_error"),
    ("a_carried_spelling_reads", "S26-3", "a: !!str 42\n", None),
    ("a_carried_spelling_reads_in_flow", "S26-3", "{a: !!str 42}\n", None),
    ("a_named_handle_spelling_reads", "S26-3", "%TAG !e! tag:yaml.org,2002:\n--- !e!str 42\n", None),
    ("a_carried_spelling_reads_on_a_sequence_entry", "S26-3", "- !!str 42\n", None),
]


def enc(value):
    if value is None:
        return {"t": "null"}
    if isinstance(value, bool):
        return {"t": "bool", "v": value}
    if isinstance(value, int):
        return {"t": "int", "v": str(value)}
    if isinstance(value, float):
        return {"t": "float", "v": repr(value)}
    if isinstance(value, (datetime.date, datetime.datetime)):
        return {"t": "timestamp", "v": str(value)}
    if isinstance(value, str):
        return {"t": "text", "v": value}
    if isinstance(value, list):
        return {"t": "list", "v": [enc(v) for v in value]}
    if isinstance(value, dict):
        return {"t": "map", "v": [[enc(k), enc(v)] for k, v in value.items()]}
    raise SystemExit("unencodable value: %r" % (value,))


cases = []
for name, source, doc, refusal in CASES:
    case = {"kind": "tag_suffix_chars", "name": name, "source": source, "input": doc}
    if refusal == "python_error":
        try:
            yaml.safe_load(doc)
        except Exception:
            pass
        else:
            raise SystemExit("case %s was expected to fail in PyYAML and did not" % name)
        case["python_error"] = "PyYAML refuses the document"
        case["provenance"] = (
            "PyYAML refuses the document and the port refuses it in its own words "
            "under the slice 26 standings"
        )
    else:
        case["expected_value"] = enc(yaml.safe_load(doc))
    cases.append(case)

out = {
    "generated_by": "tools/gen_slice26.py in this repo, run against PyYAML 6.0.1; regenerate by running it again, never by editing recordings by hand",
    "cases": cases,
}
path = sys.argv[1]
with open(path, "w", encoding="utf-8") as f:
    json.dump(out, f, indent=1, sort_keys=True)
    f.write("\n")
print("wrote", len(cases), "cases to", path)
