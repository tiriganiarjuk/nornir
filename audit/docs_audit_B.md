# Documentation Audit B — Findings

**Auditor:** B
**Date:** 2026-03-20
**Method:** For each document, verified actionable claims (file paths, function names, enum variants, CLI flags, crate inventories, wire format fields) against the actual implementation. Read source code for every claim checked. Did not run binaries.

---

## Priority 1: Disagreements Between Documentation and Implementation

### D1. CONTEXT_MAP.md — Workspace member count is wrong

**Doc says:** "95 workspace members: 13 core + 12 capability + ..."
**Code says:** Cargo.toml has 97 workspace members. Core has 16 directories on disk (not 13). Capability has 11 directories on disk (not 12).

**Missing from core inventory:** `announce_core`, `time_core`, `text_core` are present in `core/` and listed in workspace Cargo.toml members, but absent from the CONTEXT_MAP Tier 1 table.

**Capability heading/table mismatch:** The heading says "(12 crates)" but the table lists only 11 crates. No 12th capability crate exists on disk. The heading count is wrong.

**Tier 3 summary math:** With corrected numbers (16 core + 11 capability = 27 library crates), the 97 total minus 27 = 70 binary crates. The Tier 3 heading says "35 cargo executables + 35 gate modules + 2 PyO3 modules" = 72, which also does not match 70. Requires reconciliation.

### D2. CONTEXT_MAP.md — `announce` listed under senders but lives in cli/

**Doc says (line 81):** "6 senders in `senders/` — `send_alert`, `send_warning`, `send_notification`, `send_heartbeat`, `send_datagram`, `announce`"
**Code says:** `announce` lives at `cli/announce/`, not `senders/announce/`. The `senders/` directory contains exactly 5 crates. `announce` is also listed in `deploy_categories.toml` under `[tools]`, not `[senders]`.

**Internal contradiction:** The same document's Known Issues section (line 116) says "announce naming exception — no verb prefix. Decomposed (item 1 done) and moved to cli/." This directly contradicts line 81, which still lists announce under senders.

### D3. HOOK_DESIGN.md — SessionStart output contract says `systemMessage`, code uses `additionalContext`

**Doc says (line 29):** SessionStart output contract is `systemMessage`.
**Code says:** `SessionStartResponse::to_json()` in `capability/hook_io/src/response.rs` (lines 636-649) outputs `hookSpecificOutput.additionalContext`, not a top-level `systemMessage`. The `Universal` struct does have a `system_message` field available as a builder method, but `SessionStartResponse` defaults to context injection via `additionalContext`.

This is a wire format disagreement — a session building a SessionStart hook would produce the wrong output structure if following the doc.

### D4. HOOK_DESIGN.md — PreCompact output contract says `systemMessage`, code has no context field

**Doc says (line 31):** PreCompact output contract is `systemMessage`.
**Code says:** `PreCompactResponse` in `response.rs` (lines 737-758) has NO context or additionalContext field. It only has the `Universal` struct, which offers `system_message()` as an opt-in builder method. The doc implies `systemMessage` is the default/expected output, but the type offers no convenience for it — a hook builder would need to call `.system_message()` explicitly.

### D5. SYN_DESIGN.md — CLI interface is incomplete

**Doc says (CLI Interface section):** Lists flags `--mode`, `--output`, `--silent`, `--project-dir`, `--target`, `--tool`, `--level`, `--filter`.
**Code says:** `syn_cli/src/main.rs` has additional flags not documented in SYN_DESIGN.md:
- `--stdin` (line 142) — read .qa JSON from stdin
- `--kind` (line 154) — file type filter `[py|rs|svelte|all]`
- `--narrow` (line 169) — show only a specific check code

`--stdin` is mentioned in the "Input Sources > Gate Mode" section's code example (`syn --stdin`) but not in the CLI reference table. `--kind` and `--narrow` are undocumented in SYN_DESIGN.md entirely.

