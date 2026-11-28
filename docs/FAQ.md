# Frequently Asked Questions

## Does Blindgate require Linux?

Only `--run` (live `strace` capture) requires Linux `strace` on `PATH`. Offline trace analysis (`--trace` and `--trace-for`) is 100% pure Rust and runs natively on both Linux (`x86_64`, `aarch64`) and macOS (`aarch64`, `x86_64`). Because traced paths are resolved canonically against `--root` on the analysis machine, analyze offline traces against a matching `--root` directory layout (see the canonical containment rules in [GUIDE.md](GUIDE.md)) so traced reads do not resolve outside `--root` and report `empty read set`.

## Why does `Cargo.toml` specify `rust-version = "1.87"`?

Blindgate's pure-Rust YAML and conformance modules use stabilized Rust 1.87 standard-library integer methods (`is_multiple_of`), verified clean under `cargo clippy -- -D clippy::incompatible_msrv`.

## Do `stat`, `access`, or directory listings count as reading a file?

No. Blindgate intentionally counts only successful read-bearing file descriptor opens (`O_RDONLY` or `O_RDWR` without `O_PATH` or `O_DIRECTORY`). Listing a directory or checking `stat` metadata on `docs/guide.md` without opening its bytes never earns a `proven` verdict.

## What happens if a symlink inside my repo points to `/etc/passwd`?

Blindgate resolves paths canonically against `--root`. Because the written path started inside `--root` and escaped outside `--root` after symlink resolution, `/etc/passwd` is recorded under `outside_reads` rather than `read_set` and cannot satisfy any claimed surface pattern.

## How do I pass child command flags like `--root` or `--` under `--run`?

Wrap the child command in `sh -c "..."` (for example `blindgate --workflow ci.yml --config blindgate.yml --run sh -c "pytest -- --slow"`) so bare `--` and ambiguous flag prefixes stay inside the single shell command argument.
