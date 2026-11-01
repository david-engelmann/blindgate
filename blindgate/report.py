"""The one report format, rendered as plain text and as JSON."""

from __future__ import annotations

import json

from .analysis import SOURCE_OWN_TRACE, SOURCE_SHARED_RUN, SOURCE_SHARED_TRACE

_SOURCE_LABELS = {
    SOURCE_OWN_TRACE: "own trace file",
    SOURCE_SHARED_TRACE: "shared trace",
    SOURCE_SHARED_RUN: "shared run",
}


def source_text(source):
    label = _SOURCE_LABELS.get(source.kind, source.kind)
    if source.detail:
        return f"{label} {source.detail}"
    return label


def _shows_sources(results):
    return any(result.trace_source is not None for result in results)


def _shows_outside(results):
    return any(result.outside_reads for result in results)


def result_to_dict(result, show_source=False, show_outside=False):
    data = {
        "name": result.name,
        "job": result.job,
        "verdict": result.verdict,
        "claimed_surface": list(result.surface),
        "read_set": list(result.read_set),
        "reads_in_surface": list(result.matched),
    }
    if show_source:
        source = result.trace_source
        data["trace_source"] = (
            None
            if source is None
            else {"kind": source.kind, "detail": source.detail, "text": source_text(source)}
        )
    if show_outside:
        data["outside_reads"] = list(result.outside_reads)
    return data


def report_to_dict(results):
    show_source = _shows_sources(results)
    show_outside = _shows_outside(results)
    checks = [
        result_to_dict(result, show_source=show_source, show_outside=show_outside)
        for result in results
    ]
    summary = {}
    for result in results:
        summary[result.verdict] = summary.get(result.verdict, 0) + 1
    return {"checks": checks, "summary": summary}


def _listing(values):
    if not values:
        return ["    none"]
    return [f"    {value}" for value in values]


def render_text(results):
    show_source = _shows_sources(results)
    show_outside = _shows_outside(results)
    lines = ["blindgate report", "================", ""]
    for result in results:
        lines.append(f"check            {result.name}")
        lines.append(f"job              {result.job}")
        if show_source:
            rendered = (
                "not recorded" if result.trace_source is None else source_text(result.trace_source)
            )
            lines.append(f"trace source     {rendered}")
        lines.append(f"verdict          {result.verdict}")
        lines.append("claimed surface")
        lines.extend(_listing(result.surface))
        lines.append("read set")
        lines.extend(_listing(result.read_set))
        lines.append("reads in surface")
        lines.extend(_listing(result.matched))
        if show_outside:
            lines.append("outside reads")
            lines.extend(_listing(result.outside_reads))
        lines.append("")
    return "\n".join(lines)


def render_json(results):
    return json.dumps(report_to_dict(results), indent=2, sort_keys=True)
