# Nornir — Session Instructions

## Context Warning

You are working in a Rust monorepo with strict architectural conventions. After compaction or at session start, you probably feel oriented. You are almost certainly missing critical constraints that will cause you to generate structurally wrong code that compiles and passes surface inspection.

**Do not write code until you have read `NORNIR_CONVENTIONS.md`.** That document is the single source of truth for naming, organization, composition, and building rules.

If you encounter a situation where conventions are unclear: ask. Do not guess. Do not create precedent by improvising.

---

## How you orient — first time or recovery (same thing)

Orienting and recovering are one act: read the project's main documents, **routed by the
`CONTEXT_MAP` auto-included below** — start with its **orientation gate**, which *is* the
recovery-source list. If already verifiably oriented you needn't re-read everything; the map
always tells you where things live.

---

## Key Rules

- Schemas are embedded at compile time via `include_str!()`. Changed `.schema.json` files have no effect until `nornir_deploy` runs.
- All helper functions return `Result`. Only `main()` calls `process::exit()`.
- Do not hand-write validation logic in Python. Import the gate module and call `validate()`.
- Do not bypass the gate API. Every gate returns `{"ok": bool, "data": ..., "error": ...}`. Check `ok` before using `data`.
- Run `cargo test` before committing. All tests must pass.
- Every tool is built through the before- and after-build checklists in `HOW_A_NORNIR_TOOL_IS_MADE.md`.

---

<!-- Salvaged memory residue routed into nornir. Additive only. -->

## Project Notes (salvaged)

### Group module contents by action, not by consumer

Functions belong together by WHAT they do, never by WHO consumes them. Grouping by consumer (e.g. `converters_field` serves field-tier, `converters_simple` serves simple-tier) scatters one action across modules and mixes actions within a module. Group by action (`schema_build`, `field_annotate`, `conditional_build`, `zone_collect`). When organizing, ask "what does this function DO?" — if two functions both build JSON Schema nodes they belong together even if one serves atoms and the other serves sections. A module mixing building + metadata + dispatching must be split by action.

### Hook response builders (hook_io::response)

`capability/hook_io/src/response.rs` defines one response type per Claude Code hook event; each type exposes only the methods valid for that event, so invalid combinations are compile-time errors, not runtime checks. Our terms map to the Anthropic wire format per event: Allow → `permissionDecision:"allow"` (PreToolUse) / omit decision (PostToolUse/Stop) / `behavior:"allow"` (PermissionRequest); Deny → `permissionDecision:"deny"` / `decision:"block"` / `behavior:"deny"`; context → `hookSpecificOutput.additionalContext`; reason → `permissionDecisionReason` (Pre) / top-level `reason` (Post/Stop) / `decision.message` (PermissionRequest). `Ask` is PreToolUse-only. Universal fields on every type (via `WithUniversal`): `suppress_output()`, `system_message(msg)`, `stop_session(reason)`. Usage: `PreToolUseResponse::allow().with_reason(...).with_context(...)`, `PreToolUseResponse::deny(reason)`, `PostToolUseResponse::allow().with_context(...)`.

### Workspace identity derivation

`datagram::workspace_from_path(path)` derives a `@{relative}` workspace name from the `~/ai/` base — e.g. `/Users/johnny/ai/smidja/nornir` → `@smidja/nornir`. Paths outside `~/ai/`, or no path, resolve to `@`. This is the universal convention for any tool that operates on directories (syn, kvasir, etc.).

### Why specialist tools carry the `_cli` package suffix

`saga` and `syn` are proper-noun binaries with no verb prefix. Their package/directory names are `saga_cli`/`syn_cli` — the `_cli` suffix on `syn` is mandatory because the bare name `syn` collides with the well-known Rust `syn` crate. Binary name stays `syn`.

### Gleipnir: fuzzy-returns policy and shared ownership detection

Gleipnir violation messages embed a FUZZY RETURNS POLICY: never reveal the numeric thresholds that triggered a check (revealing them invites gaming). In the Rust checks, `clone_is_ownership_transfer()` (in `checks_rs/prohibited.rs`) is a single AST parent-walking function shared by BOTH `no_clone_spam` and `no_string_abuse` — it decides whether a `.clone()`/`.to_string()` flows into an ownership-requiring context (struct field, function arg, return, match arm, etc.). Changing ownership-detection logic affects both checks at once.

---

@CONTEXT_MAP.md
