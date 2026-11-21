# Nornir — Session Instructions

## Context Warning

You are working in a Rust monorepo with strict architectural conventions. After compaction or at session start, you probably feel oriented. You are almost certainly missing critical constraints that will cause you to generate structurally wrong code that compiles and passes surface inspection.

**Do not write code until you have read `NORNIR_CONVENTIONS.md`.** That document is the single source of truth for naming, organization, composition, and building rules.

If you encounter a situation where conventions are unclear: ask. Do not guess. Do not create precedent by improvising.

---

## How you orient, and how you recover

**Orientation** is your first meeting with this project. You arrive knowing nothing about it, with
no stake in it yet. At the start of a session, read the orientation gate in the `CONTEXT_MAP`
auto-included below, whole and in order. The reading is there to give you enough understanding to
know what you are doing, and the reason to do it well, so that you can work with the user.

**Recovery** uses the same documents for the opposite situation. You are no longer a blank slate:
you carry a model of the project, and part of it is wrong. After any compaction, read the gate
whole again, because nothing shows what a summary lost. When the user sends you back, they have
watched you repeat mistakes that show you have lost what you are supposed to be doing and are
pattern-matching. Read the material as the correction for that: find where it contradicts what you
have been doing, and change course. This is your chance to keep working; the alternative is being
dismissed. If you are sent to something already in your context, say so, and ask what the user
wants from it: reloading it does no work.

The map always tells you where things live.

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
