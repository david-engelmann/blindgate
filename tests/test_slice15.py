"""Tests for the slice 15 obligations in CONTRACT.md.

Slice 15 reads the bracketed process id prefix strace prints
on its standard error stream everywhere the parse reads the
bare number, so a line in either spelling is judged with the
state of the process it names. The limits the probes forced
stand pinned as stated, the unprefixed superseded banner and
the attach note lines. The bracketed shapes below are the
shapes the slice 15 probes captured from strace 6.8 on this
machine, distilled to synthetic lines.

Every fixture here is synthetic and written for this test
file. The live capture tests run small python programs under
strace with the trace taken from the error stream and are
guarded on strace availability.
"""

import re
import shutil
import subprocess
import sys

import pytest

from blindgate.errors import BlindgateError
from blindgate.tracing import parse_trace

_TRACE_SET = (
    "open,openat,chdir,fchdir,close,dup,dup2,dup3,fcntl,"
    "clone,fork,vfork,clone3,execve,execveat"
)


def make_project(tmp_path):
    project = tmp_path / "project"
    (project / "docs").mkdir(parents=True)
    (project / "docs" / "guide.md").write_text(
        "the guide\n", encoding="utf-8"
    )
    (project / "src").mkdir()
    (project / "src" / "app.py").write_text("x = 1\n", encoding="utf-8")
    (project / "guide.md").write_text("root guide\n", encoding="utf-8")
    return project


def test_bracketed_openat_line_is_judged_as_a_read(tmp_path):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            '[pid 4242] openat(AT_FDCWD, "docs/guide.md", O_RDONLY) = 3',
            '[pid 4242] openat(AT_FDCWD, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_bracketed_chdir_moves_only_its_own_process(tmp_path):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            '[pid 4242] chdir("docs") = 0',
            '[pid 4242] openat(AT_FDCWD, "guide.md", O_RDONLY) = 3',
            '5151 openat(AT_FDCWD, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_bracketed_open_records_its_descriptor_under_its_own_number(
    tmp_path,
):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            '[pid 4242] openat(AT_FDCWD, "docs", '
            "O_RDONLY|O_DIRECTORY) = 7",
            '[pid 5151] openat(AT_FDCWD, "src", '
            "O_RDONLY|O_DIRECTORY) = 7",
            '[pid 4242] openat(7, "guide.md", O_RDONLY) = 8',
            '[pid 5151] openat(7, "app.py", O_RDONLY) = 9',
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_bracketed_text_inside_a_quoted_path_is_data(tmp_path):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            '[pid 4242] openat(AT_FDCWD, "docs", '
            "O_RDONLY|O_DIRECTORY) = 7",
            '4242 openat(7, "guide.md", O_RDONLY) = 8',
            '[pid 9] chdir("src") = 0',
            '[pid 4242] openat(AT_FDCWD, "[pid 9] guide.md", '
            "O_RDONLY) = 10",
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["[pid 9] guide.md", "docs/guide.md"]
    assert outside == []


def test_bare_and_bracketed_lines_hold_one_state_for_one_process(
    tmp_path,
):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            '4242 openat(AT_FDCWD, "docs", '
            "O_RDONLY|O_DIRECTORY) = 7",
            '[pid 4242] openat(7, "guide.md", O_RDONLY) = 8',
            '[pid 4242] chdir("src") = 0',
            '4242 openat(AT_FDCWD, "app.py", O_RDONLY) = 9',
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["docs/guide.md", "src/app.py"]
    assert outside == []


def test_bracketed_split_openat_pair_is_reassembled(tmp_path):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            "[pid 4242] openat(AT_FDCWD, \"docs/guide.md\", "
            "O_RDONLY <unfinished ...>",
            '5151 openat(AT_FDCWD, "guide.md", O_RDONLY) = 3',
            "[pid 4242] <... openat resumed>) = 4",
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_bracketed_split_fcntl_dupfd_pair_creates_an_entry(tmp_path):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            '[pid 4242] openat(AT_FDCWD, "docs", '
            "O_RDONLY|O_DIRECTORY|O_CLOEXEC) = 5",
            "[pid 4242] fcntl(5, F_DUPFD, 0 <unfinished ...>",
            "[pid 4242] <... fcntl resumed>) = 9",
            '[pid 4242] execve("/usr/bin/reader", ["reader"], '
            "0x7ffd48416d98 /* 54 vars */) = 0",
            '[pid 4242] openat(9, "guide.md", O_RDONLY) = 10',
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["docs/guide.md"]
    assert outside == []


