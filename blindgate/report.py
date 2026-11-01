"""The one report format, rendered as plain text and as JSON."""

from __future__ import annotations

import json


def result_to_dict(result):
    return {
        "name": result.name,
        "job": result.job,
        "verdict": result.verdict,
        "claimed_surface": list(result.surface),
        "read_set": list(result.read_set),
        "reads_in_surface": list(result.matched),
    }


def report_to_dict(results):
    checks = [result_to_dict(result) for result in results]
    summary = {}
    for result in results:
        summary[result.verdict] = summary.get(result.verdict, 0) + 1
    return {"checks": checks, "summary": summary}


def _listing(values):
    if not values:
        return ["    none"]
    return [f"    {value}" for value in values]


def render_text(results):
    lines = ["blindgate report", "================", ""]
    for result in results:
        lines.append(f"check            {result.name}")
        lines.append(f"job              {result.job}")
        lines.append(f"verdict          {result.verdict}")
        lines.append("claimed surface")
        lines.extend(_listing(result.surface))
        lines.append("read set")
        lines.extend(_listing(result.read_set))
        lines.append("reads in surface")
        lines.extend(_listing(result.matched))
        lines.append("")
    return "\n".join(lines)


def render_json(results):
    return json.dumps(report_to_dict(results), indent=2, sort_keys=True)
