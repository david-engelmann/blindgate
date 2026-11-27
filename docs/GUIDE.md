# Blindgate Operator Guide

Blindgate asks what a run read. Given a GitHub Actions workflow file, a surface configuration file, and either pre-recorded Linux `strace` logs or a live command run under `strace`, Blindgate reconstructs every file opened for reading inside the repository root and compares that read set against the surface each check claims to certify.

## Installation

### Building from Source

With a Rust 1.87 or newer toolchain installed, build and install directly from the repository.

```sh
cargo install --git https://github.com/david-engelmann/blindgate --locked
```

### Prebuilt Release Archive

For releases cut from `v0.16.0` onward on Linux (`x86_64`, `aarch64`) and macOS (`aarch64`, `x86_64`), the installer downloads the matching GitHub Release archive, verifies its SHA-256 digest, and atomically installs `blindgate` into `$HOME/.local/bin` (or `$BLINDGATE_INSTALL_DIR`).

```sh
curl -fsSL https://raw.githubusercontent.com/david-engelmann/blindgate/main/install.sh | sh
```

Pin a specific release tag carrying release archives by setting `BLINDGATE_VERSION` (for example `BLINDGATE_VERSION=v0.16.0`). Because `install.sh` does not edit shell profile files, ensure `$HOME/.local/bin` (or your custom `$BLINDGATE_INSTALL_DIR`) stands on `PATH` in your shell (`export PATH="$HOME/.local/bin:$PATH"`).

## Platform Support and Capturing `strace` Logs

- **Offline trace analysis (`--trace` and `--trace-for`)** Runs on both Linux and macOS without requiring `strace` locally, making it straightforward to download trace artifacts from Linux CI runners and inspect them on a developer workstation.
- **Live capture (`--run`)** Requires Linux `strace` on `PATH` and writable access to `--root` (where Blindgate creates and removes a temporary `.blindgate-trace-*` capture file during execution).

When recording a trace file yourself inside a Linux CI job rather than invoking `--run`, always include `-f` (follow forked and cloned children) and the complete file, directory, descriptor, and process syscall filter so relative `openat` paths, duplicated directory descriptors, `chdir`/`fchdir`, and `O_CLOEXEC` closings across `execve` are tracked accurately.

```sh
strace -f -e trace=open,openat,openat2,chdir,fchdir,close,close_range,dup,dup2,dup3,fcntl,clone,fork,vfork,clone3,execve,execveat -o trace.log -- pytest -q
```

## Command Line Interface

```
blindgate [-h | --help] --workflow WORKFLOW --config CONFIG
          [--trace TRACE] [--trace-for JOB=PATH] [--json] [--root ROOT]
          [--run ...]
```

Options

- `-h`, `--help` Print help text to standard output and exit `0`.
- `--workflow WORKFLOW` Path to the GitHub Actions workflow YAML file declaring the jobs.
- `--config CONFIG` Path to the Blindgate configuration file (`.yml`, `.yaml`, or `.json`) declaring the checks and claimed surfaces.
- `--trace TRACE` Shared `strace` log file used by any check that carries neither a per-check `trace` field nor a `--trace-for` override.
- `--trace-for JOB=PATH` Repeatable per-job `strace` log override (`relative` paths resolve against the directory holding `CONFIG`).
- `--json` Render the report as sorted-key JSON instead of human-readable text.
- `--root ROOT` Repository root directory against which traced paths are canonically scoped (defaults to the current working directory).
- `--run ...` Command words executed live under `strace` for checks using the shared source. Spaced `--run` consumes all remaining words to the end of the command line, so place `--json` and `--root` before `--run`. Because bare `--` anywhere after `--run` terminates the remainder and turns `--` and trailing words into unrecognized arguments, and because ambiguity prescanning rejects child flags before `--` whose prefix matches multiple `blindgate` flags (`--t` through `--trac` matching `--trace` and `--trace-for`, or `--r` matching `--run` and `--root`), wrap commands carrying bare `--` or those ambiguous prefixes inside `sh -c "..."` (for example `--run sh -c "myrunner --r fast"`).

Exit codes

- `0` Report rendered (or `--help` printed). Inspect check `verdict` values (`proven`, `unproven`, `empty`) in CI to fail builds when a gate misses its claimed surface.
- `2` Argument refusal, missing or malformed config/workflow/trace file, unknown workflow job, or missing `strace` under `--run`.

