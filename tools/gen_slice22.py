import datetime
import json
import sys

import yaml

CASES = [
    ("explicit_key_over_a_plain_scalar", "S22-2", "? key\n: value\n", None),
    ("explicit_key_over_a_quoted_scalar", "S22-3", "? 'quoted key'\n: value\n", None),
    ("explicit_key_over_an_int_spelling", "S22-3", "? 1\n: a\n", None),
    ("explicit_key_over_a_bool_spelling", "S22-3", "? yes\n: a\n", None),
    ("explicit_key_with_no_value", "S22-2", "? key\n", None),
    ("empty_explicit_key", "S22-3", "?\n: value\n", None),
    ("explicit_key_in_a_nested_mapping", "S22-2", "outer:\n  ? k\n  : v\n", None),
    ("explicit_key_in_a_sequence_entry_mapping", "S22-2", "- ? a\n  : 1\n- b\n", None),
    ("explicit_key_in_a_flow_mapping", "S22-2", "{? a : 1, b: 2}\n", None),
    ("explicit_key_abutting_in_flow", "S22-2", "{?a: 1}\n", None),
    ("explicit_key_in_a_flow_sequence_pair", "S22-2", "[? a, b]\n", None),
    ("block_scalar_explicit_key", "S22-3", "? |\n  text\n: value\n", None),
    ("folded_plain_explicit_key", "S22-3", "? this is a long\n  plain key\n: value\n", None),
    ("explicit_key_with_a_block_sequence_value", "S22-2", "? key\n: \n  - 1\n  - 2\n", None),
    ("explicit_merge_key", "S22-3", "base: &b\n  x: 1\nfoo:\n  ? <<\n  : *b\n", None),
    ("anchored_explicit_key", "S22-2", "? &a key\n: value\nother: *a\n", None),
    ("tagged_explicit_key", "S22-3", "? !!str 42\n: value\n", None),
    ("explicit_key_after_a_plain_entry", "S22-2", "a: 1\n? b\n: 2\n", None),
    ("duplicate_explicit_keys_collapse", "S22-3", "? a\n: 1\n? a\n: 2\n", None),
    ("explicit_key_with_a_comment", "S22-2", "? key # note\n: value\n", None),
    ("a_sequence_explicit_key", "S22-4", "? [a, b]\n: value\n", "python_error"),
    ("a_mapping_explicit_key", "S22-4", "? {a: 1}\n: value\n", "python_error"),
    ("a_one_line_explicit_entry", "S22-4", "? key : value\n", "python_error"),
    ("a_flow_sequence_explicit_key", "S22-4", "{? [x] : 1}\n", "python_error"),
    ("an_explicit_entry_in_place_of_a_value", "S22-5", "key: ? a\n  : 1\n", "python_error"),
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
    case = {"kind": "explicit_key", "name": name, "source": source, "input": doc}
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
            "under the slice 22 standings"
        )
    else:
        case["expected_value"] = enc(yaml.safe_load(doc))
    cases.append(case)

out = {
    "generated_by": "tools/gen_slice22.py in this repo, run against PyYAML 6.0.1; regenerate by running it again, never by editing recordings by hand",
    "cases": cases,
}
path = sys.argv[1]
with open(path, "w", encoding="utf-8") as f:
    json.dump(out, f, indent=1, sort_keys=True)
    f.write("\n")
print("wrote", len(cases), "cases to", path)
