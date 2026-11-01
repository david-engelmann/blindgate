"""Diffing a read set against a claimed surface and naming a verdict."""

from __future__ import annotations

import fnmatch
from dataclasses import dataclass

from . import VERDICT_EMPTY, VERDICT_PROVEN, VERDICT_UNPROVEN

_GLOB_CHARS = ("*", "?", "[")


@dataclass(frozen=True)
class CheckResult:
    name: str
    job: str
    surface: tuple
    read_set: tuple
    matched: tuple
    verdict: str


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
    """Pair each check with the read set and its claimed surface."""
    results = []
    for check in checks:
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
            )
        )
    return results
