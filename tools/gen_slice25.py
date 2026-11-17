# Regenerate tests/conformance/slice25_cases.json.
# Run: python3 tools/gen_slice25.py tests/conformance/slice25_cases.json
# Requires PyYAML 6.0.1, the arbiter slice 25 records against.
# Each case names the slice 25 obligation it stands under in
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
    ("an_escaped_str_suffix_reads", "S25-3", "a: !!%73tr 42\n", None),
    ("an_escaped_int_suffix_reads", "S25-3", "a: !!in%74 '42'\n", None),
    ("a_wholly_escaped_suffix_reads", "S25-3", "a: !!%73%74%72 42\n", None),
    ("lowercase_hex_escapes_read", "S25-3", "a: !!%69%6et 42\n", None),
    ("an_escaped_bool_suffix_reads", "S25-3", "a: !!%62ool true\n", None),
    ("an_escaped_null_suffix_reads", "S25-3", "a: !!nu%6Cl ~\n", None),
    ("an_escaped_float_suffix_reads", "S25-3", "a: !!flo%61t 1.5\n", None),
    ("an_escaped_seq_suffix_reads", "S25-3", "a: !!%73eq [1, 2]\n", None),
    ("a_verbatim_escaped_carried_name_reads", "S25-3", "a: !<tag:yaml.org,2002:%73tr> 42\n", None),
    ("a_verbatim_escape_in_the_prefix_reads", "S25-3", "a: !<%74ag:yaml.org,2002:str> 42\n", None),
    ("a_named_handle_suffix_escape_reads", "S25-3", "%TAG !e! tag:yaml.org,2002:\n--- !e!%73tr 42\n", None),
    ("an_escaped_tag_on_an_explicit_key_reads", "S25-3", "? !!%73tr k\n: v\n", None),
    ("an_escaped_tag_in_flow_reads", "S25-3", "{a: !!%69nt 5}\n", None),
    ("an_escaped_tag_on_a_sequence_entry_reads", "S25-3", "- !!%73tr 42\n", None),
    ("an_escaped_tag_after_an_anchor_reads", "S25-3", "a: &x !!%73tr 42\n", None),
    ("a_percent_in_a_plain_scalar_stays_literal", "S25-5", "a: 100%25\n", None),
    ("a_declared_handle_spelling_still_reads", "S25-6", "%TAG !e! tag:yaml.org,2002:\n--- !e!str 42\n", None),
    ("an_escape_in_a_flow_key_reads", "S25-3", "{!!%73tr k: v}\n", None),
    ("an_escape_decoding_outside_the_carried_set_refuses", "S25-3", "a: !!str%20x 42\n", "python_error"),
    ("an_escape_decoding_to_a_space_refuses", "S25-3", "a: !!foo%20bar 1\n", "python_error"),
    ("a_double_escape_decodes_once_and_refuses", "S25-5", "a: !!str%2520 1\n", "python_error"),
    ("a_percent_spelled_escape_decodes_once_and_refuses", "S25-5", "a: !!%2573tr 42\n", "python_error"),
    ("an_escape_in_a_local_tag_stays_refused", "S25-3", "a: !foo%3Abar 1\n", "python_error"),
    ("an_escape_decoding_to_a_colon_refuses", "S25-3", "a: !!str%3Ax 1\n", "python_error"),
    ("a_utf8_escape_decoding_outside_the_carried_set_refuses", "S25-2", "a: !!caf%C3%A9 1\n", "python_error"),
    ("a_verbatim_escape_naming_another_prefix_refuses", "S25-3", "a: !<tag:example.com,2026:foo%20bar> 1\n", "python_error"),
    ("a_truncated_escape_refuses", "S25-2", "a: !!str%2 1\n", "python_error"),
    ("a_non_hex_escape_refuses", "S25-2", "a: !!str%zz 1\n", "python_error"),
    ("a_trailing_percent_refuses", "S25-2", "a: !!str% 1\n", "python_error"),
    ("an_escape_decoding_to_invalid_utf8_refuses", "S25-2", "a: !!foo%FFbar 1\n", "python_error"),
    ("a_lone_utf8_lead_byte_escape_refuses", "S25-2", "a: !!foo%C3bar 1\n", "python_error"),
    ("a_malformed_escape_in_a_verbatim_name_refuses", "S25-2", "a: !<tag:yaml.org,2002:str%2> 1\n", "python_error"),
    ("an_escape_in_a_handle_place_refuses", "S25-5", "a: !%65!x 1\n", "python_error"),
    ("an_escape_past_a_carried_name_refuses_whole", "S25-4", "a: !!str%41 1\n", "python_error"),
    ("an_unescaped_space_in_a_verbatim_name_refuses", "S25-5", "a: !<tag:example.com,2026:foo bar> 1\n", "python_error"),
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
    case = {"kind": "tag_escape", "name": name, "source": source, "input": doc}
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
            "under the slice 25 standings"
        )
    else:
        case["expected_value"] = enc(yaml.safe_load(doc))
    cases.append(case)

out = {
    "generated_by": "tools/gen_slice25.py in this repo, run against PyYAML 6.0.1; regenerate by running it again, never by editing recordings by hand",
    "cases": cases,
}
path = sys.argv[1]
with open(path, "w", encoding="utf-8") as f:
    json.dump(out, f, indent=1, sort_keys=True)
    f.write("\n")
print("wrote", len(cases), "cases to", path)
