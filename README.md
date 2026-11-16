# Nornir

Validation-as-code Rust monorepo. Produces compiled binaries, PyO3 gate modules, and libraries for schema-driven LLM agent infrastructure.

## What This Is

Nornir is a Rust workspace organised in a strict three-tier architecture:

- **Core** (pure, no I/O) — type definitions, validation engines, formatting, diffing, error types
- **Capability** (I/O boundary) — file operations, schema embedding, hook protocols, datagram emission
- **Binaries** — CLI tools, pipeline gates, hooks, watchers, senders, a reverse proxy daemon

Everything that needs to be compiled, fast, or trusted lives here. Python projects in the ecosystem (Draupnir, Regin, Galdr, Bifrost) call into Nornir's compiled artifacts — either as PyO3 modules imported directly into Python, or as standalone binaries.

Nornir is currently a monorepo because these tools were built together and share core infrastructure. As the system matures, subsystems with independent lifecycles will be split into their own repositories.

## The Problem This Solves

LLMs generate mediocre code by default. Not broken code — mediocre code, at volume. Training data gravity pulls every output toward the statistical centre of mass: classes with methods, scattered state, `dict[str, Any]` at every boundary, configuration hardcoded as constants, circular imports, god classes, `utils.py` files that grow without limit. Instructions to "write functional code" or "follow clean architecture" fade within turns as context fills and training instincts reassert themselves.

The conventional response — better prompts, stricter instructions, more code review — does not work at scale. Instructions compete with the weight of the entire training corpus, and they lose. The only thing that durably shapes LLM coding behaviour is **experienced consequences**: an environment where quality-compliant patterns pass freely and violations block progress.

Nornir provides that environment. Its tools enforce a zone-based functional architecture through AST analysis, compile-time schema validation, and real-time feedback on every file write. The result is LLM-generated code that follows architectural principles not because the LLM was told to, but because the working environment makes principled code the path of least resistance.