## Configuration File (`blindgate.yml`)

A configuration file is a YAML or JSON mapping carrying a non-empty `checks` list. Each entry declares

- `name` Non-empty human label for the check.
- `job` Job identifier that must exist under `jobs` in `--workflow`.
- `surface` Non-empty list of glob patterns or directory prefixes the check claims to cover.
- `trace` Optional path to a dedicated trace log for this check (resolved relative to the configuration file's parent directory when relative).

Example `blindgate.yml`

```yaml
checks:
  - name: unit tests
    job: unit-tests
    trace: traces/unit-tests.log
    surface:
      - "src/**"
      - "tests/**"
  - name: docs check
    job: docs-check
    trace: traces/docs-check.log
    surface:
      - "docs/*.md"
```

## Surface Matching and Verdicts

- **Glob patterns** Any surface entry containing `*`, `?`, or `[` is matched whole-string with case-sensitive `fnmatch` semantics (`*` crosses `/`, `?` matches one character, `[...]` matches character sets with leading `!` negation).
- **Directory prefixes** A surface entry without glob characters has trailing slashes stripped and matches either the exact relative path or any path beneath `<prefix>/`.
- **Canonical containment and `outside_reads`** Successful read-bearing opens (`O_RDONLY` or `O_RDWR`, excluding `O_PATH` and `O_DIRECTORY` metadata-only opens and failed syscalls) are resolved canonically against `--root`. Paths whose written form already lies outside `--root` (such as `/etc/hostname` or `/usr/lib/...`) are dropped unnamed. Paths that resolve inside `--root` enter `read_set`, while paths whose written form (after lexical dot-segment settlement) or inside-opened directory descriptor started inside `--root` and escaped outside `--root` through canonical symlink resolution enter `outside_reads` and never satisfy a claimed surface.
- **Verdicts**
  - `empty` `read_set` is empty (the run opened zero files inside `--root`).
  - `unproven` `read_set` is non-empty, and zero read files matched `surface`.
  - `proven` At least one file in `read_set` matched `surface`.

## Emitting `blindgate-evidence/1` from Rust

Downstream Gate Truth tools such as `tameshi` consume the pinned `blindgate-evidence/1` JSON format. Library callers can generate `blindgate-evidence/1` directly via `blindgate::render_evidence`.

```rust
use std::path::Path;
use blindgate::{analyse, load_config, read_set_from_log_file, render_evidence};

let checks = load_config(Path::new("examples/blindgate.yml"))?;
let reads = read_set_from_log_file(Path::new("examples/trace.log"), Path::new("examples"))?;
let results = analyse(&checks, &reads);
let evidence_json = render_evidence(&results);
```

## Self-Contained GitHub Actions Workflow Example

Save the workflow below at `.github/workflows/blindgate-audit.yml` in a repository with an existing `pytest` suite covering `src/**` and `tests/**`. It installs Python, `strace`, and your project's test dependencies, writes a matching single-job surface configuration, captures the `pytest` trace, and verifies that the check is `proven`.

```yaml
name: blindgate-audit

on:
  pull_request:

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: "3.12"
      - name: Install strace and pytest
        run: |
          sudo apt-get update && sudo apt-get install -y strace
          python -m pip install pytest
      - name: Write surface config for unit-tests job
        run: |
          cat > /tmp/blindgate-ci.yml <<'YAML'
          checks:
            - name: unit tests
              job: unit-tests
              surface:
                - "src/**"
                - "tests/**"
          YAML
      - name: Run tests under strace
        run: |
          strace -f -e trace=open,openat,openat2,chdir,fchdir,close,close_range,dup,dup2,dup3,fcntl,clone,fork,vfork,clone3,execve,execveat -o /tmp/unit.trace -- python -m pytest -q
      - name: Install blindgate
        run: cargo install --git https://github.com/david-engelmann/blindgate --locked
      - name: Verify claimed surface was read
        run: |
          blindgate --workflow .github/workflows/blindgate-audit.yml --config /tmp/blindgate-ci.yml --trace-for unit-tests=/tmp/unit.trace --json > /tmp/blindgate.json
          python - <<'PY'
          import json, pathlib, sys
          doc = json.loads(pathlib.Path("/tmp/blindgate.json").read_text())
          bad = [c["name"] for c in doc["checks"] if c["verdict"] != "proven"]
          if bad:
              print("Unproven or empty checks", bad)
              sys.exit(1)
          PY
```
