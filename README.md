# blindgate

It traces what a gate actually opened while it ran and diffs that against the surface the gate claims to certify.

## What this is

Gates can pass while never reading the thing they certify, and audit programs have found required checks whose green tick rested on an empty read list, including a visual gate that read a screenshot's hash and never opened the image. Blindgate captures what a CI job actually opened at the filesystem and process level and pairs that read set with the surface the check claims to cover. A gate whose read set never touches its surface is reported as unproven, with the trace attached, and an empty read set is named outright. The boundary with its siblings is one sentence. Deadgate asks whether it ran, finaltree asks what it ran on, blindgate asks what it read.

## Status

Local seed is done and the seed commit sits on the local main branch. The remote seed is still pending because remote writes return 403 and that was verified again on 2026-10-31. A token access fix has to land before the seed and the branches built on it can be pushed.

Setup is in progress locally. The contract in CONTRACT.md holds the obligations for this repo and the slice 1 obligations in it are met by the test suite. Slice 1 is built. It covers GitHub Actions workflows, one tracing method built on strace openat capture, and one report that pairs each check with its read set and its claimed surface.

Slice 2 is built on a local branch as the Round 1 work increment. The slice 2 obligations in CONTRACT.md are met by the test suite. Each check can now carry its own trace in the config, a command line override can replace the trace for a whole job, and a check with no trace of its own falls back to the shared trace or the shared run exactly as slice 1 behaved. The report states which trace source judged each check whenever a check is judged on a trace of its own. A committed example under examples/per-job shows one check proven by its own trace and a second check named an empty read set by its own trace in the same report, and the slice 1 example reports still regenerate byte identical under the shared trace invocation. Round 1 closed on 2026-10-31 after its audit, its two axis review, its red team, and owner approval. The red team found three Major defects in the first slice 2 head, covering dot segment paths, trace-for resolution, and unfinished opens, and every one was fixed with regression tests before the round closed.

AGENTS.md is pending. The canonical template has not been supplied by the main chat and no local substitute has been invented in its place.

Closed rounds stand at 1 of 100. Pinned formats stand at none.

## Licence

MIT. See LICENSE.