### D6. HOOK_DESIGN.md — Settings.json example for hook_pre_llm_bash is incomplete

**Doc says (line 247):** `hook_pre_llm_bash --subversion block --truncation warn --evasion warn`
**Code says:** `hook_pre_llm_bash/src/main.rs` accepts 6 category flags: `--subversion`, `--truncation`, `--evasion`, `--destruction`, `--revert`, `--workflow`. The doc example omits `--destruction`, `--revert`, and `--workflow`. The source file's own usage comment (line 8) shows the complete invocation: `hook_pre_llm_bash --subversion block --truncation warn --evasion warn --workflow ask`.

This is not a simple omission — a session reading the doc would not know that `--destruction`, `--revert`, and `--workflow` categories exist, and could not configure them.

### D7. HOOK_DESIGN.md — hook_io::rules doc comment says "Four tiers" but lists three

**Doc says (rules.rs line 12-15):** "Four tiers (ascending enforcement): Warn, Ask, Block"
**Code says:** Three variants listed, not four. The comment says "Four tiers" but only describes three. Either a fourth tier was planned and never added, or the comment is a leftover from a previous design.

### D8. CONTEXT_MAP.md — "3 specialist tools" count is stale

**Doc says (line 78):** "3 specialist tools in `cli/` — `saga_cli` (binary: saga), `syn_cli` (binary: syn), `hush`"
**Code says:** `cli/` now contains 4 non-check tools: `saga_cli`, `syn_cli`, `hush`, and `announce`. The count should be 4.

---

## Priority 2: Missing Design Rationale

### M1. No design document for the datagram/hlidskjalf subsystem

Four crates form the datagram pipeline: `datagram_core` (types), `datagram_io` (emission), `record_datagrams` (daemon), and the socket protocol. No document explains:
- Why `datagram_core` is separate from `datagram_io` (the types are simple — was there a composition reason?)
- What the Hlidskjalf socket protocol looks like (message framing, JSON schema)
- How `record_datagrams` consumes datagrams and what it writes
- What `datagram_io::emit_validated_or_alert` does differently from `datagram_io::emit`

A session working on the datagram subsystem would need to read all four crates to understand the design. The watcher crate (`watch_and_diff_exchange_intercepts`) also emits datagrams and imports `emit_validated_or_alert` from `datagram_io`, but this relationship is not documented anywhere.

### M2. No design document for the announce/voice subsystem

Three crates form the announce pipeline: `announce_core`, `announce` (cli binary), and voice alert integration in `hook_io` (the `speak_alert` function). No document explains:
- What `announce_core` provides vs what `announce` does
- The voice directory convention (`~/ai/voice/`, `SILENT.lock`)
- How `hook_io::speak_alert` spawns announce as a subprocess
- The `--severity` and `--source` CLI flags of announce

### M3. No design document for the gleipnir subsystem

`gleipnir_core` is one of the most architecturally significant crates — it enforces code quality via tree-sitter AST analysis. No design document explains:
- How checks are registered and dispatched
- The check taxonomy (per-language check categories)
- How Svelte architecture checks work (imports, component structure)
- How exemptions are implemented (test code, static initializers)

The STRUCTURAL_AUDIT_GUIDE.md Priority 9 section lists check names and categories, but this serves as an audit checklist, not a design document.

### M4. No design document for the write_engine subsystem

`write_engine` is the most-imported capability crate — used by all writers, several hooks, session_io, and other binaries. No document explains:
- The `WriterConfig` struct and what each field controls
- The `OutputPath` variants and when to use each
- The `WriteFrequency` enum
- `write_truncate_fsync` (used for compaction truncation) and why it preserves inodes
- `ai_home()` — the only approved way to resolve `$HOME/ai`

A session adding a new writer would need to read existing writers and reverse-engineer the API.

---

## Priority 3: Contradictions Between Documents

### C1. CONTEXT_MAP.md contradicts itself about announce location

