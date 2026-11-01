"""The blindgate command line.

Usage shape.

    python -m blindgate --workflow FILE --config FILE --trace LOG
    python -m blindgate --workflow FILE --config FILE --run COMMAND...

Exit code zero means the analysis completed. Exit code two means the
inputs were wrong in a way the tool can name.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .analysis import analyse
from .config import load_config, unknown_jobs
from .errors import BlindgateError
from .report import render_json, render_text
from .tracing import read_set_from_log_file, run_traced
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


def main(argv=None):
    args = build_parser().parse_args(argv)
    root = Path(args.root).resolve() if args.root else Path.cwd()
    try:
        if bool(args.trace) == bool(args.run):
            raise BlindgateError("give exactly one of --trace or --run")
        jobs = load_workflow_jobs(args.workflow)
        checks = load_config(args.config)
        unknown = unknown_jobs(checks, jobs)
        if unknown:
            names = ", ".join(f"{check.name} (job {check.job})" for check in unknown)
            raise BlindgateError(
                f"config names jobs that the workflow does not declare, {names}"
            )
        if args.trace:
            read_set = read_set_from_log_file(args.trace, root)
        else:
            command = list(args.run)
            if command and command[0] == "--":
                command = command[1:]
            read_set = run_traced(command, root)
    except BlindgateError as exc:
        print(f"error, {exc}", file=sys.stderr)
        return 2
    results = analyse(checks, read_set)
    print(render_json(results) if args.json else render_text(results))
    return 0


if __name__ == "__main__":
    sys.exit(main())
