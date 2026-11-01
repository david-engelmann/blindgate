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

## Slice 3 obligations

Slice 3 retires the two trace limits that Round 1 accepted in writing. FIX-1 stated that symlinks are not followed, and the trace reading inherited from slice 1 could not tell a file that is not strace output at all from a genuine trace that recorded no reads. Both limits stop holding in this slice, and the obligations below amend FIX-1 and that inherited trace reading where they conflict. Where a slice 3 obligation and an earlier obligation disagree, the slice 3 obligation governs.

S3-1. Canonical path resolution, amending FIX-1 and SLICE-4. Every traced path is resolved canonically before it is scoped to the root. Canonical resolution follows every symlink component on disk and settles every dot segment, the way the filesystem itself settles the path, and a path whose target no longer exists is resolved as far as the filesystem allows and judged by that landing place. A traced path that resolves outside the root never enters the read set. An outside read is a traced path whose written form lands inside the root once its dot segments settle lexically and whose canonical resolution lands outside it, which is the shape of an escape through a symlink, and only those paths are named as outside reads under S3-2. A traced path that was already outside the root in its written form, the way an open of a system library is, is dropped unnamed exactly as the earlier slices dropped it, because a real capture names dozens of those and naming them would bury the escapes the report exists to show. A traced path that resolves inside the root is judged by its resolved location and reported in its resolved root relative form. A symlink inside the project that points outside the project therefore never supports a proven verdict, and a symlink that points at a real file inside the project is judged as that file. A plain path with no symlinks and no dot segments reports exactly as the earlier slices reported it, and the committed example reports from slices 1 and 2 regenerate byte identical.
Tests. tests/test_slice3.py test_symlink_pointing_outside_never_proves_and_is_named and test_symlink_pointing_inside_is_judged_on_the_resolved_path, and tests/test_slice2.py test_slice_one_example_reports_regenerate_byte_identical, test_slice_one_example_json_regenerates_byte_identical, and test_per_job_example_reports_regenerate_byte_identical.

S3-2. Outside reads are named in the report. A check result carries the reads that resolved outside the root, written as resolved absolute paths, in a section named outside reads in the text rendering and under the key outside_reads in the JSON rendering. The section and the key appear in a report only when at least one check in the run has an outside read, the same rule the trace source field already follows, and a run with no outside reads renders exactly the reports the earlier slices rendered. Outside reads never enter the read set, never enter the reads in surface, and never change a verdict, so a check whose only surface reads arrived through a symlink pointing outside is unproven or an empty read set exactly as its real read set dictates, with the outside reads named beside it.
Tests. tests/test_slice3.py test_outside_reads_are_named_in_text_and_json and test_outside_reads_never_change_a_verdict, and the example regeneration tests named under S3-1.

S3-3. Trace sanity validation, amending the trace reading inherited from slice 1. Before a read set is built from a trace file, the trace text is checked for shape. A text that is empty keeps the slice 1 meaning and yields an empty read set. A text that is not empty and holds no recognizable strace line is refused with a named error saying the trace does not look like strace output, and the CLI exits nonzero for it. A line is recognizable when it shows a syscall shape, meaning a syscall name and an opening parenthesis and later on the same line an equals sign with a return value after it, or when it carries one of the usual strace markers, namely the unfinished marker, the resumed marker, the signal banner, or the exit marker. One recognizable line is enough to accept the whole text. A genuine strace capture that records no file reads passes the check and still yields the empty read set verdict. The check guards every trace file entry point, meaning the shared trace option, the config trace field, and the trace-for override alike.
Tests. tests/test_slice3.py test_garbage_trace_is_refused_on_the_shared_trace, test_garbage_trace_is_refused_on_a_per_job_trace, test_one_recognizable_line_accepts_the_whole_trace, test_empty_trace_file_keeps_its_slice_one_meaning, and test_strace_with_no_file_reads_still_yields_an_empty_read_set.

S3-4. The run capture is exempt from the sanity refusal. A capture the CLI makes itself under the run option is produced by strace and is judged exactly as slice 1 judged it, with no shape check standing between the capture and the read set. The sanity refusal guards only trace files handed to the tool.
Tests. tests/test_slice3.py test_run_capture_is_not_shape_checked, guarded on strace availability.

