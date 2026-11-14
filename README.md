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

The tool is the Rust binary alone, built with cargo. From a checkout of this repo, the build runs as written from the repo root and leaves the binary at target/debug/blindgate.

```
cargo build
```

## Quickstart

The example run judges the example trace against the checks the example config declares, with the examples folder as the root the read set is scoped to. Build the tool and run it over the files that ship under examples.

```
cargo build
./target/debug/blindgate --workflow examples/workflow.yml --config examples/blindgate.yml --trace examples/trace.log --root examples
```

It prints the report that stands in examples/report.txt, and its first check reads as follows.

```
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

## Usage

The full report pairs every check a config declares with the read set its job produced. This run uses the workflow, the config, and the trace that ship under examples, and its output is the report that stands in examples/report.txt.

```
./target/debug/blindgate --workflow examples/workflow.yml --config examples/blindgate.yml --trace examples/trace.log --root examples
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

The tool is the Rust crate alone, and the Python package it replaces is retired. The crate opened as slice 1 with the reader model, the line classification, the canonical scoping, and the openat reading, and the port slices that followed carried the parse entry whole, the file reading entries and the live capture, the config and workflow and analysis and report modules, and the evidence format those results imply. The command line stands in the crate's binary shell, scanning its arguments under the model the contract states, and the example reports under examples regenerate byte identical on every head. The gate is cargo test, and the suite carries the conformance batteries the Python package produced over its own recordings beside the crate's own tests.

Closed rounds stand at 20 of 100. Pinned formats stand at blindgate-evidence/1.

## Licence

MIT. See LICENSE.
