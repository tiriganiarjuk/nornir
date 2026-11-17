# Saga Design — Quality Truth Recorder

Saga examines a source file with the quality instruments and records
everything they found. It filters nothing and decides nothing: policy is syn's
(`cli/syn_cli/SYN_DESIGN.md`). Saga is the one writer of `.qa` sidecars.

```
source file  →  saga (instruments)  →  sanity report  →  .qa sidecar  →  syn, svalinn
```

`saga --help` is the flag reference. This document is the model behind the flags.

---

## What saga examines

A file's kind is read from its name, by the extension after the last dot.
`saga_core::SourceKind` is the one classifier; saga, syn and the write hook all
ask it.

| Kind | Extension | Instruments |
|---|---|---|
| Python | `py` | gleipnir, ruff, basedpyright |
| Rust | `rs` | gleipnir |
| Svelte | `svelte` | gleipnir |

Gleipnir is linked into saga as a library (`gleipnir_core`) and called
in-process. Ruff and basedpyright are run as subprocesses, each with the
configuration compiled into saga (`capability/saga_runner/ruff.toml`,
`pyrightconfig.json`), which saga writes to its directory under the system
temporary directory before the run.

basedpyright checks a file in the environment of the project it belongs to: the
interpreter `.venv/bin/python` in the nearest directory above the file that holds
both it and a `pyproject.toml`, found by the path the file is known by. A file
with no such directory above it is checked against no environment, and every
import it makes outside the standard library is unresolved.

On a file of no kind, no instrument runs, and the report carries the fault
`source_not_examinable`.

### Faults

An instrument that could not do its work on a file — it could not be started,
its output could not be used, the source could not be read — is a **fault** in
the report, beside the issues the other instruments found. A report with a
fault says the file is not known to be clean. Faults carry their failure mode
and class from the error registry, `core/failure_mode_core/failure_modes.toml`.

---

## The sidecar

The report for `dir/name.ext` is written to `dir/.name.ext.qa`: a hidden file
beside its source. `saga_core::qa_path` and `saga_core::source_path_from_qa`
are the two directions of that naming.

The sidecar's shape is `schemas/tools/sanity-report.schema.json`. A report is
validated against it before it is written, and a sidecar is validated against
it when it is loaded.

### The content hash, the examiner and the four states

A report records two things about its own making:

- `content_hash` — the sha256 of the source as it was examined;
- `examined_by` — the fingerprint of what examined it. Under default routing
  it is `saga_runner::examiner_fingerprint()`, one hash over:

  | Part | What it covers |
  |---|---|
  | gleipnir's check logic | `gleipnir_core::LOGIC_FINGERPRINT`, compiled by gleipnir's build script from every file under its `src/`, from `gleipnir_matrix.toml`, `gleipnir_messages.toml` and `gleipnir_statistics.toml`, from `deploy_categories.toml` (it fills the `{readers}` slot of the message texts), and from the name and version of every `tree-sitter*` package in the workspace `Cargo.lock` |
  | saga's own code | every file under `capability/saga_runner/src/`, compiled in |
  | the routing list | `v1_legacy_projects.toml`, compiled in |
  | the instrument configurations | `ruff.toml` and `pyrightconfig.json`, compiled in |
  | the installed instruments | the first line of `ruff --version` and of `basedpyright --version`, asked once per process; an instrument that cannot be asked is recorded as `ruff:unavailable` or `basedpyright:unavailable` |

  A report examined under `--test v1` or `--test v2` records the fingerprint
  with `+forced-v1` or `+forced-v2` appended: a forced run is another examiner
  than the default one.

From the two, `saga_runner::sidecar_state(source file)` says how the sidecar
stands to the source and to saga now. It is the one definition of sidecar
currency, used by saga and by syn.

| State | Meaning |
|---|---|
| `missing` | the source has no sidecar |
| `current` | the sidecar loads, its examiner is this saga's, and its hash is the source's hash now |
| `stale` | the sidecar loads, and its examiner is another than this saga's or it records none; or its hash differs from the source's hash now, or it records no hash, or the source cannot be hashed |
| `unreadable` | the sidecar is there and does not load |

The hash follows the source's content, and the examiner follows the checks: a
saga built from changed check logic, data files, grammars, saga code, routing
list or instrument configuration, and a saga that finds another ruff or
basedpyright installed, finds every sidecar stale, and `saga <dir>` writes each
of them with the checks as they now are. A sidecar written under `--test` is
stale to a plain `saga <dir>`, which writes it under default routing.

