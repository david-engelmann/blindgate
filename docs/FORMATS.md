# Blindgate Wire Formats

Blindgate defines three structured schemas, the `blindgate.yml` configuration input, the CLI `--json` report, and the pinned `blindgate-evidence/1` downstream evidence format consumed by Gate Truth aggregators such as `tameshi`.

## 1. Configuration Input (`blindgate.yml` / `.json`)

Top-level mapping carrying `checks`, a non-empty list of check objects

- `name` Non-empty string check title.
- `job` Non-empty string matching a job key under `jobs` in `--workflow`.
- `surface` Non-empty list of non-empty glob or directory-prefix strings.
- `trace` Optional string path to a per-check `strace` log (resolved relative to the directory containing the config file when relative).

## 2. CLI JSON Report (`--json`)

Sorted-key, 2-space-indented JSON object carrying `checks` and `summary`

- `checks` Ordered array of check result objects
  - `claimed_surface` Array of configured surface strings.
  - `job` Workflow job identifier.
  - `name` Check display name.
  - `outside_reads` Optional array of symlink-escaped target paths (present across all checks whenever at least one check recorded an outside read).
  - `read_set` Sorted deduplicated array of `--root`-relative files opened for reading.
  - `reads_in_surface` Subset of `read_set` matching `claimed_surface`.
  - `trace_source` Optional `{ "detail", "kind", "text" }` object (or `null`) present when trace provenance is recorded.
  - `verdict` One of `"proven"`, `"unproven"`, or `"empty read set"`.
- `summary` Object mapping each observed verdict string to its integer check count.

## 3. Pinned Evidence Schema (`blindgate-evidence/1`)

Emitted by `blindgate::render_evidence(&results)` with one trace entry per check per claimed surface pattern

```json
{
  "format": "blindgate-evidence/1",
  "traces": [
    {
      "check": "unit tests",
      "detail": "shared trace examples/trace.log",
      "observation": "read",
      "surface": "src/**"
    },
    {
      "check": "unit tests",
      "detail": "shared trace examples/trace.log",
      "observation": "read",
      "surface": "tests/**"
    },
    {
      "check": "docs check",
      "detail": "shared trace examples/trace.log",
      "observation": "not read",
      "surface": "docs/*.md"
    }
  ]
}
```
