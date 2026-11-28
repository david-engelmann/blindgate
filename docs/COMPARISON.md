# How Blindgate Compares

A CI job named `docs-check` or `security-scan` can exit `0` every single day while misconfigured globs, early-exit flags, or working-directory drift cause it to read zero files in the directory it claims to protect. Blindgate closes that blind spot at the kernel syscall boundary.

## Ecosystem Comparison

| Capability | GitHub `paths` triggers | Language coverage (`llvm-cov`, `coverage.py`) | Build sandboxes (`Bazel`, `verirun`) | `blindgate` |
| --- | --- | --- | --- | --- |
| Works across any language, linter, shell script, or binary | N/A | Single language runtime only | Requires migrating build system | **Yes (kernel `openat` trace)** |
| Detects a green CI check that never opened its claimed directory | No | Only for instrumented code | Only if undeclared inputs fail | **Yes (`unproven` / `empty read set`)** |
| Tracks `clone` / `fork` descriptor sharing and relative `openat` | N/A | N/A | Varies | **Yes (`CLONE_FILES` / `CLONE_FS` / `O_CLOEXEC`)** |
| Isolates symlink escapes outside repository `--root` | No | No | Yes | **Yes (`outside_reads`)** |
| Offline macOS inspection of Linux CI trace artifacts | N/A | Partial | Partial | **Yes (`--trace` / `--trace-for`)** |

## Sibling Boundaries in Gate Truth

- **Deadgate** proves the required check *actually executed* in CI rather than being skipped or orphaned by a job rename.
- **Finaltree** proves *which git tree* the job ran against.
- **Blindgate** proves *which files* inside that tree the running job actually opened for reading.
