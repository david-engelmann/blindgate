"""The one tracing method in slice 1, strace openat capture.

A read set is the set of paths a run opened for reading, taken from
openat lines in an strace log. Writes-only opens and failed opens are
not reads. Paths are resolved canonically, with symlinks followed,
before they are scoped to the repo root. Resolved paths inside the
root are reported relative to it. A path written inside the root
that resolves outside it is named as an outside read, and a path
that was already outside in its written form is dropped unnamed. A
trace file that is not empty and holds no recognizable strace line
is refused as not strace output. Directory changes recorded by
chdir are tracked per process, so a relative path is resolved
against the directory in force for its process when it was opened.
Directory descriptors are tracked beside that state, so a relative
path opened against a recorded descriptor is resolved against the
directory that descriptor was opened on, and an fchdir against a
recorded descriptor moves the directory in force to it. A call
strace split into an unfinished line and a resumed line is
reassembled and judged as the whole line it reads as, for the
state changing calls and for reads alike. What a clone shares
is decided by its flags. A clone whose flags carry CLONE_FILES
shares its descriptor table with its parent in this reading,
and a clone whose flags carry CLONE_FS shares its directory
state, the way the kernel holds both. A clone3 line is read
for the flags its decoded struct argument prints, inside its
braces, and shares or copies by the same rule.
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
    r'openat\(\s*([^,]+?)\s*,\s*"((?:[^"\\]|\\.)*)"\s*,\s*([A-Za-z0-9_|]+)'
)
_OPEN_RE = re.compile(
    r'^\s*(?:\d+\s+)?open\(\s*"((?:[^"\\]|\\.)*)"\s*,\s*[A-Za-z0-9_|]+'
)
_FCHDIR_RE = re.compile(r"^\s*(?:\d+\s+)?fchdir\(\s*(\d+)\s*\)")
_CLOSE_RE = re.compile(r"^\s*(?:\d+\s+)?close\(\s*(\d+)\s*\)")
_DUP_RE = re.compile(r"^\s*(?:\d+\s+)?dup\(\s*(\d+)\s*\)")
_DUP2_RE = re.compile(r"^\s*(?:\d+\s+)?dup[23]\(\s*(\d+)\s*,\s*(\d+)")
_CHDIR_RE = re.compile(
    r'^\s*(?:\d+\s+)?chdir\(\s*"((?:[^"\\]|\\.)*)"\s*\)'
)
_ASSIGN_RETURN_RE = re.compile(r"=\s*(-?(?:0x[0-9a-fA-F]+|\d+))")
_FORK_RES = (
    re.compile(
        r"^\s*(?:\d+\s+)?(clone3?|v?fork)\(.*\)\s*=\s*(\d+)\s*$"
    ),
    re.compile(
        r"^\s*(?:\d+\s+)?<\.\.\.\s+(clone3?|v?fork)\s+resumed>"
        r".*\)\s*=\s*(\d+)"
    ),
)
_PID_RE = re.compile(r"^\s*(\d+)\s+")
_UNFINISHED_MARKER = "<unfinished ...>"
_RESUMED_RE = re.compile(
    r"^\s*(?:\d+\s+)?<\.\.\.\s+([A-Za-z_][A-Za-z0-9_]*)\s+resumed>"
)
_EXIT_RE = re.compile(r"^\s*(?:\d+\s+)?\+\+\+\s+(?:exited|killed)\b")
_CALL_NAME_RE = re.compile(r"^\s*(?:\d+\s+)?([A-Za-z_][A-Za-z0-9_]*)\(")
_SPLIT_CALLS = frozenset(
    ("chdir", "fchdir", "open", "openat", "close", "dup", "dup2", "dup3")
)
_SPLIT_PREFIX_RES = {
    "chdir": re.compile(
        r'^\s*(?:\d+\s+)?chdir\(\s*"(?:[^"\\]|\\.)*"\s*$'
    ),
    "fchdir": re.compile(r"^\s*(?:\d+\s+)?fchdir\(\s*\d+\s*$"),
    "close": re.compile(r"^\s*(?:\d+\s+)?close\(\s*\d+\s*$"),
    "dup": re.compile(r"^\s*(?:\d+\s+)?dup\(\s*\d+\s*$"),
    "dup2": re.compile(r"^\s*(?:\d+\s+)?dup2\(\s*\d+\s*,\s*\d+\s*$"),
    "dup3": re.compile(
        r"^\s*(?:\d+\s+)?dup3\(\s*\d+\s*,\s*\d+"
        r"(?:\s*,\s*[A-Za-z0-9_|]+)?\s*$"
    ),
}
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


def _pid_key(line):
    """Return the process id prefix of a trace line, else None.

    Lines with no process id prefix share one bucket of their own,
    keyed by None, exactly as one unnumbered stream was treated by
    the earlier slices.
    """
    match = _PID_RE.match(line)
    return int(match.group(1)) if match is not None else None


def _kept_unfinished(line):
    """Return the call name and text of a kept unfinished line.

    An unfinished line is kept when it ends with the unfinished
    marker, names one of the split calls the parse reassembles,
    and the shape rules of that call can read the call and its
    arguments from the half that is present, exactly as they read
    them on a whole line. The kept text is the line with the
    marker cut away, ready to be joined to its resumed partner.
    Anything else returns None, and such a line then records
    nothing, exactly as an unfinished line always has.
    """
    marker_at = line.find(_UNFINISHED_MARKER)
    if marker_at < 0:
        return None
    if line[marker_at + len(_UNFINISHED_MARKER):].strip():
        return None
    prefix = line[:marker_at]
    call = _CALL_NAME_RE.match(prefix)
    if call is None:
        return None
    name = call.group(1)
    if name == "openat":
        readable = _OPENAT_RE.search(prefix) is not None
    elif name == "open":
        readable = _OPEN_RE.match(prefix) is not None
    elif name in _SPLIT_PREFIX_RES:
        readable = _SPLIT_PREFIX_RES[name].match(prefix) is not None
    else:
        return None
    if not readable:
        return None
    return name, prefix


def _spend_resumed(line, pending):
    """Return the reassembled line for a resumed line, else the line.

    A resumed line for one of the split calls spends the unfinished
    line kept under its own process and its own call name, whether
    or not the pair judges to anything, and the reassembled line
    is the kept text joined to the part of the resumed line that
    follows the resumed marker, which carries the return. A
    resumed line with nothing kept for it, and any other line,
    comes back unchanged and is read by the usual branches.
    """
    resumed = _RESUMED_RE.match(line)
    if resumed is None:
        return line
    name = resumed.group(1)
    if name not in _SPLIT_CALLS:
        return line
    prefix = pending.pop((_pid_key(line), name), None)
    if prefix is None:
        return line
    return prefix + line[resumed.end():]


_CLONE_FLAGS_RE = re.compile(r"\bflags=([A-Za-z0-9_|]+)")
_CLONE_FLAGS_CLOSED_RE = re.compile(r"\bflags=([A-Za-z0-9_|]+),")


def _sharing_from_flag_text(flag_text):
    """Return the sharing a bar joined flag list names.

    The pair is the descriptor table sharing CLONE_FILES names
    and the directory state sharing CLONE_FS names, each true
    only when the flag stands in the list as its own token.
    """
    tokens = flag_text.split("|")
    return "CLONE_FILES" in tokens, "CLONE_FS" in tokens


def _clone_line_sharing(line):
    """Return the sharing a whole clone line's flags name.

    Only a clone line carries a flags argument this parse reads.
    Any other fork call, and a clone line with no readable flags
    argument, names no sharing and returns the copy pair.
    """
    call = _CALL_NAME_RE.match(line)
    if call is None or call.group(1) != "clone":
        return False, False
    match = _CLONE_FLAGS_RE.search(line)
    if match is None:
        return False, False
    return _sharing_from_flag_text(match.group(1))


def _kept_clone_half(line):
    """Return the sharing an unfinished clone line's flags name.

    The half is kept for its flag text alone, and only when the
    flag list stands complete in the half, closed by the comma
    that follows it in the printed call, under the read whole
    principle the split lines already follow. Anything else
    returns None, and the split clone then reads as a copy.
    """
    marker_at = line.find(_UNFINISHED_MARKER)
    if marker_at < 0:
        return None
    if line[marker_at + len(_UNFINISHED_MARKER):].strip():
        return None
    prefix = line[:marker_at]
    call = _CALL_NAME_RE.match(prefix)
    if call is None or call.group(1) != "clone":
        return None
    match = _CLONE_FLAGS_CLOSED_RE.search(prefix)
    if match is None:
        return None
    return _sharing_from_flag_text(match.group(1))


_CLONE3_FLAGS_CLOSED_RE = re.compile(r"\{flags=([A-Za-z0-9_|]+)[,}]")


def _clone3_struct_span(line):
    """Return the text inside the braces of a clone3 struct.

    The struct argument is the first brace span the call opens
    with. The output struct strace prints after the arrow is a
    later span and is never the span returned, and a line whose
    first argument is a bare pointer has no span at all.
    """
    call = _CALL_NAME_RE.match(line)
    if call is None or call.group(1) != "clone3":
        return None
    rest = line[call.end():]
    arrow = rest.find("=>")
    opening = rest.find("{")
    if opening < 0 or (0 <= arrow < opening):
        return None
    depth = 0
    for index in range(opening, len(rest)):
        if rest[index] == "{":
            depth += 1
        elif rest[index] == "}":
            depth -= 1
            if depth == 0:
                return rest[opening + 1:index]
    return None


def _clone3_line_sharing(line):
    """Return the sharing a whole clone3 line's struct names.

    Only the flags field inside the braces of the struct argument
    is read, and nowhere else on the line. A clone3 line whose
    struct is not decoded, or whose span holds no flags field,
    names no sharing and returns the copy pair.
    """
    span = _clone3_struct_span(line)
    if span is None:
        return False, False
    depth = 0
    for index, char in enumerate(span):
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
        elif depth == 0:
            match = _CLONE_FLAGS_RE.match(span, index)
            if match is not None:
                return _sharing_from_flag_text(match.group(1))
    return False, False


def _fork_line_sharing(line, name):
    """Return the sharing a whole fork line names, by call name.

    A clone names its sharing in its flags argument and a clone3
    in the flags field of its struct argument. Every other fork
    call names no sharing and returns the copy pair.
    """
    if name == "clone":
        return _clone_line_sharing(line)
    if name == "clone3":
        return _clone3_line_sharing(line)
    return False, False


def _kept_clone3_half(line):
    """Return the sharing an unfinished clone3 line's flags name.

    The half is kept for its flag text alone, and only when the
    struct flags list stands complete in the half, closed by the
    comma or the closing brace that follows it in the printed
    call, under the read whole principle the split lines already
    follow. Anything else returns None, and the split clone3 then
    reads as a copy.
    """
    marker_at = line.find(_UNFINISHED_MARKER)
    if marker_at < 0:
        return None
    if line[marker_at + len(_UNFINISHED_MARKER):].strip():
        return None
    prefix = line[:marker_at]
    call = _CALL_NAME_RE.match(prefix)
    if call is None or call.group(1) != "clone3":
        return None
    opening = prefix.find("{")
    if opening < 0:
        return None
    match = _CLONE3_FLAGS_CLOSED_RE.match(prefix, opening)
    if match is None:
        return None
    return _sharing_from_flag_text(match.group(1))


def _record_opened_descriptor(fds, raw_path, base, root, fd):
    """Record what one successful open made of a descriptor number.

    When the opened path resolves to a directory standing on disk,
    the number names that directory in the map, stored as its
    canonical resolution beside whether the spelled path landed
    inside the root once its dot segments settle. Any other
    successful opening hands the number to something the map does
    not track, so any earlier entry for the number is dropped.
    """
    if raw_path:
        spelled = Path(raw_path)
        if not spelled.is_absolute():
            spelled = base / spelled
        resolved = Path(os.path.realpath(spelled))
        if resolved.is_dir():
            written = Path(os.path.normpath(spelled))
            try:
                written.relative_to(root)
                inside = True
            except ValueError:
                inside = False
            fds[fd] = (resolved, inside)
            return
    fds.pop(fd, None)


def _copy_descriptor(fds, source, target):
    """Give the target number whatever the source number names.

    A recorded source hands its directory entry to the target. An
    unrecorded source leaves the target naming the same untracked
    thing, so any earlier entry for the target is dropped.
    """
    if source in fds:
        fds[target] = fds[source]
    else:
        fds.pop(target, None)


def _canonical_scoped(raw_path, root, base=None):
    """Resolve a traced path canonically and scope it to the root.

    Returns a pair. The first item is the resolved path relative to
    the root when the path lands inside the root, else None. The
    second item is the resolved absolute path when the path is an
    outside read, else None. Canonical resolution follows symlink
    components on disk and settles dot segments, so a path is judged
    by where it actually lands. A path that resolves to the root
    itself names no file inside it and lands in neither item.
    Relative traced paths are taken against base, the directory in
    force for the process that opened them, which the parse keeps
    per process and defaults to the root.

    An outside read is a path whose written form lands inside the
    root once dot segments settle lexically, yet whose canonical
    resolution lands outside it, which is the shape of an escape
    through a symlink. The same naming covers a path spelled
    through an alias of the root itself, when the directory the
    path hangs from resolves inside the root while the path itself
    resolves outside it. A path that was already outside the root
    in its written form and hangs from a directory outside the
    root, the way system library opens are, is dropped unnamed
    exactly as the earlier slices dropped it.
    """
    if not raw_path:
        return None, None
    candidate = Path(raw_path)
    if not candidate.is_absolute():
        candidate = (root if base is None else base) / candidate
    written = Path(os.path.normpath(candidate))
    try:
        written.relative_to(root)
        written_inside = True
    except ValueError:
        written_inside = False
    resolved = Path(os.path.realpath(candidate))
    if resolved == root:
        return None, None
    if base is not None and resolved == Path(os.path.realpath(base)):
        return None, None
    try:
        return resolved.relative_to(root).as_posix(), None
    except ValueError:
        pass
    parent_resolved = Path(os.path.realpath(candidate.parent))
    try:
        parent_resolved.relative_to(root)
        parent_inside = True
    except ValueError:
        parent_inside = False
    if written_inside or parent_inside:
        return None, resolved.as_posix()
    return None, None


_STRACE_LINE_RE = re.compile(
    r"^\s*(?:\d+\s+)?"
    r"(?:<\.\.\.\s+[A-Za-z_][A-Za-z0-9_]*\s+resumed>|"
    r"[A-Za-z_][A-Za-z0-9_]*\(.*\)\s*=\s*"
    r"(?:-?\d+|0x[0-9a-fA-F]+|\?)"
    r"(?:\s+[A-Z][A-Z0-9_]*(?:\s+\(.*\))?)?)"
    r"\s*$"
)
_STRACE_MARKERS = ("<unfinished ...>", "--- ", "+++ ")


def looks_like_strace(text):
    """Return True when a trace text holds a recognizable strace line.

    A line is recognizable when it carries one of the usual strace
    markers, the unfinished marker, the signal banner, or the exit
    marker, or when the whole line is a syscall shape, an optional
    process id, a syscall name hard against its opening parenthesis,
    the call, an equals sign, and a return value, where the return
    value is a number or the question mark strace prints when a
    return is unknown, with an optional error name and message
    after it. A call shape buried inside a longer line of prose or
    source code does not make the line recognizable. An empty text
    holds no line at all and is not judged by this check.
    """
    for line in text.splitlines():
        if any(marker in line for marker in _STRACE_MARKERS):
            return True
        if _STRACE_LINE_RE.match(line):
            return True
    return False


def refuse_unshaped_trace(text, origin):
    """Raise BlindgateError when a handed over trace is not strace shaped.

    An empty text keeps the slice 1 meaning and passes. A non empty
    text with no recognizable strace line is refused with a named
    error, so a garbage file can no longer read as a genuine trace
    that happened to record no reads.
    """
    if text and not looks_like_strace(text):
        raise BlindgateError(f"trace {origin} does not look like strace output")


def parse_trace(text, root, check_shape=True):
    """Return the read set and the outside reads found in a log text.

    The read set is the sorted list of resolved paths inside the
    root. The outside reads are the sorted list of resolved absolute
    paths that landed outside the root. root is the repo root the
    paths are scoped to. When check_shape holds, a non empty text
    with no recognizable strace line is refused before any read set
    is built.

    Directory state is tracked while the lines are read. Every
    process id starts at the root, a chdir line with a recorded
    success moves the directory of its own process from that line
    onward, and the state is stored as the canonical resolution of
    the directory the process moved to. A fork the trace records
    seeds the child process with the directory its parent held at
    that line. A relative open resolves against the directory in
    force for its process at that line.

    Descriptor state is tracked beside the directory state. An
    open or openat line that returns a descriptor for a path
    resolving to a directory on disk records that directory for
    the number, a close line with a recorded success forgets the
    number, and a dup line copies an entry to its new number. A
    relative openat against a recorded descriptor resolves
    against that descriptor directory instead of the directory in
    force, and an fchdir line against a recorded descriptor moves
    the directory in force to it. A fork seeds the child with the
    state its parent held at that line, held in common with the
    parent when the flags the fork line prints carry CLONE_FILES
    for the descriptor map or CLONE_FS for the directory state,
    whether those flags stand in a clone flags argument or in
    the flags field of a decoded clone3 struct, and copied
    otherwise.

    Split lines are reassembled while the lines are read. An
    unfinished line of one of the split calls whose arguments the
    shape rules can read whole is kept per process, and a resumed
    line for the same process and call spends the kept line and is
    judged as the whole line the two halves read as, its return
    taken from the resumed half. An unfinished line that never
    resumes, a resumed line with nothing kept for it, and an
    unfinished line the shape rules cannot read whole all record
    nothing, exactly as split lines always have.
    """
    root_path = Path(root).resolve()
    if check_shape:
        refuse_unshaped_trace(text, "log")
    reads = set()
    outside = set()
    cwd_by_pid = {}
    fds_by_pid = {}
    pending = {}
    clone_halves = {}
    for raw_line in text.splitlines():
        kept = _kept_unfinished(raw_line)
        if kept is not None:
            name, prefix = kept
            pending[(_pid_key(raw_line), name)] = prefix
            continue
        clone_half = _kept_clone_half(raw_line)
        if clone_half is not None:
            clone_halves[(_pid_key(raw_line), "clone")] = clone_half
            continue
        clone3_half = _kept_clone3_half(raw_line)
        if clone3_half is not None:
            clone_halves[(_pid_key(raw_line), "clone3")] = (
                clone3_half
            )
            continue
        line = _spend_resumed(raw_line, pending)
        pid_key = _pid_key(line)
        if _EXIT_RE.match(line) is not None:
            for key in [key for key in pending if key[0] == pid_key]:
                del pending[key]
            for name in ("clone", "clone3"):
                clone_halves.pop((pid_key, name), None)
            continue
        fork_child = None
        fork_name = None
        fork_resumed = False
        for fork_index, fork_re in enumerate(_FORK_RES):
            fork_match = fork_re.search(line)
            if fork_match is not None:
                fork_name = fork_match.group(1)
                fork_child = int(fork_match.group(2))
                fork_resumed = fork_index == 1
                break
        if fork_child:
            if fork_resumed and fork_name in ("clone", "clone3"):
                kept_sharing = clone_halves.pop(
                    (pid_key, fork_name), None
                )
                share_files, share_fs = kept_sharing or (False, False)
            elif fork_resumed:
                share_files, share_fs = (False, False)
            else:
                for name in ("clone", "clone3"):
                    clone_halves.pop((pid_key, name), None)
                share_files, share_fs = _fork_line_sharing(
                    line, fork_name
                )
            parent_cell = cwd_by_pid.setdefault(pid_key, [root_path])
            if share_fs:
                cwd_by_pid[fork_child] = parent_cell
            else:
                cwd_by_pid[fork_child] = [parent_cell[0]]
            parent_fds = fds_by_pid.setdefault(pid_key, {})
            if share_files:
                fds_by_pid[fork_child] = parent_fds
            else:
                fds_by_pid[fork_child] = dict(parent_fds)
            for key in [key for key in pending if key[0] == fork_child]:
                del pending[key]
            for name in ("clone", "clone3"):
                clone_halves.pop((fork_child, name), None)
            continue
        chdir_match = _CHDIR_RE.search(line)
        if chdir_match is not None:
            returned = _ASSIGN_RETURN_RE.search(line[chdir_match.end():])
            if returned is not None and int(returned.group(1), 0) >= 0:
                target = _unescape(chdir_match.group(1))
                if target:
                    cell = cwd_by_pid.setdefault(pid_key, [root_path])
                    candidate = Path(target)
                    if not candidate.is_absolute():
                        candidate = cell[0] / candidate
                    cell[0] = Path(os.path.realpath(candidate))
            continue
        fchdir_match = _FCHDIR_RE.match(line)
        if fchdir_match is not None:
            returned = _ASSIGN_RETURN_RE.search(line[fchdir_match.end():])
            if returned is not None and int(returned.group(1), 0) >= 0:
                entry = fds_by_pid.get(pid_key, {}).get(
                    int(fchdir_match.group(1))
                )
                if entry is not None:
                    cwd_by_pid.setdefault(pid_key, [root_path])[0] = (
                        entry[0]
                    )
            continue
        close_match = _CLOSE_RE.match(line)
        if close_match is not None:
            returned = _ASSIGN_RETURN_RE.search(line[close_match.end():])
            if returned is not None and int(returned.group(1), 0) >= 0:
                fds_by_pid.get(pid_key, {}).pop(
                    int(close_match.group(1)), None
                )
            continue
        dup_match = _DUP_RE.match(line)
        if dup_match is not None:
            returned = _ASSIGN_RETURN_RE.search(line[dup_match.end():])
            if returned is not None and int(returned.group(1), 0) >= 0:
                _copy_descriptor(
                    fds_by_pid.setdefault(pid_key, {}),
                    int(dup_match.group(1)),
                    int(returned.group(1), 0),
                )
            continue
        dup2_match = _DUP2_RE.match(line)
        if dup2_match is not None:
            returned = _ASSIGN_RETURN_RE.search(line[dup2_match.end():])
            if returned is not None and int(returned.group(1), 0) >= 0:
                _copy_descriptor(
                    fds_by_pid.setdefault(pid_key, {}),
                    int(dup2_match.group(1)),
                    int(dup2_match.group(2)),
                )
            continue
        open_match = _OPEN_RE.match(line)
        if open_match is not None:
            returned = _ASSIGN_RETURN_RE.search(line[open_match.end():])
            if returned is not None and int(returned.group(1), 0) >= 0:
                _record_opened_descriptor(
                    fds_by_pid.setdefault(pid_key, {}),
                    _unescape(open_match.group(1)),
                    cwd_by_pid.setdefault(pid_key, [root_path])[0],
                    root_path,
                    int(returned.group(1), 0),
                )
            continue
        match = _OPENAT_RE.search(line)
        if match is None:
            continue
        dirfd_token = match.group(1).strip()
        raw_path = _unescape(match.group(2))
        flags = match.group(3)
        fds = fds_by_pid.setdefault(pid_key, {})
        entry = None
        if dirfd_token == "AT_FDCWD":
            base = cwd_by_pid.setdefault(pid_key, [root_path])[0]
        elif dirfd_token.isdigit() and int(dirfd_token) in fds:
            entry = fds[int(dirfd_token)]
            base = entry[0]
        else:
            base = cwd_by_pid.setdefault(pid_key, [root_path])[0]
        returned = _ASSIGN_RETURN_RE.search(line[match.end():])
        returned_value = (
            int(returned.group(1), 0) if returned is not None else None
        )
        if returned_value is not None and returned_value >= 0:
            _record_opened_descriptor(
                fds, raw_path, base, root_path, returned_value
            )
        if not _is_read(flags):
            continue
        if returned_value is None or returned_value < 0:
            continue
        relative, outside_path = _canonical_scoped(raw_path, root_path, base)
        if relative is not None:
            reads.add(relative)
        elif outside_path is not None:
            outside.add(outside_path)
        elif (
            entry is not None
            and entry[1]
            and raw_path
            and not Path(raw_path).is_absolute()
        ):
            spelled = Path(raw_path)
            if not spelled.is_absolute():
                spelled = base / spelled
            resolved = Path(os.path.realpath(spelled))
            if resolved != root_path and resolved != Path(
                os.path.realpath(base)
            ):
                try:
                    resolved.relative_to(root_path)
                except ValueError:
                    outside.add(resolved.as_posix())
    return sorted(reads), sorted(outside)


def parse_strace_log(text, root):
    """Return the sorted read set found in an strace log text.

    root is the repo root the read set is scoped to. The read set is
    one half of parse_trace, kept under its slice 1 name.
    """
    reads, _outside = parse_trace(text, root)
    return reads


def read_trace_from_log_file(path, root):
    """Return the read set and outside reads from a trace log file."""
    log_path = Path(path)
    try:
        text = log_path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        raise BlindgateError(f"cannot read trace log {log_path} ({exc})") from exc
    return parse_trace(text, root)


def read_set_from_log_file(path, root):
    reads, _outside = read_trace_from_log_file(path, root)
    return reads


def run_traced(command, root):
    """Run command under strace and return its reads and outside reads.

    The pair matches parse_trace, so a live capture names its outside
    reads exactly the way a handed over trace file does. Raises
    BlindgateError when strace is not available.
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
            ["strace", "-f", "-e", "trace=open,openat,chdir,fchdir,close,dup,dup2,dup3,clone,fork,vfork,clone3", "-o", trace_name, "--", *command],
            check=False,
        )
        text = Path(trace_name).read_text(encoding="utf-8", errors="replace")
    finally:
        Path(trace_name).unlink(missing_ok=True)
    # The capture was produced by strace itself, so the shape check
    # that guards handed over trace files does not apply to it.
    return parse_trace(text, root_path, check_shape=False)