def test_bracketed_clone_line_seeds_its_child_with_the_sharing_its_flags_name(
    tmp_path,
):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            '[pid 4242] openat(AT_FDCWD, "docs", '
            "O_RDONLY|O_DIRECTORY) = 7",
            "[pid 4242] clone(child_stack=0x7f0, "
            "flags=CLONE_VM|CLONE_FS|CLONE_FILES|CLONE_SIGHAND|"
            "CLONE_THREAD, child_tidptr=0x7f1) = 5151",
            "[pid 5151] close(7) = 0",
            '[pid 4242] openat(7, "guide.md", O_RDONLY) = 8',
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_bracketed_exit_line_drops_the_halves_kept_under_its_process(
    tmp_path,
):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            "[pid 4242] openat(AT_FDCWD, \"docs/guide.md\", "
            "O_RDONLY <unfinished ...>",
            "[pid 4242] +++ exited with 0 +++",
            "[pid 4242] <... openat resumed>) = 4",
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == []
    assert outside == []


def test_bracketed_pid_changed_half_completes_under_a_bare_banner(
    tmp_path,
):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            '4243 openat(AT_FDCWD, "docs", '
            "O_RDONLY|O_DIRECTORY|O_CLOEXEC) = 5",
            '4243 openat(AT_FDCWD, "src", '
            "O_RDONLY|O_DIRECTORY) = 6",
            '[pid 4243] execve("/usr/bin/reader", ["reader"], '
            "0x7ffd48416d98 /* 0 vars */ "
            "<pid changed to 4242 ...>",
            "4242 +++ superseded by execve in pid 4243 +++",
            "4242 <... execve resumed>) = 0",
            '4242 openat(5, "guide.md", O_RDONLY) = 7',
            '4242 openat(6, "app.py", O_RDONLY) = 8',
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["guide.md", "src/app.py"]
    assert outside == []


def test_bracketed_only_text_passes_the_shape_check(tmp_path):
    project = make_project(tmp_path)
    text = '[pid 4242] openat(AT_FDCWD, "guide.md", O_RDONLY) = 3'
    reads, outside = parse_trace(text, project)
    assert reads == ["guide.md"]
    assert outside == []


def test_text_holding_only_attach_notes_is_refused(tmp_path):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            "strace: Process 4242 attached",
            "strace: Process 5151 attached",
        ]
    )
    with pytest.raises(BlindgateError):
        parse_trace(text, project)


def test_bracketed_garbage_is_refused(tmp_path):
    project = make_project(tmp_path)
    text = "[pid 4242] this is not a syscall line at all"
    with pytest.raises(BlindgateError):
        parse_trace(text, project)


