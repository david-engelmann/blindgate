import datetime
import json
import sys

import yaml

CASES = [
    ("str_tag_on_an_int_spelling", "S21-3", "a: !!str 42\n", None),
    ("int_tag_on_a_quoted_number", "S21-3", "a: !!int '42'\n", None),
    ("int_tag_reads_the_hex_form", "S21-3", "a: !!int 0x2A\n", None),
    ("int_tag_reads_the_0o_form", "S21-3", "a: !!int 0o17\n", None),
    ("float_tag_on_a_bare_exponent", "S21-3", "a: !!float 1e3\n", None),
    ("float_tag_reads_a_signed_bare_exponent", "S21-3", "a: !!float -1e3\n", None),
    ("bool_tag_reads_a_spelling", "S21-3", "a: !!bool yes\n", None),
    ("null_tag_reads_a_word_as_null", "S21-3", "a: !!null x\n", None),
    ("timestamp_tag_reads_a_date", "S21-3", "a: !!timestamp 2024-01-02\n", None),
    ("seq_tag_on_a_flow_sequence", "S21-3", "a: !!seq [1, 2]\n", None),
    ("map_tag_on_a_block_mapping", "S21-3", "a: !!map\n  b: 1\n", None),
    ("tag_stands_on_a_key", "S21-2", "!!str 42: v\n", None),
    ("tag_stands_in_flow", "S21-2", "{a: !!str 42}\n", None),
    ("tag_before_a_block_scalar", "S21-2", "a: !!str |\n  42\n", None),
    ("tag_and_anchor_in_one_order", "S21-2", "a: &x !!str 42\nb: *x\n", None),
    ("anchor_and_tag_in_the_other_order", "S21-2", "a: !!str &x 42\nb: *x\n", None),
    ("verbatim_tag_reads", "S21-2", "a: !<tag:yaml.org,2002:str> 42\n", None),
    ("tag_with_no_value_reads_the_empty_scalar", "S21-2", "a: !!str\n", None),
    ("bare_non_specific_tag_reads_as_no_tag", "S21-2", "a: ! 42\n", None),
    ("tag_on_a_sequence_entry", "S21-2", "- !!str 42\n", None),
    ("an_unknown_tag_refuses_in_both", "S21-4", "a: !!unknown x\n", "python_error"),
    ("a_local_tag_refuses_in_both", "S21-4", "a: !custom x\n", "python_error"),
    ("a_scalar_tag_on_a_sequence_refuses_in_both", "S21-5", "a: !!str [1]\n", "python_error"),
    ("a_seq_tag_on_a_scalar_refuses_in_both", "S21-5", "a: !!seq hello\n", "python_error"),
    ("an_int_tag_on_a_word_refuses_in_both", "S21-5", "a: !!int hello\n", "python_error"),
    ("a_bool_tag_outside_the_spellings_refuses_in_both", "S21-5", "a: !!bool maybe\n", "python_error"),
    ("two_tags_refuse_in_both", "S21-5", "a: !!str !!int 5\n", "python_error"),
    ("a_tag_on_an_alias_refuses_in_both", "S21-5", "a: &x 42\nb: !!str *x\n", "python_error"),
    ("a_binary_tag_stands_as_a_stated_limit", "S21-4", "a: !!binary aGk=\n", "port_refuses"),
    ("a_set_tag_stands_as_a_stated_limit", "S21-4", "a: !!set {x: null, y: null}\n", "port_refuses"),
    ("an_omap_tag_stands_as_a_stated_limit", "S21-4", "a: !!omap [x: 1]\n", "port_refuses"),
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
    case = {"kind": "tag", "name": name, "source": source, "input": doc}
    if refusal == "python_error":
        try:
            yaml.safe_load(doc)
        except Exception:
            pass
        else:
            raise SystemExit("case %s was expected to fail in PyYAML and did not" % name)
        case["python_error"] = "PyYAML refuses the document"
    elif refusal == "port_refuses":
        yaml.safe_load(doc)
        case["port_refuses"] = True
        case["provenance"] = (
            "PyYAML reads a shape the value model does not carry; "
            "the port refuses it under S21-4 as its stated limit"
        )
    else:
        case["expected_value"] = enc(yaml.safe_load(doc))
    cases.append(case)

out = {
    "generated_by": "tools/gen_slice21.py in this repo, run against PyYAML 6.0.1; regenerate by running it again, never by editing recordings by hand",
    "cases": cases,
}
path = sys.argv[1]
with open(path, "w", encoding="utf-8") as f:
    json.dump(out, f, indent=1, sort_keys=True)
    f.write("\n")
print("wrote", len(cases), "cases to", path)
