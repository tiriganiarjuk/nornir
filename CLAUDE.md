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

@CONTEXT_MAP.md