Line 81 lists announce under "6 senders in `senders/`". Line 116 says "announce ... moved to cli/." Both are in the same document. (Also reported as D2 above.)

### C2. HOOK_DESIGN.md hook output contracts vs response.rs event types

HOOK_DESIGN.md documents 7 event types in the event mapping table (PreToolUse, PostToolUse, Stop, UserPromptSubmit, SessionStart, SessionEnd, PreCompact). The `response.rs` module implements 11 response types: PreToolUse, PostToolUse, PostToolUseFailure, PermissionRequest, Stop, SubagentStop, UserPromptSubmit, SessionStart, SubagentStart, Notification, PreCompact, SessionEnd, InstructionsLoaded, ConfigChange.

Missing from HOOK_DESIGN.md: PostToolUseFailure, PermissionRequest, SubagentStop, SubagentStart, Notification, InstructionsLoaded, ConfigChange. These are either unused response types built speculatively, or they represent real CC hook events that the design doc hasn't been updated to cover. A session seeing these types in response.rs without doc coverage may not know whether to use them.

---

## Priority 4: Stale Inventories

### S1. CONTEXT_MAP.md Tier 1 table missing 3 core crates

Missing: `announce_core`, `time_core`, `text_core`. All three exist on disk and in workspace Cargo.toml. A session looking at the CONTEXT_MAP inventory would not know these crates exist, risking reimplementation.

### S2. CONTEXT_MAP.md Tier 2 heading says 12, table has 11

The heading says "(12 crates)" but only 11 are listed. Only 11 capability directories exist on disk. The heading number is wrong.

### S3. deploy_categories.toml `[senders]` lists 6 crates including `send_datagram`; `senders/` has 5 dirs

Wait — let me re-check. deploy_categories.toml [senders] lists: send_heartbeat, send_notification, send_warning, send_alert, send_datagram. That's 5, matching disk. But CONTEXT_MAP line 81 says "6 senders" by including announce. This is only a CONTEXT_MAP issue (covered in D2), not a deploy_categories.toml issue.

### S4. NORNIR_CONVENTIONS.md Workspace Dependencies list

The versions listed in NORNIR_CONVENTIONS.md match the actual workspace Cargo.toml exactly. No disagreement found.

---

## Summary

| Priority | ID | Document | Issue |
|----------|----|----------|-------|
| P1 | D1 | CONTEXT_MAP.md | Workspace member count wrong (95 vs 97), core count wrong (13 vs 16) |
| P1 | D2 | CONTEXT_MAP.md | announce listed under senders but lives in cli/ |
| P1 | D3 | HOOK_DESIGN.md | SessionStart output says systemMessage, code uses additionalContext |
| P1 | D4 | HOOK_DESIGN.md | PreCompact output says systemMessage, code has no context field |
| P1 | D5 | SYN_DESIGN.md | CLI reference missing --stdin, --kind, --narrow flags |
| P1 | D6 | HOOK_DESIGN.md | settings.json example missing 3 of 6 bash hook categories |
| P1 | D7 | HOOK_DESIGN.md | rules.rs comment says "Four tiers" but lists three |
| P1 | D8 | CONTEXT_MAP.md | "3 specialist tools" should be 4 (announce moved to cli/) |
| P2 | M1 | (missing) | No datagram/hlidskjalf design document |
| P2 | M2 | (missing) | No announce/voice subsystem design document |
| P2 | M3 | (missing) | No gleipnir design document |
| P2 | M4 | (missing) | No write_engine design document |
| P3 | C1 | CONTEXT_MAP.md | Self-contradiction: announce in senders (line 81) vs moved to cli (line 116) |
| P3 | C2 | HOOK_DESIGN.md | Documents 7 event types, code implements 11+ response types |
| P4 | S1 | CONTEXT_MAP.md | 3 core crates invisible (announce_core, time_core, text_core) |
| P4 | S2 | CONTEXT_MAP.md | Capability heading says 12, table has 11 |
