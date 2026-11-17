# Regenerate tests/conformance/slice24_cases.json.
# Run: python3 tools/gen_slice24.py tests/conformance/slice24_cases.json
# Requires PyYAML 6.0.1, the arbiter slice 24 records against.
# Each case names the slice 24 obligation it stands under in
# its source field. A case marked "python_error" is one
# PyYAML itself refuses; the runner then asserts the port
# refuses it too. Every other case records the value PyYAML
# reads, and the runner asserts the port reads the same.
import datetime
import json
import sys

import yaml

CASES = [
    ("a_block_sequence_on_the_value_line", "S24-3", "? k\n: - a\n  - b\n", None),
    ("a_single_entry_block_sequence_on_the_value_line", "S24-3", "? k\n: - a\n", None),
    ("a_dash_alone_on_the_value_line", "S24-3", "? k\n: -\n", None),
    ("a_nested_block_sequence_on_the_value_line", "S24-3", "? k\n: - - a\n", None),
    ("a_block_sequence_on_the_value_line_before_a_sibling", "S24-3", "? k\n: - a\n  - b\nnext: 1\n", None),
    ("a_block_mapping_on_the_value_line", "S24-3", "? k\n: a: b\n  c: d\n", None),
    ("a_block_mapping_on_the_value_line_before_a_sibling", "S24-3", "? k\n: a: b\nnext: 1\n", None),
    ("an_empty_value_in_the_block_mapping", "S24-3", "? k\n: a:\n", None),
    ("a_quoted_first_key_on_the_value_line", "S24-3", "? k\n: 'q key': b\n", None),
    ("a_nested_value_in_the_block_mapping", "S24-3", "? k\n: a:\n    b: c\n", None),
    ("a_further_explicit_entry_on_the_value_line", "S24-3", "? k\n: ? n\n  : v\n", None),
    ("a_further_explicit_entry_without_a_value_line", "S24-3", "? k\n: ? n\n", None),
    ("a_question_alone_on_the_value_line", "S24-3", "? k\n: ?\n", None),
    ("an_explicit_chain_across_value_lines", "S24-3", "? a\n: ? b\n  : ? c\n    : v\n", None),
    ("a_block_sequence_in_a_nested_mapping", "S24-2", "outer:\n  ? k\n  : - a\n    - b\n", None),
    ("a_block_mapping_in_a_sequence_entry", "S24-2", "- ? k\n  : a: b\n", None),
    ("a_block_sequence_in_a_sequence_entry", "S24-2", "- ? k\n  : - a\n    - b\n- z\n", None),
    ("a_merge_through_a_block_mapping_on_the_value_line", "S24-3", "? <<\n: a: b\nc: 1\n", None),
    ("a_merge_through_a_block_sequence_on_the_value_line", "S24-3", "? <<\n: - a: b\n    c: d\ne: 1\n", None),
    ("a_typed_first_key_on_the_value_line", "S24-3", "? k\n: !!int 5: b\n", None),
    ("a_spelled_first_key_on_the_value_line", "S24-3", "? k\n: !!str a: b\n", None),
    ("an_anchored_first_key_on_the_value_line", "S24-3", "? k\n: &x a: b\nj: *x\n", None),
    ("a_block_sequence_on_the_nested_entry_value_line", "S24-3", "? k\n: ? n\n  : - a\n    - b\n", None),
    ("a_block_mapping_on_the_nested_entry_value_line", "S24-3", "? k\n: ? n\n  : a: b\n", None),
    ("a_block_scalar_key_with_a_compact_value", "S24-2", "? |-\n  kk\n: - a\n", None),
    ("a_directive_before_a_compact_value", "S24-2", "%YAML 1.2\n---\n? k\n: a: b\n", None),
    ("a_flow_value_on_the_value_line_still_reads", "S24-6", "? k\n: {a: b}\n", None),
    ("a_value_on_the_following_lines_still_reads", "S24-6", "? k\n:\n  a: b\n", None),
    ("a_collection_as_the_nested_explicit_key", "S24-5", "? k\n: ? [a, b]\n  : v\n", "python_error"),
    ("a_block_mapping_as_the_nested_explicit_key", "S24-5", "? k\n: ?\n    a: b\n  : v\n", "python_error"),
    ("a_one_line_nested_explicit_entry", "S24-5", "? k\n: ? n : v\n", "python_error"),
    ("an_anchor_before_a_block_sequence_on_the_value_line", "S24-4", "? k\n: &x - a\n  - b\n", "python_error"),
    ("a_tag_before_a_block_sequence_on_the_value_line", "S24-4", "? k\n: !!seq - a\n  - b\n", "python_error"),
    ("an_anchor_before_a_further_explicit_entry", "S24-4", "? k\n: &x ? n\n  : v\n", "python_error"),
    ("a_tag_before_a_further_explicit_entry", "S24-4", "? k\n: !!str ? n\n  : v\n", "python_error"),
    ("a_collection_tag_before_a_block_mapping", "S24-4", "? k\n: !!map a: b\n", "python_error"),
    ("block_sequence_entries_at_the_entry_indentation", "S24-4", "? k\n: - a\n- b\n", "python_error"),
    ("a_double_colon_on_the_value_line", "S24-4", "? k\n: a: b: c\n", "python_error"),
    ("a_compact_sequence_inside_the_first_entry", "S24-4", "? k\n: a: - b\n", "python_error"),
    ("a_compact_entry_inside_the_first_entry", "S24-4", "? k\n: a: ? n\n", "python_error"),
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
    case = {"kind": "compact_value", "name": name, "source": source, "input": doc}
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
            "under the slice 24 standings"
        )
    else:
        case["expected_value"] = enc(yaml.safe_load(doc))
    cases.append(case)

out = {
    "generated_by": "tools/gen_slice24.py in this repo, run against PyYAML 6.0.1; regenerate by running it again, never by editing recordings by hand",
    "cases": cases,
}
path = sys.argv[1]
with open(path, "w", encoding="utf-8") as f:
    json.dump(out, f, indent=1, sort_keys=True)
    f.write("\n")
print("wrote", len(cases), "cases to", path)
