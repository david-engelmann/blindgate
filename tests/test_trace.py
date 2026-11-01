import pytest

import blindgate.tracing as tracing
from blindgate.errors import BlindgateError
from blindgate.tracing import parse_strace_log, run_traced


def log_line(path, flags="O_RDONLY", result=3, pid=4321):
    return f'{pid} openat(AT_FDCWD, "{path}", {flags}) = {result}'


def test_read_opens_are_collected(tmp_path):
    log = "\n".join(
        [
            log_line("src/app.py"),
            log_line("src/data.txt", flags="O_RDWR|O_CLOEXEC", result=4),
            log_line("tests/test_app.py", flags="O_RDONLY|O_CLOEXEC", result=5),
        ]
    )
    assert parse_strace_log(log, tmp_path) == [
        "src/app.py",
        "src/data.txt",
        "tests/test_app.py",
    ]


def test_writes_only_open_is_not_a_read(tmp_path):
    log = "\n".join(
        [
            '4321 openat(AT_FDCWD, "out/report.txt", O_WRONLY|O_CREAT|O_TRUNC, 0644) = 3',
            log_line("src/app.py"),
        ]
    )
    assert parse_strace_log(log, tmp_path) == ["src/app.py"]


def test_failed_open_is_not_a_read(tmp_path):
    log = "\n".join(
        [
            '4321 openat(AT_FDCWD, "src/missing.py", O_RDONLY) = -1 ENOENT (No such file or directory)',
            log_line("src/app.py"),
        ]
    )
    assert parse_strace_log(log, tmp_path) == ["src/app.py"]


def test_paths_outside_root_are_dropped(tmp_path):
    log = "\n".join(
        [
            log_line("/etc/hostname"),
            log_line("/usr/lib/python3.12/os.py"),
            log_line("src/app.py"),
        ]
    )
    assert parse_strace_log(log, tmp_path) == ["src/app.py"]


def test_paths_inside_root_become_relative(tmp_path):
    inside = tmp_path / "docs" / "guide.md"
    log = log_line(str(inside))
    assert parse_strace_log(log, tmp_path) == ["docs/guide.md"]


def test_unrelated_syscalls_are_ignored(tmp_path):
    log = "\n".join(
        [
            '4321 read(3, "payload", 7) = 7',
            '4321 execve("/bin/true", ["/bin/true"], 0x0) = 0',
            log_line("src/app.py"),
        ]
    )
    assert parse_strace_log(log, tmp_path) == ["src/app.py"]


def test_run_command_without_strace_raises_a_clear_error(tmp_path, monkeypatch):
    monkeypatch.setattr(tracing.shutil, "which", lambda name: None)
    with pytest.raises(BlindgateError) as excinfo:
        run_traced(["cat", "anything.txt"], tmp_path)
    assert "strace is not available" in str(excinfo.value)
