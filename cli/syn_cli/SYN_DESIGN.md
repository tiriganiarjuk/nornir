# Syn Design — Quality Policy Gate

**Named after Old Norse "syn" — denial, refusal. The gatekeeper.**

Syn sits between saga (the truth recorder) and the reader of a quality report
(a worker session, a person at a terminal, a program). Saga records every
issue. Syn applies policy: which issues are shown, and which ones a gate denies.

```
saga (truth)  →  .qa sidecars  →  syn (policy)  →  report + decision
```

Syn reads reports, and writes no sidecar. On a directory given on the command
line it runs one examination of its own, the dead-code banner (below).

`syn --help` is the flag reference. This document is the model behind the flags.

---

## What syn reads

### Reports

Syn takes its reports from one of two places.

**Sidecars on disk** — the positional path, the current directory when none is
given:

| The path is | Syn reads |
|---|---|
| a `.qa` file | that sidecar |
| a source file | its sidecar, `.{file name}.qa` beside it; a `py`, `rs` or `svelte` file without one is a fault |
| a directory | every sidecar in the tree, narrowed by `--target` and `--kind` |

A directory is walked the way saga walks it: hidden directories and
`__pycache__`, `node_modules`, `venv` and `target` are never entered.
`--target src` (the default) also leaves out directories named `tests`,
`--target tests` leaves out directories named `src`, and `--target all` reads
the whole tree. `--kind` keeps the sidecars of one source kind — `py`, `rs` or
`svelte` — by the name of the source the sidecar belongs to; `all` keeps every
sidecar.

**Standard input** — with `--stdin`, syn reads one report or a JSON array of
reports. Each is validated against `schemas/tools/sanity-report.schema.json`.
Text that is not JSON, or a report that does not match the schema, is a refusal.

### Policy

The policy is two jq expressions, each evaluated against one issue:

| File | Selects | Default when the file is absent |
|---|---|---|
| `.syn/warn.toml` | the issues that are shown | `.tool == "gleipnir"` |
| `.syn/deny.toml` | the issues a gate denies | `.severity == "blocked"` |

```toml
filter = '.tool == "gleipnir"'
```

Syn reads `.syn/` in one directory: the one `--project-dir` names, or the
current directory. `--project-dir` sets where the policy is read and the
workspace a broadcast is filed under; which sidecars are read is decided by the
positional path alone.

A policy file's shape is `schemas/tools/syn-policy.schema.json`: one key,
`filter`, a non-empty string. An absent file leaves its default in force. A
file that is present and cannot be read, is not TOML, does not match the
schema, or states a filter that does not compile is a refusal — a default
never stands in for a policy file that is there.

---

## Filtering

```
every issue of every report
  │
  ├─ not selected for showing ──→ left out
  │
  └─ selected for showing ──→ VISIBLE, grouped by tool and check
       │
       └─ also matches the deny filter ──→ counted as a deny issue
```

What is shown is selected by the warn filter. In report mode the command line
can select instead: when any of `--tool`, `--level`, `--filter` or `--narrow`
is given, an issue is visible when it satisfies every one of those given, and
the warn filter is set aside for that run. A `--filter` expression that does
not compile is a refusal.

The deny filter is evaluated on visible issues only.

### Faults

A fault says that part of the examination did not happen, so a file is not
known to be clean. No filter hides a fault. Syn reports:

- every fault a report carries — an instrument saga could not run, or whose
  output it could not use;
- a sidecar that does not load — unreadable, or not matching the sanity-report
  schema; it contributes no report;
- a sidecar that is **stale** — its source holds other content than saga
  examined, or it was examined by other checks than saga's now; its report is
  still used, and the fault names the command that refreshes it;
- a sidecar whose source file is gone; its report is still used;
- one source file that has no sidecar, when the path names that file — saga has
  not examined it, and the fault names the command that does. A directory's
  sources without sidecars are listed by `saga <dir> --check`;
- a directory tree that could not be listed whole.

Whether a sidecar is stale is decided by `saga_runner::sidecar_state`, the one
definition of sidecar currency (see `cli/saga_cli/SAGA_DESIGN.md`): it compares
the sidecar's `content_hash` with the source, and its `examined_by` with the
examiner fingerprint of the saga crates syn is built with. Reports handed over
on stdin are taken as they are.

### Decision

| Decision | When |
|---|---|
| `deny` | gate mode, and at least one visible issue matches the deny filter, or there is a fault |
| `warn` | otherwise, when there is a visible issue or a fault |
| `allow` | nothing visible and no fault |