**The proof is visible.** The Python projects built under this system — [Draupnir](https://github.com/johnny111272/draupnir) and [Regin](https://github.com/johnny111272/regin) — are entirely LLM-generated. Compare their file trees to any typical LLM-generated Python project. The typical project is flat, vaguely named, and unnavigable. Draupnir and Regin are spatial maps you can read like blueprints — every file in a zone, every function at a complexity level, every import path encoding its safety contract. Same model wrote both kinds of code. The difference is the infrastructure.

## The Code Quality Triad: Gleipnir, Saga, Syn

The guardrail system enforces a **zone-based functional architecture** where the file path IS the safety contract. LLM-generated Python projects are organised into two worlds that can never mix:

**Structure zone** (`structure/`) — data only. Frozen Pydantic models, enums, type definitions. No functions, no logic, no constants. Classes must inherit from BaseModel, RootModel, or Enum. Cannot import from logic zones. This kills OOP at the root: classes are data containers, period.

**Logic zones** (`logic/`) — functions only. No classes allowed. Organised by purity (pure, impure, transform) and by complexity level. Each level has a fixed filename:

| File | Level | What belongs here |
|------|-------|-------------------|
| `ffi.py` | L1 | FFI wrappers — thin calls to compiled Rust gates |
| `primitive.py` | L1 | Smallest useful operations — one thing, one function |
| `simple.py` | L2 | Compositions of primitives |
| `dispatch.py` | L3/L6 | Typed dispatch tables only — `dict[type, Callable]` |
| `composed.py` | L4 | Complex compositions |
| `assembled.py` | L5 | Highest-level compositions |
| `orchestrate.py` | L7 | Pipeline orchestration |

Lower levels cannot import higher levels. The dependency graph is a strict DAG enforced by Gleipnir's AST analysis. When you read `from pkg.logic.pure.graph_build.primitive import ref_prefix`, you know before reading the code: it's pure (no I/O, no side effects), it's a primitive (smallest useful operation), it's in the graph_build domain. The import path never lies.

### Gleipnir — Detection

Tree-sitter-based AST analysis engine. Pure computation — receives source bytes, returns typed Violation structs. Supports Python, Rust, TypeScript, and Svelte.

Every check exists because a specific LLM failure mode was observed repeatedly across independent projects. Each produces an educational message: what was found, and the check's detail, signal, direction and canary. The header of `core/gleipnir_core/gleipnir_messages.toml` defines the fields.

Key architectural checks: no methods on classes (kills OOP at the root), no classes outside structure zone, no functions inside structure zone, no constants in logic zones, no inline dispatch tables, no re-export shims, import count limits (high fan-in = coordination smell), zone boundary enforcement (structure can't import logic), unknown filename detection (files outside the level naming convention), and dispatch-file-only-tables enforcement.

### Saga — Truth Recording

Runs Gleipnir, Ruff, and Basedpyright on source files and writes unfiltered `.qa` sidecar reports adjacent to each source file. Records everything, filters nothing. Raw truth only. Post-execution hooks run Saga automatically on every file write, so quality data is always current as a side effect of working — not a separate step.

### Syn — Policy Enforcement

Reads `.qa` sidecars and applies configurable three-tier filtering. The write hook runs Syn in report mode and hands its assessment to the session: it informs and directs, and never blocks the write. Report mode also allows ad-hoc exploration with filter overrides. Gate mode gives a deterministic pass/fail locked to configuration — no overrides — for a caller that wants one.

The separation is strict: detection, recording, and policy are independent. Multiple consumers (Syn CLI, Svalinn dashboard, hooks, future tools) read the same truth data at zero analysis cost.

### The Economic Model

Gleipnir is not a quality certification system. It is **economic pressure**. Passing checks does not mean the code is good — it means the code hasn't triggered any failure detectors. Violating checks blocks progress. Over time, LLMs learn which patterns are cheap (functional, typed, bounded) and which are expensive (OOP, scattered, untyped). The only winning move is to write genuinely good code — the constraints are too tight to game with superficial fixes.

The pressure gradient is deliberate: low-pressure violations require trivial fixes (use a logger instead of print), medium-pressure violations require genuine thought (decompose an overlength function), high-pressure violations signal architectural problems (widespread `Any` types = broken boundary discipline). The gradient ensures the most consequential violations demand the deepest engagement.

## Enforcement Today: The Hook System

Nornir includes compiled Rust hook binaries that enforce security and quality in real time during LLM coding sessions. These are production tools, running daily, with proper test coverage.

### Pre-Execution Security Gating

Every tool invocation passes through layered security:

- **Floor rules** — permanently block access to credentials (`.ssh/`, `.aws/`, `.gnupg/`, `.kube/`, `.docker/`, `.netrc`). Cannot be overridden. The LLM cannot read your SSH keys regardless of what it is told.
- **Probing detection** — flags attempts to inspect the security infrastructure itself. Configurable severity.
- **Gaming detection** — flags attempts to circumvent constraints. Configurable severity.
- **Allow-path exemptions** — for sessions that work *on* the security system. Manual only, never auto-selected.

### Post-Execution Quality Assessment

Every write to a Python, Rust or Svelte file triggers the full quality pipeline: Gleipnir → Saga → Syn. Violations are injected as context into the LLM's next turn — immediate, automatic, zero-effort feedback.

### Context Re-Injection

When the LLM's context compacts, the assembled behavioural configuration is re-injected from the stored session record. Identity survives context resets.

## The Road to Bifrost

The hook system works, but depends on vendor-specific injection points that can change without notice.

### Bifrost Proxy (Current State)

An async Rust reverse proxy (hyper/tokio) in the traffic path between the LLM client and the API. Currently handles request interception with wire-format schema validation, compaction rewriting with injected instructions, and SSE response stream interception with thinking block stripping and side-logging. Operational and handling live traffic.

### Projected Direction

The enforcement logic currently in hooks — security gating, quality assessment, context management — will migrate into Bifrost's traffic layer. The proxy sees every request and response regardless of what the vendor changes. Same Rust code, same decision logic, same test coverage — in a position that cannot be pulled out from under it.

## Other Key Tools

### cc_launch — Session Initialisation TUI

A ratatui terminal interface that assembles composable system prompts from a library of atomic XML fragments. Personas invoke mythological archetypes that activate complex behavioural frameworks from training data — configuring how the LLM thinks rather than specifying what it should do. Assembled prompts are stored per-session by UUID for audit and re-injection at compaction.

### Pipeline Gates

Compiled Rust/PyO3 gate modules that enforce JSON Schema validation at every stage boundary of the agent definition pipeline. Each gate embeds its schema at compile time. Python code between gates never touches the filesystem.

## The Agent Definition Pipeline

Nornir's gates are one stage of a five-stage pipeline:

**[Verdandi](https://github.com/johnny111272/verdandi)** (type system) → **[Draupnir](https://github.com/johnny111272/draupnir)** (schema generation) → **Nornir Gates** (compiled validation) → **[Regin](https://github.com/johnny111272/regin)** (pipeline resolution) → **[Galdr](https://github.com/johnny111272/galdr)** (composition and render)

One declarative TOML definition produces many benchmarkable, auditable, reproducible agent configurations.

## Architecture

```
core/           Pure Rust libraries. No I/O, no side effects.
capability/     I/O boundary crates. File operations, schema embedding, protocols.
the rest        Binaries and Python extension modules, one directory per category.
```

The category directories, and the verb prefix each one's crates carry, are defined in
`NORNIR_CONVENTIONS.md` § Architecture and § Naming.

## Building

All crates are built and deployed through `nornir_deploy`, which wraps Cargo and Maturin to handle the mixed binary/PyO3 workspace.

```bash
nornir_deploy --all              # Build and deploy everything
nornir_deploy --build gates      # One category
nornir_deploy --list             # The categories, from deploy_categories.toml
```

## Licence

Copyright (c) 2025–2026 John Oker-Blom

This program is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

See [LICENSE](LICENSE) for the full licence text.