## Round 2 review fixes

The Round 2 audit and red team found places where the slice 3 claims held on only part of the surface they named. The obligations below amend the slice 3 obligations where they conflict, and each one is covered by tests in tests/test_round2_fixes.py unless it says otherwise.

Obligation FIX2-1 covers the root itself. A traced path that resolves to the project root names no file inside it and enters neither the read set nor the outside reads, so an open of the root can never support a verdict. The guard the code always intended is now the guard that runs, and the case is pinned at unit level and end to end.

Obligation FIX2-2 covers live captures. A run capture names its outside reads exactly the way a handed over trace file does, because the capture path returns the same pair of read set and outside reads and the report carries them the same way. The slice 3 naming claim now holds on both capture modes the tool offers.

Obligation FIX2-3 covers the root alias spelling. An escape spelled through a symlink alias of the root itself is named as an outside read, because the naming rule also fires when the directory a path hangs from resolves inside the root while the path itself resolves outside it. Symlink chains and dangling symlinks to outside targets are named under the same rule and are pinned by tests.

Obligation FIX2-4 covers the shape definition. A recognizable strace line is a whole line of syscall shape, an optional process id, then a syscall name hard against its opening parenthesis, the call, an equals sign, and a return value, with an optional error name and message after it, or a line carrying one of the usual strace markers. A call shape buried inside a longer line of prose or source code no longer makes a file pass, and the two source shaped shapes the red team used are pinned as refused.

Obligation FIX2-5 covers uniform judgements. Every judgement handed to the analysis is a triple of read set, source, and outside reads, and the analysis refuses any other shape with a named error instead of guessing. Callers that have no outside reads pass an empty list in the third place.

Two points are accepted in writing rather than changed. A read that lands inside through a symlink is reported under its resolved path and the report does not name the alias spelling, because the resolved file is the substance of what was read and the alias is only a spelling of it. A trace text holding only whitespace is refused rather than treated as empty, because empty in the contract means a text of no length at all, and that boundary is stated here so it is a decision and not a discovery.

## Slice 4 obligations

Slice 4 removes the standing assumption the earlier slices made about relative paths. A relative traced path was resolved against the project root, which is only right when the traced process never changed directory. This slice tracks directory changes through the trace, so a relative path is resolved against the directory in force for its process at the line where it was opened. Where a slice 4 obligation and an earlier obligation disagree, the slice 4 obligation governs.

Obligation S4-1 covers directory state in the trace parse. The parse keeps one current directory per process id, and every process id starts at the project root, which is the standing assumption for the directory a traced process starts in. A line with a process id prefix speaks for that process, and lines with no process id prefix share one bucket of their own, exactly as the earlier slices treated one unnumbered stream. A change of directory for one process never moves the state of another, so an interleaved multi process trace keeps each process on its own path through the tree.
Tests. tests/test_slice4.py test_two_processes_keep_separate_directories and test_unprefixed_lines_share_one_bucket.

Obligation S4-2 covers the chdir record. A chdir line whose argument is a path and whose recorded return is not a failure moves the current directory of its process from that line onward. A chdir line with no recorded return, or with a failure return, moves nothing, under the same recorded success rule the opens already follow. A relative chdir argument resolves against the directory in force for that process at that point, so chains of relative chdirs walk the tree step by step. An absolute chdir argument replaces the directory outright. The chdir record itself is not a read and never enters a read set or an outside read list.
Tests. tests/test_slice4.py test_relative_chdir_chain_walks_step_by_step, test_failed_chdir_moves_nothing, test_unfinished_chdir_moves_nothing, and test_chdir_itself_is_not_a_read.

Obligation S4-3 covers relative opens under a moved directory. A relative open resolves against the directory in force for its process at that line, and the resolved path then goes through the canonical scoping of the earlier slices unchanged, so it is judged and reported by where it lands. A relative open after a chdir into a subdirectory of the root is therefore judged at the place the process actually opened, not at the root spelling the old reading gave it.
Tests. tests/test_slice4.py test_open_after_chdir_into_a_subdirectory_lands_there and test_open_before_any_chdir_still_resolves_against_the_root.

