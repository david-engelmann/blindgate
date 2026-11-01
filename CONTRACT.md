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
