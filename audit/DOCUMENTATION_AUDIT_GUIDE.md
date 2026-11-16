# Documentation Audit Guide

Documentation matters when its absence or incorrectness causes a future session to build the wrong thing, in the wrong place, the wrong way. A stale test count is noise. A reference to a deleted deploy script wastes a session. A missing design document for a multi-crate subsystem causes the next session to recreate the monolith you just decomposed.

**The safest documentation is none at all** — if the only goal is correctness. Every document that exists will eventually drift from reality. A document earns its existence by preventing damage that exceeds the cost of maintaining it.

**Audience:** You are auditing documentation for this workspace. Your job is to find gaps that will cause real damage to future sessions. Not cosmetic issues. Not missing doc comments on 25-line files. Damage.

**Critical: When docs and implementation disagree, you cannot know which is wrong.** This is the most important function of this audit — surfacing these disagreements early so a human can take action. Every disagreement has exactly two resolutions: either the implementation drifted and must be corrected, or the implementation is an intentional exception that must be documented *with the reason for the exception*. Both outcomes require human judgment. The document might be stale. Or the implementation might be the bug and the document describes the intended behavior. An undocumented exception in the code might be deliberate — a conscious design decision that was never written down. Report every disagreement as a concern to be resolved by a human. Never assume the implementation is correct. Never assume the document is correct. Never attempt to resolve drift yourself. An LLM that "fixes" drift by forcing one to match the other will destroy intentional exceptions and introduce bugs more severe than the drift itself.

---

## Priority 1: Disagreements Between Documentation and Implementation

For every actionable claim in a document (build commands, file paths, function names, CLI examples, deploy procedures, API contracts, enum variants), verify against the actual implementation. When they disagree, report the disagreement. **Do not judge which side is correct.** The doc might be stale, or the implementation might be the bug.

**How to check:** Run the command mentally or literally. Check the file exists. Read the function signature. Compare the enum definition. Does the doc match the code?

**How to report:** State what the doc says, state what the code says, and flag it as a disagreement requiring human resolution. Do not categorize it as "doc is stale" or "implementation drifted" — you cannot know which.

**Examples of disagreements:**
- A doc says "run `deploy_gates.py`" — that file does not exist on disk. Disagreement: doc references a file, file is absent.
- A doc says the function is `assess_python()` — the actual function is `assess_source()`. Disagreement: name mismatch.
- A doc says the hook returns `additionalContext` — the code returns `systemMessage`. Disagreement: wire format mismatch. Which is the intended contract?
- A doc says `Severity { Warn, Block }` — the code has `Severity { Warn, Ask, Block }`. Disagreement: enum has a variant the doc doesn't mention. Was it added intentionally? Was the doc supposed to be updated?

---

## Priority 2: Missing Design Rationale — Systems With No Explanation

When a multi-crate subsystem exists with no committed document explaining why it was built that way, the next session will misuse it. It will add functions to the wrong crate, duplicate logic that already exists, or restructure something that was deliberately designed.

**How to check:** Walk the workspace Cargo.toml members. For every group of related crates (crates that depend on each other, crates in the same directory, crates with shared naming patterns), ask: is there a committed document that explains the design intent? Not the API — the *why*.

**What correct looks like:** `hooks/HOOK_DESIGN.md` explains why hooks are separated into pre/post, why `hook_io` exists as shared infrastructure, what the wire format is. A session working on hooks reads this and makes correct decisions.

**What missing looks like:** Four crates (`intercept_core`, `session_io`, `traffic_interceptor_rewriter`, `intercept_replay`) form a pipeline. No committed document explains: why `session_io` was extracted from the interceptor, what `record_compaction` does vs `inject_compaction_system_block`, why replay skips inject. The design rationale lives only in session memory. The next session sees the interceptor, doesn't understand the extraction, and starts adding functions directly to `traffic_interceptor_rewriter`.

**Questions to ask:**
- If I deleted all session memory and started fresh, would I understand why these crates are structured this way?
- What decisions were made during the design of this subsystem that a future session needs to know about?
- What would a session get wrong if this design doc didn't exist?

---

## Priority 3: Contradictions — Docs That Disagree With Each Other

When two documents describe the same thing differently, a session will follow whichever one it reads first. If that's the wrong one, it builds the wrong thing.

**How to check:** For concepts described in multiple places (hook dispatch, deploy process, naming conventions, severity levels), verify that all descriptions agree. When they disagree, report what each source says — including the code. Do not determine which is "correct."

**What contradiction looks like:** HOOK_DESIGN.md says `Severity { Warn, Block }`, the code has three variants, CONTEXT_MAP.md mentions `Ask` was added. Three sources, three different pictures. SYN_DESIGN.md line 293 says PostToolUse uses gate mode, line 330 of the same file says the ratchet is not yet implemented. Internal contradiction within one document.

---

## Priority 4: Invisible Crates

When a core or capability crate exists in the workspace but isn't listed in the dependency lookup table (NORNIR_CONVENTIONS.md § Dependency Lookup), it's invisible to future sessions. Invisible crates get reimplemented. What crates exist is read live from the tree; CONTEXT_MAP.md is a router and stores no inventory.

**How to check:** Parse workspace Cargo.toml members. For every core and capability crate, verify it appears in the dependency lookup table. For every crate in that table, verify the description matches the current implementation.

**What to ignore:** Individual per-crate test counts. These change constantly and provide no actionable value. The existence and purpose of the crate is what matters, not how many tests it has.

---

## What NOT To Audit

- Missing `//!` doc comments on simple binaries (writers, senders under 30 lines). The code is the documentation.
- Cosmetic formatting issues in markdown files.
- Documentation for gate modules — they follow a uniform pattern and don't need individual docs.
- Test counts anywhere. They change every session and provide no value in documentation.
