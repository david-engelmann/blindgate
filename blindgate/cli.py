"""The blindgate command line.

Usage shape.

    python -m blindgate --workflow FILE --config FILE --trace LOG
    python -m blindgate --workflow FILE --config FILE --run COMMAND...
    python -m blindgate --workflow FILE --config FILE --trace-for JOB=LOG

A check that carries its own trace, from the config or from a
trace-for override, is judged on that trace alone. A check with no
trace of its own falls back to the shared trace or the shared run.
Exit code zero means the analysis completed. Exit code two means the
inputs were wrong in a way the tool can name.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .analysis import (
    SOURCE_OWN_TRACE,
    SOURCE_SHARED_RUN,
    SOURCE_SHARED_TRACE,
    TraceSource,
    analyse,
    analyse_judged,
    analyse_with_outside,
)
from .config import load_config, unknown_jobs
from .errors import BlindgateError
from .report import render_json, render_text
from .tracing import read_trace_from_log_file, run_traced
from .workflow import load_workflow_jobs


def build_parser():
    parser = argparse.ArgumentParser(
        prog="blindgate",
        description="Diff what a CI gate read against the surface it claims to certify.",
    )
    parser.add_argument("--workflow", required=True, help="GitHub Actions workflow file")
    parser.add_argument("--config", required=True, help="config file declaring the checks")
    parser.add_argument("--trace", help="an existing strace openat log to analyse")
    parser.add_argument(
        "--trace-for",
        action="append",
        default=[],
        metavar="JOB=PATH",
        help="a per job strace log that overrides the config trace for that job, repeatable",
    )
    parser.add_argument(
        "--run",
        nargs=argparse.REMAINDER,
        help="a command to run under strace and analyse",
    )
    parser.add_argument("--json", action="store_true", help="render the report as JSON")
    parser.add_argument(
        "--root",
        default=None,
        help="repo root the read set is scoped to, defaults to the working directory",
    )
    return parser


def parse_trace_for(values, checks, config_dir):
    """Return trace-for overrides as job id to display and read path pairs.

    A relative path resolves against the config file folder, the same
    basis the config trace field uses, never against the working
    directory the command happens to run from. The display text stays
    the value as given, so reports remain portable.
    """
    overrides = {}
    known_jobs = {check.job for check in checks}
    for value in values:
        job, separator, path = value.partition("=")
        if not separator or not job or not path:
            raise BlindgateError(
                f"trace-for value {value} is not written as a job id, an equals sign, and a path"
            )
        if job not in known_jobs:
            raise BlindgateError(f"trace-for names job {job}, which no check in the config runs")
        if job in overrides:
            raise BlindgateError(f"trace-for names job {job} more than once")
        candidate = Path(path)
        read_path = candidate if candidate.is_absolute() else Path(config_dir) / candidate
        overrides[job] = (path, read_path)
    return overrides


def _read_set_naming(path, root, owner):
    try:
        return read_trace_from_log_file(path, root)
    except BlindgateError as exc:
        raise BlindgateError(f"{owner}, {exc}") from exc


def _judgements(args, checks, overrides, root):
    """Build one read set, source, and outside reads triple per check."""
    shared_read_set = None
    shared_source = None
    shared_outside = ()
    shared_ready = False

    def shared():
        nonlocal shared_read_set, shared_source, shared_outside, shared_ready
        if not shared_ready:
            if args.trace:
                shared_read_set, shared_outside = read_trace_from_log_file(
                    args.trace, root
                )
                shared_source = TraceSource(SOURCE_SHARED_TRACE, args.trace)
            else:
                command = list(args.run)
                if command and command[0] == "--":
                    command = command[1:]
                shared_read_set, shared_outside = run_traced(command, root)
                shared_source = TraceSource(SOURCE_SHARED_RUN, " ".join(command))
            shared_ready = True
        return shared_read_set, shared_source, shared_outside

    has_shared = bool(args.trace) or bool(args.run)
    if args.trace:
        # A shared trace that was handed over is read even when every
        # check carries its own trace, so a bad path is never ignored.
        shared()
    judgements = []
    any_own = False
    for check in checks:
        if check.job in overrides:
            display, read_path = overrides[check.job]
            read_set, outside = _read_set_naming(
                read_path, root, f"the trace for job {check.job} cannot be used"
            )
            judgements.append(
                (read_set, TraceSource(SOURCE_OWN_TRACE, display), outside)
            )
            any_own = True
        elif check.trace_path is not None:
            read_set, outside = _read_set_naming(
                check.trace_path, root, f"check {check.name} names a trace that cannot be used"
            )
            judgements.append(
                (read_set, TraceSource(SOURCE_OWN_TRACE, check.trace), outside)
            )
            any_own = True
        elif has_shared:
            judgements.append(shared())
        else:
            raise BlindgateError(
                f"check {check.name} has no trace of its own and no shared trace or run was given"
            )
    return judgements, any_own


def main(argv=None):
    args = build_parser().parse_args(argv)
    root = Path(args.root).resolve() if args.root else Path.cwd()
    try:
        if args.trace and args.run:
            raise BlindgateError("give exactly one of --trace or --run")
        jobs = load_workflow_jobs(args.workflow)
        checks = load_config(args.config)
        unknown = unknown_jobs(checks, jobs)
        if unknown:
            names = ", ".join(f"{check.name} (job {check.job})" for check in unknown)
            raise BlindgateError(
                f"config names jobs that the workflow does not declare, {names}"
            )
        overrides = parse_trace_for(
            args.trace_for, checks, Path(args.config).resolve().parent
        )
        if not args.trace and not args.run and not overrides and all(
            check.trace_path is None for check in checks
        ):
            raise BlindgateError("give exactly one of --trace or --run")
        judgements, any_own = _judgements(args, checks, overrides, root)
    except BlindgateError as exc:
        print(f"error, {exc}", file=sys.stderr)
        return 2
    if any_own:
        results = analyse_judged(checks, judgements)
    elif judgements and judgements[0][2]:
        # No check carried its own trace, so the per check sources are
        # dropped on purpose here to keep the report in its slice 1
        # shape, but the shared trace did name outside reads and those
        # are carried into the report.
        results = analyse_with_outside(checks, judgements[0][0], judgements[0][2])
    else:
        # No check carried its own trace and nothing resolved outside
        # the root, so the report stays in its exact slice 1 shape.
        results = analyse(checks, judgements[0][0])
    print(render_json(results) if args.json else render_text(results))
    return 0


if __name__ == "__main__":
    sys.exit(main())
