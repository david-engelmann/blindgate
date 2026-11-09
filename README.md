# blindgate

It traces what a gate actually opened while it ran and diffs that against the surface the gate claims to certify.

Blindgate captures what a CI job actually opened at the filesystem and process level and pairs that read set with the surface the check claims to cover. A gate whose read set never touches its surface is reported as unproven, with the trace attached, and an empty read set is named outright. It is used by handing it a workflow file, a config that declares each check and the surface it claims, and either a trace recorded earlier or a command to run under strace. The boundary with its siblings is one sentence. Deadgate asks whether it ran, finaltree asks what it ran on, blindgate asks what it read.

[![CI](https://github.com/david-engelmann/blindgate/actions/workflows/ci.yml/badge.svg)](https://github.com/david-engelmann/blindgate/actions/workflows/ci.yml)

```
blindgate report
================

check            unit tests
job              unit-tests
verdict          proven
claimed surface
    src/**
    tests/**
read set
    notes.txt
    src/app.py
    src/util.py
    tests/test_app.py
reads in surface
    src/app.py
    src/util.py
    tests/test_app.py
```

## Install

From a checkout of this repo, the Python tool installs with pip and the Rust crate builds with cargo. Both commands run as written from the repo root.

```
python -m pip install -e .
cargo build
```

## Quickstart

The crate carries a thin binary over the trace parse. Build it and run it over the example trace that ships in this repo, with the repo root as the root the read set is scoped to.

```
cargo build
./target/debug/blindgate examples/trace.log .
```

It prints the read set the trace holds, one path per line.

```
notes.txt
src/app.py
src/util.py
tests/test_app.py
```

## Usage

The full report pairs every check a config declares with the read set its job produced. This run uses the workflow, the config, and the trace that ship under examples, and its output is the report that stands in examples/report.txt.

```
python -m blindgate --workflow examples/workflow.yml --config examples/blindgate.yml --trace examples/trace.log --root examples
```

The first check is the captured block on the first screen of this README. The second check in the same report reads as follows.

```
check            docs check
job              docs-check
verdict          unproven
claimed surface
    docs/*.md
read set
    notes.txt
    src/app.py
    src/util.py
    tests/test_app.py
reads in surface
    none
```

A check can carry its own trace in the config, and a command line override can replace the trace for a whole job. A check with no trace of its own falls back to the shared trace. In place of a handed over trace, the tool can run a command under strace itself and judge the capture, on a machine that has strace installed, by giving the run flag and the command in place of the trace flag. A json flag on any of these runs renders the same report as JSON, and the JSON form of the example report stands in examples/report.json.

## Status

The local seed is done and the repo stands on GitHub. The 403 refusals the seed week recorded no longer hold, and work lands through pull requests. The contract in CONTRACT.md holds the obligations for this repo. AGENTS.md stands in this repo, the canonical template of the program with this repo's annex below its marked line.

The Python tool is the complete tool today. It judges a handed over trace or a live capture against the checks a config declares, and the example reports under examples regenerate byte identical on every head. The Rust port stands beside it as a library crate with a thin binary. The crate opened as port slice 1 with the reader model, the line classification, the canonical scoping, and the openat reading, and it now carries the parse entry whole, so the Rust parse answers any trace the Python parse answers, proven against recordings the Python package itself produced over the inputs its own tests use. The crate now carries the file reading entries and the live capture beside the parse, and the config and workflow and analysis and report modules beside those, and it publishes the evidence format those results imply. What stays Python only is the command line whole. Later port slices take that in the order the contract names.

Closed rounds stand at 18 of 100. Pinned formats stand at blindgate-evidence/1.

## Licence

MIT. See LICENSE.
