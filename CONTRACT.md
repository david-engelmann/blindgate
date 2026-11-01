# blindgate contract

This contract is the executable spec for blindgate. Every obligation below maps to at least one test in the test suite, and the mapping is named under each obligation. Wording rule for verdicts throughout this project. An unread surface is unproven, never fraud, and an empty read set is named as an empty read set.

## The boundary

Deadgate asks whether it ran. Finaltree asks what it ran on. Blindgate asks what it read. Blindgate traces what a CI gate actually opened while it ran and diffs that against the surface the gate claims to certify.

## Setup obligations

SETUP-1. The repo keeps the seed files it was created with, namely README.md, LICENSE with the MIT text, and .gitignore, and the licence text is not altered.
Tests. tests/test_contract.py test_seed_files_present_and_licence_intact.

SETUP-2. The README status section states the local seed state, the remote seed state, the contract state, the slice state, the closed round count, the AGENTS.md state, and the pinned formats.
Tests. tests/test_contract.py test_readme_status_section_states_the_facts.

SETUP-3. The CI workflow runs the same command as the local gate, which is python -m pytest, and it runs on push and on pull_request.
Tests. tests/test_contract.py test_ci_runs_the_local_gate_command.

SETUP-4. No AGENTS.md exists in this repo until the canonical template is supplied by the main chat, and no local substitute is invented in its place.
Tests. tests/test_contract.py test_no_agents_md_until_canonical_template_arrives.

## Slice 1 obligations

SLICE-1. Workflow parsing. Given a GitHub Actions workflow file, blindgate extracts the set of job ids declared under the jobs key. A workflow with no jobs is an error, not an empty pass.
Tests. tests/test_workflow.py test_extracts_job_ids and test_workflow_without_jobs_is_an_error.

SLICE-2. Claimed surface declaration. A config file in JSON or YAML declares checks, and each check carries a name, a job id, and a claimed surface given as a list of path globs. A check whose job id does not appear in the workflow is a reported error, never a silent pass, and the CLI exits nonzero for it.
Tests. tests/test_config.py test_loads_json_config and test_loads_yaml_config, tests/test_cli.py test_unknown_job_is_a_reported_error.

SLICE-3. One tracing method. The only tracing method is strace openat capture. A read set is built from openat calls that opened a path for reading and succeeded. An open with O_RDONLY or O_RDWR counts as a read. An open with O_WRONLY alone does not count. An open that failed with a negative return does not count.
Tests. tests/test_trace.py test_read_opens_are_collected, test_writes_only_open_is_not_a_read, and test_failed_open_is_not_a_read.

SLICE-4. Read set scope. The read set reported for a check holds only paths that fall under the repo root. Absolute paths outside the root are dropped, and absolute paths inside the root are reported relative to the root. Relative paths are kept as given.
Tests. tests/test_trace.py test_paths_outside_root_are_dropped and test_paths_inside_root_become_relative.

SLICE-5. Verdicts. Each check gets exactly one verdict from its read set and claimed surface. Proven means at least one read falls inside the claimed surface. Unproven means the read set is not empty and no read falls inside the claimed surface. Empty means the read set is empty and the report names it as an empty read set. The word fraud appears in no verdict and no report.
Tests. tests/test_analysis.py test_proven_when_a_read_lands_in_the_surface, test_unproven_when_reads_miss_the_surface, test_empty_when_the_read_set_is_empty, and test_no_verdict_or_report_says_fraud.

SLICE-6. One report format. One report pairs each check with its claimed surface, its full read set, and the reads that landed inside the surface, plus the verdict. The report renders as plain text and as JSON with the same facts.
Tests. tests/test_report.py test_text_report_pairs_check_surface_reads_and_verdict and test_json_report_carries_the_same_facts.

SLICE-7. Live capture. When strace is available, the CLI can run a command under strace with openat tracing and analyse the capture it produced. When strace is missing, the CLI reports that plainly and exits nonzero rather than guessing a read set.
Tests. tests/test_cli.py test_run_mode_captures_reads_from_a_live_command, guarded on strace availability, and tests/test_trace.py test_run_command_without_strace_raises_a_clear_error, guarded on strace absence.

SLICE-8. Exit codes. The CLI exits zero when the analysis completes, whatever the verdicts are, because the verdicts are the report and not a crash. The CLI exits nonzero with a written error for a bad workflow, a bad config, an unknown job, or a missing trace source.
Tests. tests/test_cli.py test_unproven_verdict_still_exits_zero, test_unknown_job_is_a_reported_error, and test_missing_trace_and_run_is_an_error.

## Slice 2 obligations

Slice 2 removes the named slice 1 limit that one trace judged every check in a run. Each check can now carry its own trace, and a check with a trace of its own is judged on that trace alone. The shared trace and the shared run from slice 1 remain as the fallback for checks that carry no trace of their own.

S2-1. A per check trace field. A config entry may carry an optional trace field naming one strace log file for that check. The path is resolved relative to the folder holding the config file. A trace field that is not a non empty piece of text is a reported error. A check with a trace of its own is judged on the read set from that trace alone, even when a shared trace in the same run read different files.
Tests. tests/test_config.py test_loads_a_per_check_trace_field and test_trace_field_must_be_text, and tests/test_slice2.py test_per_check_trace_is_judged_alone.

