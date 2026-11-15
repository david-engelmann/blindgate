import json
import sys

import yaml

CASES = [
    ("anchor_on_a_scalar_reads_at_the_alias", "S20-2", "a: &x 5\nb: *x\n", None),
    ("anchor_on_a_block_mapping_reads_at_the_alias", "S20-2", "base: &m\n  k: 1\nb: *m\n", None),
    ("anchor_on_a_block_sequence_reads_at_the_alias", "S20-2", "a: &s\n  - 1\n  - 2\nb: *s\n", None),
    ("anchor_on_a_flow_collection_reads_at_the_alias", "S20-2", "a: &s [1, 2]\nb: *s\n", None),
    ("anchor_with_no_value_anchors_the_null", "S20-2", "a: &x\nb: *x\n", None),
    ("anchor_before_a_block_scalar_reads", "S20-2", "k: &a |\n  x\n", None),
    ("anchor_at_the_document_root_reads", "S20-2", "&root\nk: 1\n", None),
    ("an_anchor_stands_on_a_key", "S20-2", "&k name: 5\nb: *k\n", None),
    ("an_alias_reads_in_every_value_position", "S20-3", "a: &x 9\nb: [*x, *x]\nc:\n  - *x\n", None),
    ("an_alias_stands_as_a_key", "S20-3", "a: &k name\n*k : 5\n", None),
    ("an_anchor_defined_inside_a_structure_stands_for_the_document", "S20-3", "outer:\n  a: &x 1\nother:\n  c: *x\n", None),
    ("an_undefined_alias_refuses", "S20-3", "a: *nope\n", "python_error"),
    ("a_forward_alias_refuses", "S20-3", "a: *x\nb: &x 3\n", "python_error"),
    ("a_duplicate_anchor_refuses", "S20-3", "a: &x 1\nb:\n  c: &x 2\n", "python_error"),
    ("a_recursive_alias_refuses", "S20-3", "a: &r\n  self: *r\n", "port_refuses"),
    ("an_anchor_on_an_alias_refuses", "S20-3", "a: &x 7\nb: &y *x\n", "python_error"),
    ("two_anchors_on_one_value_refuse", "S20-3", "a: &x &y 5\n", "python_error"),
    ("a_merge_key_splices_the_anchored_mapping", "S20-4", "base: &b\n  x: 1\nderived:\n  <<: *b\n  y: 2\n", None),
    ("an_explicit_key_wins_over_a_merged_key_standing_after_it", "S20-4", "base: &b\n  x: 1\n  y: 2\nderived:\n  <<: *b\n  y: 3\n", None),
    ("an_explicit_key_wins_over_a_merged_key_standing_before_it", "S20-4", "base: &b\n  x: 1\n  y: 2\nderived:\n  y: 3\n  <<: *b\n", None),
    ("merged_pairs_stand_before_the_explicit_pairs", "S20-4", "base: &b\n  m: 1\n  a: 2\nd:\n  z: 9\n  <<: *b\n", None),
    ("a_merge_sequence_takes_the_first_source_for_a_shared_key", "S20-4", "b1: &b1\n  p: 1\n  q: 1\nb2: &b2\n  q: 2\n  r: 2\nd:\n  <<: [*b1, *b2]\n", None),
    ("two_merge_keys_in_one_mapping_both_merge", "S20-4", "b1: &b1\n  p: 1\n  q: 1\nb2: &b2\n  q: 2\n  r: 2\nd:\n  <<: *b1\n  <<: *b2\n", None),
    ("a_merge_of_a_direct_mapping_reads", "S20-4", "d:\n  <<:\n    x: 1\n  y: 2\n", None),
    ("a_merge_value_that_is_not_a_mapping_refuses", "S20-4", "base: &b 5\nd:\n  <<: *b\n", "python_error"),
    ("a_merge_sequence_holding_a_non_mapping_refuses", "S20-4", "b1: &b1\n  x: 1\nd:\n  <<: [*b1, 5]\n", "python_error"),
    ("a_quoted_merge_key_is_an_ordinary_key", "S20-4", "base: &b\n  x: 1\nd:\n  '<<': *b\n", None),
    ("a_merge_inside_an_anchored_mapping_composes_before_the_alias", "S20-4", "base: &b\n  x: 1\nd1: &d\n  <<: *b\n  y: 2\nd2: *d\n", None),
    ("a_merge_collision_follows_the_key_equality_family", "S20-4", "base: &b\n  1: merged\nd:\n  true: explicit\n  <<: *b\n", None),
    ("an_anchor_inside_a_merged_source_stands_for_the_document", "S20-4", "base: &b\n  inner: &i 3\n  x: 1\nd:\n  <<: *b\ne: *i\n", None),
]


def enc(value):
    if value is None:
        return {"t": "null"}
    if isinstance(value, bool):
        return {"t": "bool", "v": value}
    if isinstance(value, int):
        return {"t": "int", "v": str(value)}
    if isinstance(value, str):
        return {"t": "text", "v": value}
    if isinstance(value, list):
        return {"t": "list", "v": [enc(v) for v in value]}
    if isinstance(value, dict):
        return {"t": "map", "v": [[enc(k), enc(v)] for k, v in value.items()]}
    raise SystemExit("unencodable value: %r" % (value,))


cases = []
for name, source, doc, refusal in CASES:
    case = {"kind": "anchor", "name": name, "source": source, "input": doc}
    if refusal == "python_error":
        try:
            yaml.safe_load(doc)
        except Exception:
            pass
        else:
            raise SystemExit("case %s was expected to fail in PyYAML and did not" % name)
        case["python_error"] = "PyYAML refuses the document"
    elif refusal == "port_refuses":
        value = yaml.safe_load(doc)
        assert value["a"]["self"] is value["a"], (name, "the arbiter reads a self holding structure")
        case["port_refuses"] = True
        case["provenance"] = "PyYAML reads a structure that holds itself; the port refuses it under S20-3 as its stated limit"
    else:
        case["expected_value"] = enc(yaml.safe_load(doc))
    cases.append(case)

out = {
    "generated_by": "tools/gen_slice20.py in this repo, run against PyYAML 6.0.1; regenerate by running it again, never by editing recordings by hand",
    "cases": cases,
}
path = sys.argv[1]
with open(path, "w", encoding="utf-8") as f:
    json.dump(out, f, indent=1, sort_keys=True)
    f.write("\n")
print("wrote", len(cases), "cases to", path)