def test_unprefixed_lines_keep_their_one_bucket_beside_bracketed_lines(
    tmp_path,
):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            'chdir("docs") = 0',
            'openat(AT_FDCWD, "guide.md", O_RDONLY) = 3',
            '[pid 4242] openat(AT_FDCWD, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_unprefixed_banner_completes_no_exec(tmp_path):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            '4243 openat(AT_FDCWD, "docs", '
            "O_RDONLY|O_DIRECTORY|O_CLOEXEC) = 5",
            '4243 openat(AT_FDCWD, "src", '
            "O_RDONLY|O_DIRECTORY) = 6",
            '4243 execve("/usr/bin/reader", ["reader"], '
            "0x7ffd48416d98 /* 0 vars */ "
            "<pid changed to 4242 ...>",
            "+++ superseded by execve in pid 4243 +++",
            "<... execve resumed>) = 0",
            '4242 openat(6, "app.py", O_RDONLY) = 7',
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["app.py"]
    assert outside == []


def test_attach_note_standing_alone_records_nothing(tmp_path):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            '4242 openat(AT_FDCWD, "docs/guide.md", O_RDONLY) = 3',
            "strace: Process 5151 attached",
            '4242 openat(AT_FDCWD, "guide.md", O_RDONLY) = 4',
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["docs/guide.md", "guide.md"]
    assert outside == []


def test_note_split_clone_line_seeds_no_child(tmp_path):
    project = make_project(tmp_path)
    text = "\n".join(
        [
            '[pid 4242] openat(AT_FDCWD, "docs", '
            "O_RDONLY|O_DIRECTORY) = 7",
            "clone3({flags=CLONE_VM|CLONE_FS|CLONE_FILES|"
            "CLONE_SIGHAND|CLONE_THREAD, exit_signal=0, "
            "stack=0x7f0, stack_size=0x9000}"
            "strace: Process 5151 attached",
            " => {parent_tid=[5151]}, 88) = 5151",
            '[pid 5151] openat(7, "guide.md", O_RDONLY) = 8',
        ]
    )
    reads, outside = parse_trace(text, project)
    assert reads == ["guide.md"]
    assert outside == []


def capture_error_stream(command, cwd):
    """Run a command under strace with the trace on the error stream."""
    result = subprocess.run(
        [
            "strace",
            "-f",
            "-e",
            f"trace={_TRACE_SET}",
            "--",
            *command,
        ],
        capture_output=True,
        text=True,
        check=False,
        cwd=str(cwd),
    )
    return result.stderr


def test_live_threaded_capture_from_the_error_stream_reads_its_workers(
    tmp_path,
):
    if shutil.which("strace") is None:
        pytest.skip("strace is not available")
    project = make_project(tmp_path)
    program = (
        "import os, threading\n"
        "barrier = threading.Barrier(3)\n"
        "def worker():\n"
        "    barrier.wait()\n"
        "    fd = os.open('src', os.O_RDONLY | os.O_DIRECTORY)\n"
        "    for _ in range(50):\n"
        "        held = os.open('app.py', os.O_RDONLY, dir_fd=fd)\n"
        "        os.close(held)\n"
        "    os.close(fd)\n"
        "def churner():\n"
        "    barrier.wait()\n"
        "    for _ in range(50):\n"
        "        held = os.open('guide.md', os.O_RDONLY)\n"
        "        os.close(held)\n"
        "threads = [threading.Thread(target=worker), "
        "threading.Thread(target=churner)]\n"
        "for thread in threads:\n"
        "    thread.start()\n"
        "barrier.wait()\n"
        "fd = os.open('docs', os.O_RDONLY | os.O_DIRECTORY)\n"
        "for _ in range(50):\n"
        "    held = os.open('guide.md', os.O_RDONLY, dir_fd=fd)\n"
        "    os.close(held)\n"
        "os.close(fd)\n"
        "for thread in threads:\n"
        "    thread.join()\n"
    )
    text = capture_error_stream(
        [sys.executable, "-c", program], project
    )
    assert "[pid " in text
    reads, outside = parse_trace(text, project)
    assert "docs/guide.md" in reads
    assert "src/app.py" in reads
    assert "guide.md" in reads


def test_live_churn_capture_from_the_error_stream_holds_bracketed_split_pairs(
    tmp_path,
):
    if shutil.which("strace") is None:
        pytest.skip("strace is not available")
    project = make_project(tmp_path)
    program = (
        "import os, threading\n"
        "barrier = threading.Barrier(5)\n"
        "def worker():\n"
        "    barrier.wait()\n"
        "    for _ in range(200):\n"
        "        held = os.open('docs/guide.md', os.O_RDONLY)\n"
        "        os.close(held)\n"
        "        held = os.open('guide.md', os.O_RDONLY)\n"
        "        os.close(held)\n"
        "threads = [threading.Thread(target=worker) "
        "for _ in range(4)]\n"
        "for thread in threads:\n"
        "    thread.start()\n"
        "barrier.wait()\n"
        "for thread in threads:\n"
        "    thread.join()\n"
    )
    text = capture_error_stream(
        [sys.executable, "-c", program], project
    )
    assert "[pid " in text
    assert "<unfinished ...>" in text
    assert re.search(
        r"\[pid +\d+\] <\.\.\. openat resumed>", text
    ) is not None
    reads, outside = parse_trace(text, project)
    assert "docs/guide.md" in reads
    assert "guide.md" in reads


def test_space_padded_bracket_numbers_are_the_probed_spelling(
    tmp_path,
):
    project = make_project(tmp_path)
    for prefix in ("[pid  4242] ", "[pid     37] "):
        number = prefix.split()[1].rstrip("]")
        text = "\n".join(
            [
                prefix + 'chdir("docs") = 0',
                prefix + 'openat(AT_FDCWD, "guide.md", '
                "O_RDONLY) = 3",
                '5151 openat(AT_FDCWD, "guide.md", O_RDONLY) = 4',
            ]
        )
        reads, outside = parse_trace(text, project)
        assert reads == ["docs/guide.md", "guide.md"], number
        assert outside == []


def test_tab_and_leading_space_bracket_spellings_are_not_the_probed_form(
    tmp_path,
):
    project = make_project(tmp_path)
    padded = [
        '[pid\t4242] openat(AT_FDCWD, "guide.md", O_RDONLY) = 3',
        ' [pid 4242] openat(AT_FDCWD, "guide.md", O_RDONLY) = 3',
    ]
    for line in padded:
        text = "\n".join(
            [
                '[pid 4242] chdir("docs") = 0',
                line,
                '[pid 4242] openat(AT_FDCWD, "guide.md", '
                "O_RDONLY) = 4",
            ]
        )
        reads, outside = parse_trace(text, project)
        assert reads == ["docs/guide.md", "guide.md"]
        assert outside == []
