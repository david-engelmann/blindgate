# Changelog

All notable changes to blindgate are recorded in this file.

## Unreleased

- Align `Cargo.toml` package metadata (`version`, `repository`, `documentation`, `readme`, `keywords`, `categories`, `rust-version = "1.87"`) and `.crate` archive `exclude` rules.
- Add operator manual `docs/GUIDE.md`, CI `strace` capture documentation in `README.md`, checksum-verifying `install.sh`, `SECURITY.md`, `CONTRIBUTING.md`, `deny.toml`, and Linux/macOS GitHub Release workflow `.github/workflows/release.yml`.

## 0.15.0 on 2026-11-18

- Complete slice 26 across 30 closed verification rounds with the pure-Rust `strace` parser, per-process descriptor and directory tracking across `clone`/`fork`/`vfork`/`clone3` and `execve`, canonical `--root` symlink containment, PyYAML-arbitrated YAML subset config and workflow readers, and pinned `blindgate-evidence/1` emitter.
