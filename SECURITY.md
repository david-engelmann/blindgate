# Security Policy

## Supported Versions

Security fixes land on the latest `0.15.x` and subsequent `0.x` release line on `main`. Earlier tags do not receive backported patches.

## Security Model

Offline trace analysis (`--trace` and `--trace-for`) parses `strace` logs, workflow YAML, and configuration documents with pure-Rust bounded-depth readers without executing traced binaries or making network calls, and canonicalizes paths against `--root` so symlinks escaping the repository root land in `outside_reads` rather than satisfying claimed surfaces. Live capture (`--run`) executes the supplied command under Linux `strace` as the invoking OS user, so untrusted commands must only be traced inside an isolated container or VM. Never commit real host paths or production `strace` captures to the repository.

## Reporting a Vulnerability

Do not open a public issue for a suspected security vulnerability. Report vulnerabilities privately through [GitHub Security Advisories](https://github.com/david-engelmann/blindgate/security/advisories/new) with synthetic traces and configuration inputs only, naming the affected version and the relevant section of `CONTRACT.md`. Every report receives an initial review within seven days, and confirmed fixes ship with a regression pin in the test suite before public disclosure.
