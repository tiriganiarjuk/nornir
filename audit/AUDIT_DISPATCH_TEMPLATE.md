# Audit Dispatch Template

Exact instructions for dispatching audit agents. Follow these precisely. Do not improvise.

---

## Common Rules (All Audits)

- **Two agents, A and B.** Independent. Results to `audit/strict_audit_A.md` and `audit/strict_audit_B.md` (structural) or `audit/docs_audit_A.md` and `audit/docs_audit_B.md` (documentation).
- **Model: opus.** General-purpose agent type. Run in foreground.
- **Response: SUCCESS or FAILURE only.** Based on whether the agent completed the task, not whether the codebase passed.
- **Auditors read only their guide in `audit/`.** The directory may hold reports from earlier runs, kept for comparison, and the other auditor's report. Both prompts below scope the auditor away from it, so each run is independent of every other.
- **Intersection is truth.** After both complete, only findings that appear in both reports become actionable items. Single-auditor findings are noted but not prioritized.

---

## Structural Audit Dispatch

**Guide:** `audit/STRUCTURAL_AUDIT_GUIDE.md`

### Agent prompt (copy exactly, change A→B for second agent):

```
You are Auditor A. Perform a strict architectural audit of the nornir
codebase at /Users/johnny/ai/smidja/nornir/.

Read audit/STRUCTURAL_AUDIT_GUIDE.md first. This is your primary
reference — it defines every invariant you are checking (P1-P10).

In audit/, read only that guide. Every other file in audit/ is
outside the audit.

Then audit the IMPLEMENTATION — read Cargo.toml files, read src/main.rs
and src/lib.rs files, read actual function signatures. Check every crate
in the workspace against the invariants. The guide tells you what correct
looks like and what drift looks like.

Do not just read documentation. Read code. The audit finds gaps between
what the architecture SHOULD be (per the guide) and what the code
ACTUALLY is.

Write your complete findings to:
/Users/johnny/ai/smidja/nornir/audit/strict_audit_A.md

Your response to me should be ONLY "SUCCESS" or "FAILURE" based on
whether you completed the audit task. Do not include any other text.
```

---

## Documentation Audit Dispatch

**Guide:** `audit/DOCUMENTATION_AUDIT_GUIDE.md`

### Agent prompt (copy exactly, change A→B for second agent):

```
You are Documentation Auditor A. Perform a strict documentation audit
of the nornir codebase at /Users/johnny/ai/smidja/nornir/.

Read audit/DOCUMENTATION_AUDIT_GUIDE.md first. This is your primary
reference — it defines the priorities and methodology.

In audit/, read only that guide. Every other file in audit/ is
outside the audit.

YOUR METHOD: For every document in the workspace outside audit/,
verify its claims
against the ACTUAL IMPLEMENTATION. Read the code. Check file paths
exist. Check function names match. Check enum variants match. Check
CLI flags match --help output. Check crate inventories against
workspace Cargo.toml.

When documentation and implementation DISAGREE, report it as a
disagreement. State what the doc says and what the code says. DO NOT
judge which side is correct — you cannot know whether the doc is stale
or the implementation drifted from the intended design. Both are
possible. Report disagreements neutrally for human resolution.

Then identify what is MISSING: which multi-crate subsystems have no
design document? Which architectural decisions are undocumented? What
would a fresh session get wrong because no document explains it?

Do NOT flag: missing doc comments on simple binaries, stale test counts,
cosmetic formatting, gate module documentation.

Write your complete findings to:
/Users/johnny/ai/smidja/nornir/audit/docs_audit_A.md

Your response to me should be ONLY "SUCCESS" or "FAILURE" based on
whether you completed the audit task. Do not include any other text.
```

---

## Post-Audit Process

1. Read both A and B reports
2. Identify intersection (findings both auditors agree on)
3. Create or update `plans/IMPROVEMENT_PLAN.md` with prioritized items
4. Work items top-down: plan mode → execute → verify → commit → next