Obligation S4-4 covers leaving the root. A chdir to a directory outside the root moves the state outside, and relative opens from there are judged by their canonical resolution exactly as an absolute spelling of the same path would be, so an open that lands outside the root can never support a proven verdict for a surface inside the root, and the outside naming rule of the earlier slices decides whether the read is named or dropped. The directory state can also come back inside the root through a later chdir, and opens after that point resolve inside again.
Tests. tests/test_slice4.py test_chdir_outside_the_root_cannot_prove_an_inside_surface and test_chdir_back_inside_the_root_restores_inside_reads.

Obligation S4-5 covers the stated fchdir limit. Directory changes made through a file descriptor are not tracked, because a trace line for such a call names a descriptor and not a path, and this tool does not pair descriptors with the directories they were opened on. A relative open that follows an fchdir line resolves against the last directory set by a chdir line for its process, or against the root when no chdir line came before it. This limit is stated here so it is a decision and not a discovery.
Tests. tests/test_slice4.py test_fchdir_is_not_tracked_and_the_last_chdir_stands.

Obligation S4-6 covers every entry point at once. The directory tracking lives in the one trace parse that every entry point shares, so the shared trace option, the config trace field, the trace-for override, and the live run capture all read a trace the same way. The live capture now asks strace to record chdir beside openat, because a capture that never records a directory change could never be read with one.
Tests. tests/test_slice4.py test_run_capture_tracks_a_chdir, guarded on strace availability, beside the parse level tests above which every handed over trace entry point passes through.

Obligation S4-7 covers the unchanged past. A trace with no chdir line reads exactly as the earlier slices read it, because every process simply stays at the root it started from. The committed example reports from slices 1 and 2 regenerate byte identical, and no committed fixture trace needed a chdir line added to it or taken from it.
Tests. the example regeneration tests in tests/test_slice2.py and tests/test_slice3.py, and the whole earlier suite passing unaltered.

## Round 3 review fixes

The Round 3 red team found two shapes where the directory state the new slice kept could manufacture an inside read, and the two axis review found three limits the contract had not stated. The obligations below amend the slice 4 obligations where they conflict, and each one is covered by tests in tests/test_round3_fixes.py unless it says otherwise.

Obligation FIX3-1 covers fork inheritance. A fork the trace records, a clone or fork or vfork line with a recorded child process id as its return, seeds the child process bucket with the directory its parent held at that line. The live capture asks strace to record those calls beside openat and chdir, so the shell shape, a process that changes directory and then runs a command, reads the command's relative opens at the place the process actually was. A fork split across unfinished and resumed lines is seeded by the resumed form when that form records the child id.

Obligation FIX3-2 covers the stored state. The directory state is stored as the canonical resolution of the directory the process moved to, never as the spelled path with its dot segments settled lexically. A chdir that walks through a symlinked directory and back out with a dotdot therefore leaves the state where the kernel left the process, outside the root when the symlink target sits outside it, and no later relative open can be manufactured into an inside read by the spelling of an earlier move.

Obligation FIX3-3 covers the chdir record reading. The chdir pattern is anchored to the start of its line behind the optional process id, so chdir shaped text inside the path of an open moves nothing and swallows no read. The recorded return is read as a whole number token in decimal or hex, so a failure spelled in hex moves nothing the way a failure spelled in decimal does not. Process buckets key on the numeric value of the id, so one process spelled with padded digits shares one bucket.

Obligation FIX3-4 covers opening the directory in force. A traced path that resolves to the directory in force for its process, or to the root, enters neither the read set nor the outside reads, because an open of the directory itself is not a read of a file inside it.

Three limits are stated here so they are decisions and not discoveries. A chdir split by strace into an unfinished line and a resumed line moves nothing, because no single line records both the path and the success, and that follows the same recorded return rule the opens have always followed. A relative openat against a real directory descriptor rather than AT_FDCWD is read against the directory in force for its process, because the parse does not track which directory a descriptor names. Directory changes made through a file descriptor stay untracked, as S4-5 already states.
