# blindgate

It traces what a gate actually opened while it ran and diffs that against the surface the gate claims to certify.

## What this is

Gates can pass while never reading the thing they certify, and audit programs have found required checks whose green tick rested on an empty read list, including a visual gate that read a screenshot's hash and never opened the image. Blindgate captures what a CI job actually opened at the filesystem and process level and pairs that read set with the surface the check claims to cover. A gate whose read set never touches its surface is reported as unproven, with the trace attached, and an empty read set is named outright. The boundary with its siblings is one sentence. Deadgate asks whether it ran, finaltree asks what it ran on, blindgate asks what it read.

## Status

Local seed is done and the seed commit sits on the local main branch. The remote seed is still pending because remote writes return 403 and that was verified again on 2026-10-31. A token access fix has to land before the seed and the branches built on it can be pushed.

Setup is in progress locally. The contract in CONTRACT.md holds the obligations for this repo and the slice 1 obligations in it are met by the test suite. Slice 1 is built. It covers GitHub Actions workflows, one tracing method built on strace openat capture, and one report that pairs each check with its read set and its claimed surface.

Slice 2 is built on a local branch as the Round 1 work increment. The slice 2 obligations in CONTRACT.md are met by the test suite. Each check can now carry its own trace in the config, a command line override can replace the trace for a whole job, and a check with no trace of its own falls back to the shared trace or the shared run exactly as slice 1 behaved. The report states which trace source judged each check whenever a check is judged on a trace of its own. A committed example under examples/per-job shows one check proven by its own trace and a second check named an empty read set by its own trace in the same report, and the slice 1 example reports still regenerate byte identical under the shared trace invocation. Round 1 closed on 2026-10-31 after its audit, its two axis review, its red team, and owner approval. The red team found three Major defects in the first slice 2 head, covering dot segment paths, trace-for resolution, and unfinished opens, and every one was fixed with regression tests before the round closed.

Slice 3 is built on a local branch as the Round 2 work increment. The slice 3 obligations in CONTRACT.md are met by the test suite. Traced paths are now resolved canonically before they are scoped to the root, so the limit FIX-1 stated in Round 1, that symlinks are not followed, is retired. A symlink inside the project that points outside it resolves outside and never supports a proven verdict, and the reads that escape through a symlink are named in the report as outside reads, while an open that was already outside in its written form, the way a system library open is, stays dropped and unnamed as before. A trace file that is not strace output at all is now refused with a named error instead of reading as a genuine trace with no reads, and that retires the second limit Round 1 accepted in writing. The slice 1 and slice 2 example reports still regenerate byte identical. Round 2 closed on 2026-10-31 after slice 3 was audited and reviewed on two axes and attacked by a red team, with every material finding fixed under the contract fix obligations FIX2-1 to FIX2-5 before the owner of this line approved the round.

Slice 4 is built on a local branch as the Round 3 work increment. The slice 4 obligations in CONTRACT.md are met by the test suite. Traced paths are now resolved against the directory in force for their process at the line where they were opened, because the trace parse tracks the directory changes a chdir record makes, one current directory per process id, starting from the root. A gate that changed into a subdirectory and read there is judged at the place it actually read, and a gate that changed out of the project and read there can no longer support a proven verdict for a surface inside it. Directory changes made through a file descriptor are not tracked, and the contract states that limit in writing rather than half implementing it. The live capture records chdir beside openat so a capture can be read the same way as a handed over trace. The slice 1 and slice 2 example reports still regenerate byte identical, because no earlier trace holds a directory change. Round 3 closed on 2026-11-01 after slice 4 was audited and reviewed on two axes and attacked by a red team, with every material finding fixed under the contract fix obligations FIX3-1 to FIX3-4 before the owner of this line approved the round.

AGENTS.md is pending. The canonical template has not been supplied by the main chat and no local substitute has been invented in its place.

Closed rounds stand at 3 of 100. Pinned formats stand at none.

## Licence

MIT. See LICENSE.