The rule is one function, `syn_core::decision_of`. A fault is a part of the
examination that did not happen — an instrument that did not run, a sidecar that
is stale, unreadable, without its source, or absent for the file asked about.
A gate answers whether a file was examined and is acceptable, so a gate denies
on any fault. Faults found while loading sidecars are joined to the result with
`syn_core::with_faults`, which takes the decision with them counted.

---

## Modes

### `--mode report` (default)

The assessment itself. The command-line selectors are available for
exploration. The exit code is 0 whatever was found.

### `--mode gate`

A reproducible accept/reject decision, locked to the policy files:
`--tool`, `--level`, `--filter` and `--narrow` are wrong arguments here. Gate
mode reads the same inputs as report mode — sidecars from the path, or reports
with `--stdin` — prints the same report, adds a one-line summary on stderr, and
exits 1 on a `deny`. A pass means the file was examined and is acceptable: a
file syn could not judge is denied.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | the report was made; in gate mode, nothing is denied |
| 1 | gate mode denies — or a refusal or failure, with its failure class on the first stderr line and the message after it |
| 2 | wrong arguments |

The refusals are a policy file syn cannot use, a `--filter` that does not
compile, and stdin that is not a valid report. The failure classes are listed
in `MUST_READ_BEFORE_BUILDING.md`.

---

## Output

**Stdout**, one of:

- **TOON** — what a worker session is shown; the default when stdout is a pipe
- **Colored** — ANSI output for a terminal; the default on a tty
- **JSON** — for a program

**Hlidskjalf broadcast** — a side channel beside stdout. When there is
something to report — a visible group or a fault — syn sends one quality
datagram: the grouped issues and faults, the counts, and the decision, filed
under the workspace of the policy directory. A fault or a deny makes it high
priority. A run with nothing to report sends nothing, and `--no-broadcast`
sends nothing in any case. The datagram is fire-and-forget: it costs nothing
when Hlidskjalf is not listening.

---

## The dead-code banner

When syn is run on a directory — not on a file, not with `--stdin` — it also
prints, on stderr, the dead code of every Python project at or under it: one
line per top-level function or class nothing reaches, `path:line  name  (kind)`,
ready to hand to whoever wrote it. Nothing is printed when nothing is dead. It is
`dead_code_find <dir>`'s report, from the same crate, `capability/dead_code_io`;
what counts as alive is `core/dep_resolve_core/src/liveness.rs`. It is computed
fresh on each such run and recorded nowhere, and it does not touch the decision
or the exit code. The write hook never reaches it.

## The write hook

`hooks/hook_post_llm_tool` runs after a tool writes a `.py`, `.rs` or `.svelte`
file. It links `syn_runner` and makes one call,
`syn_runner::assess_written_file(file, policy directory)`, with the hook's
working directory as the policy directory. That call:

1. has saga examine the file and write its sidecar;
2. loads the policy;
3. applies it in report mode, with no command-line selectors;
4. broadcasts when there is something to report;
5. returns an `Assessment`: `clean` — no visible group and no fault, computed
   from the filtered result — and `report_text`, the TOON report.

The hook says nothing for a clean assessment and hands the session the report
text otherwise. An assessment that did not happen — the sidecar could not be
written, the policy could not be used — is the failure `quality_check_not_run`:
the session is told the file was not examined, and an alert goes to Hlidskjalf.
The hook informs and directs; it never blocks the write.

---

## Crates

| Crate | Tier | Holds |
|---|---|---|
| `core/syn_core` | core, pure | filter compilation and evaluation (jaq), the policy types, `apply_filters` and the decision |
| `core/report_render_core` | core, pure | grouping, the TOON / colored / JSON renderers; shared with svalinn |
| `capability/syn_runner` | capability, I/O | policy loading, sidecar discovery and loading with the currency faults, reports from stdin, the broadcast, `assess_written_file` |
| `cli/syn_cli` | binary `syn` | argument parsing, and the calls into the three above |

`syn_runner` reads sidecars and asks their state through `saga_runner`, reads
policy files through `reader_io`, and takes its failures from the error
registry, `core/failure_mode_core/failure_modes.toml`.

## Rebuilding

```
nornir_deploy --build tools
nornir_deploy --build hooks
```

`tools` rebuilds the `syn` binary. The write hook links `syn_runner`, so a
change to syn's crates reaches it through `hooks`.
