# blindgate

It traces what a gate actually opened while it ran and diffs that against the surface the gate claims to certify.

## What this is

Gates can pass while never reading the thing they certify, and audit programs have found required checks whose green tick rested on an empty read list, including a visual gate that read a screenshot's hash and never opened the image. Blindgate captures what a CI job actually opened at the filesystem and process level and pairs that read set with the surface the check claims to cover. A gate whose read set never touches its surface is reported as unproven, with the trace attached, and an empty read set is named outright. The boundary with its siblings is one sentence. Deadgate asks whether it ran, finaltree asks what it ran on, blindgate asks what it read.

## Status

Local seed is done and the seed commit sits on the local main branch. The remote seed is still pending because remote writes return 403 and that was verified again on 2026-10-31. A token access fix has to land before the seed and the branches built on it can be pushed.

Setup is in progress locally. The contract in CONTRACT.md holds the obligations for this repo and the slice 1 obligations in it are met by the test suite. Slice 1 is built. It covers GitHub Actions workflows, one tracing method built on strace openat capture, and one report that pairs each check with its read set and its claimed surface.

AGENTS.md is pending. The canonical template has not been supplied by the main chat and no local substitute has been invented in its place.

Closed rounds stand at 0 of 100. Pinned formats stand at none.

## Licence

MIT. See LICENSE.