---

## Modes

The path decides the mode: a file or a directory.

### File mode

- `saga <file>` examines the file and prints the report as JSON on stdout. A
  file that is not there, and a stdin that cannot be read, are the failure
  `file_unreadable`.
- `--sidecar` also writes the sidecar. A sidecar that cannot be written is a
  failure.
- A report that cannot be written as JSON is the failure
  `sanity_report_invalid`.
- `--stdin` examines content read from stdin under the path's name: the
  content is held in a temporary file, and gleipnir applies its rules by the
  path the file is known by.
- `--project-dir` names the root that the report's `relative_path` is taken
  from; without it, the relative path is the last three components.

### Directory mode

`saga <dir>` walks the tree — hidden directories and `__pycache__`,
`node_modules`, `venv` and `target` are never entered — and takes every source
of the kinds `--kind` names. It then:

1. removes orphaned sidecars: each sidecar whose source the listing shows
   absent. A tree that cannot be listed whole is a failure, and nothing is
   removed;
2. brings each source's sidecar up to date, by its state:

| State | `saga <dir>` | `saga <dir> --force` |
|---|---|---|
| `missing` | written — *generated* | written — *generated* |
| `stale`, `unreadable` | replaced — *refreshed* | replaced — *refreshed* |
| `current` | left — *current* | replaced — *refreshed* |

The summary line on stderr counts generated, refreshed, current, failed
(a sidecar that could not be written) and not fully examined (a report with a
fault).

### `--orphans`

`saga <dir> --orphans` does step 1 of directory mode alone: it removes each
sidecar whose source the listing shows absent, and examines nothing. A file
deleted outside the write hook leaves its sidecar behind, and this clears it
without re-examining the tree. A tree that cannot be listed whole is a failure,
and nothing is removed; a sidecar that could not be removed fails the run. It is
refused together with `--force`, `--check` and `--strip`.

### `--check`

`saga <dir> --check` reports and changes nothing on disk: no sidecar is
written, and no orphan is removed. For every source whose sidecar is not
current it prints one line on stdout,

```
<state>\t<path relative to the directory>
```

with the state in lowercase, and gives the count of each state on stderr. It
exits 0 whatever it found. `--check` belongs to a directory and stands alone:
with `--force`, with `--strip`, or on a single file it is wrong arguments.

### `--strip`

`saga <dir> --strip` removes every `.qa` sidecar in the tree.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | refusal or failure |
| 2 | wrong arguments |

---

## Routing: which gleipnir checks a Python file gets

Gleipnir has two sets of Python checks. Saga routes each Python file to one:

- **V2** — the zone-architecture checks — is the default.
- **V1** applies to the projects listed in
  `capability/saga_runner/v1_legacy_projects.toml`. Each entry is a project
  path relative to the AI root, `$HOME/ai/`; a file whose path under that root
  starts with a listed entry takes V1. A file outside the root takes V2.

The list is embedded at compile time, so an edit to it takes effect with the
next build. `--test v1` and `--test v2` force one set for a run, whatever the
list says.

Rust and Svelte files each have one set of checks.

---

## Crates

| Crate | Tier | Holds |
|---|---|---|
| `core/saga_core` | core, pure | the report types (`SanityReport`, `Issue`, `Fault`), the sidecar naming, `SourceKind` |
| `capability/saga_runner` | capability, I/O | running the instruments, report generation, sidecar save and load, `examiner_fingerprint`, `sidecar_state`, the directory walk, the orphan sweep, the V1 routing list |
| `cli/saga_cli` | binary `saga` | argument parsing, the directory, check, strip and file modes |

Two more consumers link `saga_runner`:

- `capability/syn_runner` loads sidecars through it and asks it for their
  state;
- `hooks/hook_post_llm_tool`, the write hook, reaches it through
  `syn_runner::assess_written_file`, which examines a written file and writes
  its sidecar in-process.

## Rebuilding

```
nornir_deploy --build tools
nornir_deploy --build hooks
```

`tools` rebuilds the `saga` and `syn` binaries. The write hook links these
crates, so a change to saga, to gleipnir's checks or to the V1 list reaches it
through `hooks`.
