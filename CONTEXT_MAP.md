<!-- ═══════════════════════════════════════════════════════════════════════ -->
<!-- DO NOT EDIT THIS BLOCK. Before editing ANY part of this file, you MUST  -->
<!-- first read /Users/johnny/ai/CONTEXT_MANAGEMENT_SYSTEM.md — it defines  -->
<!-- what this file is for and what may NOT go in it.                        -->
<!--                                                                          -->
<!-- This file is a MAP (a router), not a source. Its only job is to tell    -->
<!-- you WHERE the context lives — it never holds the context, state, or     -->
<!-- progress itself. Reading this map does NOT orient you; you are NOT      -->
<!-- oriented until you have read the Orientation-gate docs it points to, in -->
<!-- the order given.                                                         -->
<!--                                                                          -->
<!-- THE WHITELIST RULE: if a doc is not in this map, it is not for           -->
<!-- orientation — do not read it. A stale doc is UNLINKED or DELETED, never -->
<!-- flagged in place. No freshness tags. No "do-not" guards. No state.       -->
<!-- Behavioral rules live in CLAUDE.md, not here.                            -->
<!--                                                                          -->
<!-- This file is LOW-CHURN. It changes only when how-to-get-oriented        -->
<!-- changes — i.e. a doc is added or removed. It is not a worklog and is    -->
<!-- not regenerated per session.                                            -->
<!-- ═══════════════════════════════════════════════════════════════════════ -->

# CONTEXT_MAP — Nornir

## Purpose

A Rust monorepo of compiled validators, tools and hooks for the `ai` ecosystem — including the gates that own every boundary the Python stages cross.

---

## Orientation gate — read these, in this order, to be oriented

> **Scope first.** Nornir is large and most of it is unrelated to whatever you were sent
> here for. Read the conventions and the build rules, then only the part you need. The
> gates are one tenant among many.

1. `NORNIR_CONVENTIONS.md` — the crate layout, tier rules, and naming law. Before writing any code.
2. `MUST_READ_BEFORE_BUILDING.md` — the build and deploy rules. A bare `cargo build` leaves the artifact where nothing looks for it; gates additionally need the maturin path, `.so` extraction and code signing, without which macOS kills the process on import.

> The next two apply if your work touches the agent pipeline's gates or writers. Skip them
> if you are here for something else in the monorepo.

3. `~/ai/smidja/galdr/context/AGENT_BUILD_SYSTEM.md` — why the pipeline exists, and the double gate that makes an LLM-written transform safe.
4. `~/ai/smidja/galdr/context/NORNIR_GATES_ELEMENT.md` — what a gate is concretely, the four call patterns, entry-point gates versus stage-boundary pairs, the three kinds of checking a gate does, and the output tool builder.

**Proof:** before writing anything, state (a) why the wall is a separate language and what that argument does *not* claim, (b) what happens to a changed schema before the gate is rebuilt, and (c) one thing you would have done that the build rules forbid.

---

## Refresh guide — want X → read Y

| Need | Read |
|------|------|
| crate layout, tiers, naming | `NORNIR_CONVENTIONS.md` |
| how to build or deploy anything | `MUST_READ_BEFORE_BUILDING.md` — never bare `cargo build` or `maturin build` |
| what a gate is / the call patterns / what it checks | `context/NORNIR_GATES_ELEMENT.md` |
| the output tool builder — registry, generator, write engine | `context/NORNIR_GATES_ELEMENT.md` § The output tool builder, then `tool_registry.toml` and `generate_writer.py` |
| which schema a gate embeds | the gate's `src/lib.rs` — one `include_str!` through a symlink into Verdandi's output |
| what a schema allows | `~/ai/smidja/verdandi/{project}/output/*.schema.json` |
| what calls the gates | `context/REGIN_ELEMENT.md`, `context/GALDR_ELEMENT.md` |
| what crates exist right now | read live — `ls gates/ writers/ capability/ core/`. Inventories are not stored here; a stored one is wrong by the next commit |
| the full forward cascade after a schema change | `context/AGENT_BUILD_SYSTEM.md` § The forward cascade |
| current state / what's left to build | read live — the code and `git log`. Nothing is stored |
| a tool owed a move onto a newer, better tool — or a new tool built that others should now use | `REQUIRED_UPGRADES.md` — forward-only; add the entry when the newer tool lands |