S2-2. The shared fallback stands. A check with no trace of its own is judged on the shared trace given with the trace option or on the shared run capture, exactly as slice 1 behaved. Giving both the trace option and the run option in one invocation remains an error.
Tests. tests/test_slice2.py test_check_without_own_trace_falls_back_to_shared_trace and test_check_without_own_trace_falls_back_to_shared_run, and tests/test_cli.py test_missing_trace_and_run_is_an_error.

S2-3. The command line override. The CLI accepts a repeatable trace-for option written as a job id, an equals sign, and a path. The override replaces the config trace for every check that runs that job. A trace-for value without an equals sign or with an empty side is a reported error. A trace-for that names a job no configured check runs is a reported error naming the job, never a silent pass.
Tests. tests/test_slice2.py test_trace_for_overrides_the_config_trace, test_trace_for_with_an_unknown_job_is_a_reported_error, and test_trace_for_without_an_equals_sign_is_a_reported_error.

S2-4. Missing traces are named errors. When a per check trace file cannot be read, the CLI exits nonzero with an error naming the check that owns the trace. When the trace-for file for a job cannot be read, the error names the job. When a check carries no trace of its own and no shared trace or shared run was given, the error names that check. When every check carries a trace of its own, neither the trace option nor the run option is required.
Tests. tests/test_slice2.py test_missing_per_check_trace_is_a_named_error, test_missing_trace_for_file_is_a_named_error, test_uncovered_check_without_a_shared_source_is_a_named_error, and test_all_checks_with_own_traces_need_no_shared_source.

S2-5. The report states the trace source. In a run where at least one check is judged on a trace of its own, the report states for every check which source judged it, as its own trace file, the shared trace, or the shared run, in the text rendering and in the JSON rendering alike. The word fraud appears in no verdict and no report, as slice 1 already required.
Tests. tests/test_slice2.py test_report_states_each_trace_source and test_mixed_report_still_never_says_fraud.

S2-6. Slice 1 keeps working unchanged. Every slice 1 invocation form, config shape, and example keeps working. A run judged on one shared trace alone renders exactly the report slice 1 rendered, and the committed slice 1 example reports regenerate byte identical under the shared trace invocation.
Tests. tests/test_slice2.py test_slice_one_example_reports_regenerate_byte_identical and test_slice_one_example_json_regenerates_byte_identical, and the whole slice 1 suite passing unaltered.

S2-7. The mixed verdict proof. One run over two checks that share one workflow can name the first check proven from its own trace and the second check an empty read set from its own trace, in the same report. A committed example under examples/per-job shows this proof with its own traces and its generated reports, and the slice 1 example is not altered to build it.
Tests. tests/test_slice2.py test_mixed_verdicts_from_per_job_traces_in_one_run and test_per_job_example_reports_regenerate_byte_identical.

## Round 1 review fixes

These obligations amend SLICE-3, SLICE-4, S2-2, and S2-3 where they were written loosely, and they came out of the Round 1 red team and review. Where a fix obligation and an earlier obligation disagree, the fix obligation governs.

FIX-1. Normalised read set paths, amending SLICE-4. Dot segments in a traced path are resolved lexically before the path is scoped to the root and matched against a claimed surface. A relative path whose normalised form climbs above the root is dropped from the read set. An absolute path is normalised first and then scoped, so a path that lands in a different folder than its written form suggests is judged by where it lands. Symlinks are not followed, and that is a stated limit of lexical tracing, not a silent one.
Tests. tests/test_round1_fixes.py test_relative_dotdot_escape_is_dropped, test_absolute_dotdot_lands_where_it_points, and test_relative_dotdot_inside_root_normalises.

FIX-2. Unfinished and non content opens are not reads, amending SLICE-3. An openat line with no recorded return value counts as no read, because success was never recorded for it. An open carrying O_PATH or O_DIRECTORY counts as no content read, whatever else it carries.
Tests. tests/test_round1_fixes.py test_unfinished_open_counts_as_no_read, test_o_path_open_counts_as_no_read, and test_directory_open_counts_as_no_read.

FIX-3. Trace-for paths resolve like config traces, amending S2-3. A relative trace-for path resolves against the folder holding the config file, the same basis as the config trace field. Naming the same job twice in trace-for options is a reported error naming the job. The report still prints the value as given.
Tests. tests/test_round1_fixes.py test_trace_for_relative_path_resolves_against_the_config_folder and test_duplicate_trace_for_job_is_a_reported_error.

FIX-4. A handed over shared trace is always read, amending S2-2. When the trace option names a file, that file is read even when every check carries a trace of its own, so a missing shared trace is a reported error and never silently ignored. The both given error for the trace option and the run option together is exercised by a named test, closing the S2-2 test mapping.
Tests. tests/test_round1_fixes.py test_both_trace_and_run_is_a_reported_error and test_missing_shared_trace_errors_even_when_no_check_needs_it.

FIX-5. Trace field error wording and coverage, amending S2-1. The reported error for a bad trace field says the field is not a non empty piece of text, and empty text and list values are covered by tests alongside the integer case from slice 2.
Tests. tests/test_round1_fixes.py test_empty_trace_field_is_a_named_error_with_the_contract_wording and test_list_trace_field_is_a_named_error.
