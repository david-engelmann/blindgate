# blindgate

It traces what a gate actually opened while it ran and diffs that against the surface the gate claims to certify.

## What this is

Gates can pass while never reading the thing they certify, and audit programs have found required checks whose green tick rested on an empty read list, including a visual gate that read a screenshot's hash and never opened the image. Blindgate captures what a CI job actually opened at the filesystem and process level and pairs that read set with the surface the check claims to cover. A gate whose read set never touches its surface is reported as unproven, with the trace attached, and an empty read set is named outright. The boundary with its siblings is one sentence. Deadgate asks whether it ran, finaltree asks what it ran on, blindgate asks what it read.

## Status

Seeded in October 2026. The build has not started. Part of the impatient programming portfolio, in the Gate Truth arc.

## Licence

MIT. See LICENSE.
