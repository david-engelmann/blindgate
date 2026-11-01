"""Diffing a read set against a claimed surface and naming a verdict."""

from __future__ import annotations

import fnmatch
from dataclasses import dataclass

from . import VERDICT_EMPTY, VERDICT_PROVEN, VERDICT_UNPROVEN

_GLOB_CHARS = ("*", "?", "[")


SOURCE_OWN_TRACE = "own_trace"
SOURCE_SHARED_TRACE = "shared_trace"
SOURCE_SHARED_RUN = "shared_run"


@dataclass(frozen=True)
class TraceSource:
    kind: str
    detail: str


@dataclass(frozen=True)
class CheckResult:
    name: str
    job: str
    surface: tuple
    read_set: tuple
    matched: tuple
    verdict: str
    trace_source: TraceSource | None = None


def path_matches_surface(path, pattern):
    """Return True when a root relative path falls inside a glob."""
    if fnmatch.fnmatchcase(path, pattern):
        return True
    if not any(char in pattern for char in _GLOB_CHARS):
        prefix = pattern.rstrip("/")
        if path == prefix or path.startswith(prefix + "/"):
            return True
    return False


def verdict_for(read_set, matched):
    if not read_set:
        return VERDICT_EMPTY
    if matched:
        return VERDICT_PROVEN
    return VERDICT_UNPROVEN


def analyse(checks, read_set):
    """Pair each check with one shared read set and its claimed surface."""
    return analyse_judged(checks, [(read_set, None) for _check in checks])


def analyse_judged(checks, judgements):
    """Pair each check with the read set and trace source that judged it.

    judgements runs parallel to checks and holds one pair per check, the
    read set that judged the check and the TraceSource that produced it,
    or None for the source in a plain shared read set run.
    """
    if len(judgements) != len(checks):
        raise ValueError("judgements must run parallel to checks")
    results = []
    for check, (read_set, source) in zip(checks, judgements):
        matched = tuple(
            path
            for path in read_set
            if any(path_matches_surface(path, glob) for glob in check.surface)
        )
        results.append(
            CheckResult(
                name=check.name,
                job=check.job,
                surface=tuple(check.surface),
                read_set=tuple(read_set),
                matched=matched,
                verdict=verdict_for(read_set, matched),
                trace_source=source,
            )
        )
    return results
