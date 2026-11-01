"""The one tracing method in slice 1, strace openat capture.

A read set is the set of paths a run opened for reading, taken from
openat lines in an strace log. Writes-only opens and failed opens are
not reads. Paths are reported relative to the repo root, and paths
outside the root are dropped.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

from .errors import BlindgateError

_OPENAT_RE = re.compile(
    r'openat\(\s*[^,]+,\s*"((?:[^"\\]|\\.)*)"\s*,\s*([A-Za-z0-9_|]+)'
)
_RETURN_RE = re.compile(r"\)\s*=\s*(-?\d+)")
_READ_FLAGS = ("O_RDONLY", "O_RDWR")


def _unescape(raw):
    out = []
    index = 0
    while index < len(raw):
        char = raw[index]
        if char == "\\" and index + 1 < len(raw):
            nxt = raw[index + 1]
            out.append({"n": "\n", "t": "\t", '"': '"', "\\": "\\"}.get(nxt, nxt))
            index += 2
        else:
            out.append(char)
            index += 1
    return "".join(out)


_NON_CONTENT_FLAGS = ("O_PATH", "O_DIRECTORY")


def _is_read(flags):
    parts = flags.split("|")
    if any(flag in parts for flag in _NON_CONTENT_FLAGS):
        return False
    return any(flag in parts for flag in _READ_FLAGS)


def _root_relative(raw_path, root):
    """Normalise a traced path and scope it to the root.

    Dot segments are resolved lexically before scoping, so a path
    carrying .. segments is judged by where it actually lands. A path
    that lands outside the root is dropped. Symlinks are not followed;
    the traced path itself is what is judged, and that limit is stated
    in the contract.
    """
    if not raw_path:
        return None
    if os.path.isabs(raw_path):
        normalized = os.path.normpath(raw_path)
        try:
            return Path(normalized).relative_to(root).as_posix()
        except ValueError:
            return None
    normalized = os.path.normpath(raw_path)
    if normalized in ("", "."):
        return None
    if normalized.split(os.sep)[0] == "..":
        return None
    return Path(normalized).as_posix()


def parse_strace_log(text, root):
    """Return the sorted read set found in an strace log text.

    root is the repo root the read set is scoped to.
    """
    root_path = Path(root).resolve()
    reads = set()
    for line in text.splitlines():
        match = _OPENAT_RE.search(line)
        if match is None:
            continue
        raw_path = _unescape(match.group(1))
        flags = match.group(2)
        if not _is_read(flags):
            continue
        returned = _RETURN_RE.search(line[match.end():])
        if returned is None or int(returned.group(1)) < 0:
            continue
        relative = _root_relative(raw_path, root_path)
        if relative is not None:
            reads.add(relative)
    return sorted(reads)


def read_set_from_log_file(path, root):
    log_path = Path(path)
    try:
        text = log_path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        raise BlindgateError(f"cannot read trace log {log_path} ({exc})") from exc
    return parse_strace_log(text, root)


def run_traced(command, root):
    """Run command under strace and return the read set it produced.

    Raises BlindgateError when strace is not available.
    """
    if shutil.which("strace") is None:
        raise BlindgateError(
            "strace is not available on this machine, so no read set can be captured"
        )
    if not command:
        raise BlindgateError("no command was given to run")
    root_path = Path(root).resolve()
    handle, trace_name = tempfile.mkstemp(prefix=".blindgate-trace-", dir=str(root_path))
    os.close(handle)
    try:
        subprocess.run(
            ["strace", "-f", "-e", "trace=openat", "-o", trace_name, "--", *command],
            check=False,
        )
        text = Path(trace_name).read_text(encoding="utf-8", errors="replace")
    finally:
        Path(trace_name).unlink(missing_ok=True)
    return parse_strace_log(text, root_path)
