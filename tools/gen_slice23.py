import datetime
import json
import sys

import yaml

CASES = [
    ("a_version_directive_reads", "S23-3", "%YAML 1.1\n---\na: 1\n", None),
    ("a_version_1_2_directive_reads", "S23-3", "%YAML 1.2\n---\na: 1\n", None),
    ("a_version_1_3_directive_reads", "S23-3", "%YAML 1.3\n---\na: 1\n", None),
    ("a_version_with_leading_zeros_reads", "S23-3", "%YAML 01.2\n---\na: 1\n", None),
    ("a_version_directive_changes_no_bool_resolution", "S23-3", "%YAML 1.2\n---\non\n", None),
    ("a_version_directive_changes_no_sexagesimal_resolution", "S23-3", "%YAML 1.1\n---\n1:20\n", None),
    ("a_version_directive_with_a_comment", "S23-2", "%YAML 1.2 # note\n---\na: 1\n", None),
    ("directives_after_a_comment_and_a_blank_line", "S23-2", "# note\n\n%YAML 1.2\n---\na: 1\n", None),
    ("an_empty_document_after_directives", "S23-3", "%YAML 1.2\n---\n", None),
    ("a_document_end_after_directives", "S23-3", "%YAML 1.2\n---\na: 1\n...\n", None),
    ("directives_before_a_flow_document", "S23-3", "%YAML 1.2\n--- {a: 1}\n", None),
    ("a_tag_directive_reads_a_carried_type", "S23-3", "%TAG !e! tag:yaml.org,2002:\n---\n!e!str 5\n", None),
    ("a_tag_directive_on_a_value", "S23-3", "%TAG !e! tag:yaml.org,2002:\n---\na: !e!int '42'\n", None),
    ("a_tag_directive_on_a_flow_value", "S23-3", "%TAG !e! tag:yaml.org,2002:\n---\n{a: !e!str 5}\n", None),
    ("a_tag_directive_on_an_explicit_key", "S23-3", "%TAG !e! tag:yaml.org,2002:\n---\n? !e!str 42\n: v\n", None),
    ("a_tag_directive_on_a_block_scalar", "S23-3", "%TAG !e! tag:yaml.org,2002:\n---\n!e!str |\n  text\n", None),
    ("the_primary_handle_redefined", "S23-3", "%TAG ! tag:yaml.org,2002:\n---\n!str 5\n", None),
    ("the_secondary_handle_restated", "S23-3", "%TAG !! tag:yaml.org,2002:\n---\n!!str 5\n", None),
    ("a_percent_decoded_prefix", "S23-3", "%TAG !e! tag:yaml.org%2C2002:\n---\n!e!str 5\n", None),
    ("a_handle_with_digits_and_dashes", "S23-2", "%TAG !e-x_y! tag:yaml.org,2002:\n---\n!e-x_y!bool yes\n", None),
    ("a_yaml_and_a_tag_directive_together", "S23-3", "%YAML 1.2\n%TAG !e! tag:yaml.org,2002:\n---\n!e!str hi\n", None),
    ("a_tag_directive_before_a_yaml_directive", "S23-3", "%TAG !e! tag:yaml.org,2002:\n%YAML 1.1\n---\n!e!int '7'\n", None),
    ("a_reserved_directive_is_skipped", "S23-2", "%FOO bar\n---\na: 1\n", None),
    ("a_reserved_directive_without_arguments", "S23-2", "%FOO\n---\na: 1\n", None),
    ("a_lowercase_reserved_directive_is_skipped", "S23-2", "%yaml 1.2\n---\na: 1\n", None),
    ("a_version_2_0_directive", "S23-3", "%YAML 2.0\n---\na: 1\n", "python_error"),
    ("a_version_0_9_directive", "S23-3", "%YAML 0.9\n---\na: 1\n", "python_error"),
    ("a_version_directive_with_no_version", "S23-5", "%YAML\n---\na: 1\n", "python_error"),
    ("a_version_directive_with_a_bad_version", "S23-5", "%YAML x.y\n---\na: 1\n", "python_error"),
    ("a_version_directive_with_an_extra_token", "S23-5", "%YAML 1.2 extra\n---\na: 1\n", "python_error"),
    ("a_version_directive_with_a_tab_separator", "S23-5", "%YAML\t1.2\n---\na: 1\n", "python_error"),
    ("a_duplicate_version_directive", "S23-5", "%YAML 1.1\n%YAML 1.2\n---\na: 1\n", "python_error"),
    ("a_duplicate_tag_handle", "S23-5", "%TAG !e! tag:x/\n%TAG !e! tag:y/\n---\na: 1\n", "python_error"),
    ("a_tag_directive_with_no_prefix", "S23-5", "%TAG !e!\n---\na: 1\n", "python_error"),
    ("a_tag_directive_with_an_unmarked_handle", "S23-5", "%TAG e! tag:x/\n---\na: 1\n", "python_error"),
    ("a_tag_directive_with_an_extra_token", "S23-5", "%TAG !e! tag:x/ extra\n---\na: 1\n", "python_error"),
    ("a_tag_directive_with_a_hash_in_its_prefix", "S23-5", "%TAG !e! tag:x#y/\n---\na: 1\n", "python_error"),
    ("a_bare_percent_sign", "S23-5", "%\n---\na: 1\n", "python_error"),
    ("a_directive_with_no_document_start", "S23-5", "%YAML 1.2\na: 1\n", "python_error"),
    ("a_directive_alone", "S23-5", "%YAML 1.2\n", "python_error"),
    ("a_directive_before_an_end_marker", "S23-5", "%YAML 1.2\n...\n", "python_error"),
    ("a_second_document_with_directives", "S23-3", "%YAML 1.2\n---\na: 1\n...\n%YAML 1.1\n---\nb: 2\n", "python_error"),
    ("a_directive_after_content", "S23-3", "a: 1\n%YAML 1.2\n", "python_error"),
    ("an_undeclared_handle", "S23-3", "%TAG !f! tag:yaml.org,2002:\n---\n!e!str 5\n", "python_error"),
    ("a_tag_expanding_to_an_unread_tag", "S23-4", "%TAG !e! tag:example.com,2000:app/\n---\n!e!foo bar\n", "python_error"),
    ("the_secondary_handle_redefined_away", "S23-4", "%TAG !! tag:x/\n---\n!!str 5\n", "python_error"),
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
    case = {"kind": "directive", "name": name, "source": source, "input": doc}
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
            "under the slice 23 standings"
        )
    else:
        case["expected_value"] = enc(yaml.safe_load(doc))
    cases.append(case)

out = {
    "generated_by": "tools/gen_slice23.py in this repo, run against PyYAML 6.0.1; regenerate by running it again, never by editing recordings by hand",
    "cases": cases,
}
path = sys.argv[1]
with open(path, "w", encoding="utf-8") as f:
    json.dump(out, f, indent=1, sort_keys=True)
    f.write("\n")
print("wrote", len(cases), "cases to", path)
