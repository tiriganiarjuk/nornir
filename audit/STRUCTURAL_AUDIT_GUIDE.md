# Nornir Structural Audit Guide

This document describes the architectural invariants of the nornir workspace and why they matter. It exists because LLM-generated code drifts from these invariants in ways that look correct but cause structural damage that compounds across sessions.

**Audience:** You are auditing this workspace. Your job is to find violations of these invariants. Use your own judgment to decide how to check — the invariants are described here, the detection methodology is yours.

**What this is NOT:** A linter checklist. Gleipnir already enforces many invariants mechanically via AST analysis — from style (naming, function length) through design (ownership patterns, error discipline) to architecture (import zone boundaries, class placement). See "What Gleipnir Covers" at the end. This guide covers invariants that require reading comprehension and judgment — things an AST walker cannot detect.

---

## Priority 1: Pure Logic Must Not Live in Binary Crates

This is the single most damaging pattern in LLM-generated Rust code and the hardest to catch because the code works perfectly.

**The invariant:** Logic that is deterministic — takes inputs, returns outputs, no I/O, no environment access — belongs in a core library crate (`core/`), not in a binary crate (`cli/`, `hooks/`, `senders/`, etc.).

**Why it matters:** Pure logic trapped in a binary crate is invisible to the rest of the workspace. When a second binary needs the same logic, it gets reimplemented (differently, with different bugs). When a third binary needs it, you have three divergent copies. By the time you notice, extraction is a multi-session project.

**What makes this hard to catch:** The binary compiles, passes tests, does the right thing. There is no error. The damage is architectural — it's a decision that forecloses future composition. An auditor has to read the code and ask: "could anything else ever need this logic?" If yes, it shouldn't be here.

**What correct looks like:** Binary crates are thin. They handle CLI args, call into core/capability crates, and format output. The binary is orchestration; the logic is elsewhere. Writers are ~20 lines. Simple senders are ~25 lines. Even complex tools like saga keep their pure types in saga_core, their I/O in saga_runner, and their binary is orchestration.

**Questions to ask while reading a binary:**
- If I deleted this binary, would any reusable logic disappear with it?
- Could a future tool reasonably need any of these functions?
- Are there functions here that take data in and return data out without touching the filesystem, network, or environment? Those are pure — why are they in a binary?
- Is this binary growing because new logic is being added to it, or because it's composing more capabilities? Growth from logic accumulation is the warning sign.

**Common drift patterns:**
- A binary starts small and correct, then a "quick feature" adds a pure function directly to main.rs instead of to the appropriate core crate. The next session sees that function in main.rs and adds another one next to it. Within three sessions the binary is a monolith.
- Formatting and rendering logic is especially prone to this. It's pure, it's easy to write inline, and it "only" serves this one binary — until it doesn't.
- Helper functions that transform data structures (grouping, sorting, filtering, mapping) are almost always pure logic that belongs in a core crate, even when they feel too small to extract.

---

## Priority 2: The Three-Tier Dependency Model

**The invariant:** Nornir has three tiers of crates with strict dependency rules:

- **Tier 1 (core/):** Pure library crates. Depend only on other core crates and external workspace dependencies. No I/O, no side effects. Names use `_core` suffix.
- **Tier 2 (capability/):** Feature library crates that may perform I/O. Depend on core and other capability crates. Names do NOT use `_core` suffix.
- **Tier 3 (binaries):** Executable crates and Python extension modules, in the category directories listed in `NORNIR_CONVENTIONS.md` § Architecture. Depend on core and capability. **Never depend on other binary crates.**

**Why it matters:** Tier violations create invisible coupling. A binary importing from another binary means their build, test, and deploy lifecycles are entangled. Shared logic between binaries signals that a core crate is missing.

**What makes this hard to catch:** Cargo doesn't enforce tiers. A binary can depend on another binary's library target and the compiler won't complain. The violation is in the `Cargo.toml` dependency declarations, not in the Rust source.

**Questions to ask:**
- Does this crate's dependency list make sense for its tier? A core crate depending on a capability crate is a tier violation even if it compiles.
- Are two binary crates sharing logic through a dependency between them? That shared logic should be in a core crate instead.
- Does a core crate perform I/O (file reads, network, environment variable access)? If so, it's either misplaced or miscategorized — it should be in capability/.
- Is there a crate in `core/` that doesn't have `_core` in its name, or a crate outside `core/` that does? The suffix and directory should agree.

**Common drift patterns:**
- A binary needs a function from another binary. Instead of extracting to a shared core crate, the developer adds a `[lib]` target to the source binary and imports from it. This "works" but creates a tier violation — binary-to-binary dependency.
- A core crate starts pure, then someone adds "just one" filesystem read because it's convenient. Now it's a capability crate wearing a core crate's name, sitting in the wrong directory.

---

## Priority 3: process::exit and Panic Discipline

**The invariant:** `process::exit()` appears ONLY in `main()`. Every other function returns `Result`; an engine's error is typed and names its failure class through `error_type()` (`NORNIR_CONVENTIONS.md` § Binary Structure). No exceptions. `.unwrap()` and `.expect()` are prohibited in production code (gleipnir enforces this), with specific exemptions for `static`/`LazyLock` initializers and `const` contexts.

**Why it matters:** `process::exit()` in a helper function makes that function untestable — it kills the test harness. `.unwrap()` and `.expect()` are soft `process::exit` — they panic, which in a binary is an uncontrolled exit. In pure logic that should be testable, panics are just as bad.

**What correct looks like:** Every function below main returns Result. Complex binaries use clap derive for argument parsing — clap handles parse errors and `--help` exits internally, so main contains only the match on `run()`. Simpler binaries have two match arms: parse_args and run.

**Questions to ask:**
- Can every function in this binary be called from a test? If a function would kill the test harness on error, it has a hidden exit or panic.
- Are there `.unwrap()` or `.expect()` calls outside of `static` initializers, `const` blocks, or test code? Those are panic sites in production.
- Does a function return a bare value (not Result) but can encounter error conditions? Where does the error go?

---

## Priority 4: Composition Over Reimplementation

**The invariant:** Before writing any logic, check whether an existing core or capability crate already handles it. The workspace has purpose-built crates for file writing, schema validation, directory walking, QA report rendering, hook decision contracts, datagram emission, format conversion, path validation, and more.

**Why it matters:** Reimplemented logic diverges. Two directory walkers skip different directories. Two schema validators handle errors differently. Two path traversal checks reject different characters. The bugs are subtle and the divergence is invisible until someone compares the implementations.

**Key crates to know about:** the table in `NORNIR_CONVENTIONS.md` § Dependency Lookup names the one crate for each need.

If you see logic in a binary that overlaps with any crate in that table, that's a violation.

**Questions to ask:**
- Is this binary doing its own file writing instead of using write_engine? Its own directory walking instead of directory_read_io? Its own path traversal checks instead of path_core?
- Does this binary contain a function that looks like it belongs in one of the crates listed above? Even if the implementation differs slightly, the intent may overlap.
- Are two binaries doing similar things in different ways? That usually means both should be using a shared core function that doesn't exist yet.

---

## Priority 5: Naming Encodes Architecture

**The invariant:** Names in nornir carry architectural meaning. They are not labels — they are contracts.

- Binary names use verb prefixes that encode their category, as tabled in `NORNIR_CONVENTIONS.md` § Naming
- Core crate names use `_core` suffix. Capability crates do NOT use `_core` suffix.
- Directory name = package name = binary name. Always. No aliases, no mismatches. (Exception: `saga_cli` → binary `saga`, `syn_cli` → binary `syn` for specialist tools.)
- The crate's category directory tells you its tier and deploy script.

**Why it matters:** An LLM encountering this workspace infers architecture from names. If a capability crate has a `_core` suffix, a future session will misclassify it as a Tier 1 pure library. If a binary in `interceptors/` doesn't use the `intercept_` prefix, the next session won't know what category it belongs to.

**Questions to ask:**
- If an LLM saw only this crate's name, would it correctly infer the crate's purpose, tier, and category?
- Does the binary name's verb prefix match the directory it lives in?
- Do the `Cargo.toml` package name, the `[[bin]]` name, and the directory name all agree?
- For complex binaries using clap derive: do the clap struct field names match the domain vocabulary? clap derive makes the field name the flag name, so field naming IS CLI naming.

---

## Priority 6: Security Hook Coverage

**The invariant:** Hook binaries that enforce security policy must have dual-direction test coverage:

1. Known-malicious inputs ARE detected (no false negatives)
2. Known-benign inputs are NOT flagged (no false positives)

**Why it matters:** A security hook with only positive tests ("it catches bad things") is a ticking time bomb. You don't know what it falsely blocks. A developer running `cargo build` shouldn't trigger the same hook that catches `rm -rf ~/ai/`. Both sides must be tested.

**Hooks to pay special attention to:** The `hook_pre_llm_bash` and `hook_pre_subagent_bash` hooks enforce security policy on shell commands. Their detection rules involve regex patterns for subversion, truncation, and evasion of workspace configuration. These are the most security-critical code in the workspace.

**Questions to ask:**
- For each detection rule in a security hook, is there at least one test that confirms a benign input is NOT flagged?
- Are the regex patterns too broad? A pattern that catches `rm` will also catch `cargo rm` and `--format`. Are those false positives tested for?
- Do the hook rules and the hook tests tell the same story? Rules added after tests were written may lack test coverage entirely.
- Are the TOML rule files (`hook_io::rules`) and the hardcoded detection logic in sync? A rule that exists in TOML but isn't exercised in tests is untested policy.

---

## Priority 7: Schema-First Data Validation

**The invariant:** All structured data is validated against JSON Schema files in `schemas/`. Schemas are the source of truth. Procedural shape-checking in Rust (match arms that check field names and types) is not schema validation.

**Why it matters:** When validation logic is procedural, it drifts from the actual data format. The schema says field X is required, but the Rust code doesn't check for it. Or the Rust code checks for a field that was removed from the schema three sessions ago. The schema file and the code tell different stories.

**What correct looks like:** A JSON Schema file exists in `schemas/`. The schema is embedded via `schemas_embedded`. Validation calls the schema validator. The binary code never manually checks whether fields exist or have the right type — the schema handles that.

**Questions to ask:**
- Is there Rust code that manually checks whether a JSON field exists, what type it is, or whether it matches a set of allowed values? That's procedural shape-checking pretending to be validation.
- For each data shape that flows through the system, can you point to a `.schema.json` file that defines it? If the answer is "the validation is in the Rust code," there is no schema.
- Do the schemas in `schemas/` match what the code actually validates?

---

## Priority 8: Stale Tests After Contract Changes

**The invariant:** When a data contract changes — a schema field is renamed, a struct gains or loses a field, a function signature changes — every test that touches that contract must be fully re-evaluated. Not tweaked. Re-evaluated.

**Why it matters:** This is the most common and most dangerous LLM test failure mode. A schema changes `type` to `kind`. A test asserts `json["type"] == "syn_report"`. The test fails. The LLM sees a failing test and a one-line fix: add `"type": "syn_report"` back to the function output. The test passes. The code is now wrong — it emits a field that nothing consumes, the test verifies a phantom contract, and the actual current contract remains untested.

**The correct response to a failing test after a contract change:**

1. **Stop.** Do not touch the test or the code yet.
2. **Find the contract change.** What actually changed?
3. **Read the test's assertions.** Which claims are still true? Which are stale?
4. **Decide: update or rewrite.** If most assertions are still valid and only one is stale, update it. If the contract changed substantially, delete the test and write a new one from the current contract.
5. **Never make the code match the test.** If the test expects a field and the code doesn't produce it, the test is wrong.

**Questions to ask:**
- When was this test last meaningfully updated? If the code has evolved since, the test may be stale.
- Does this test verify the current data contract, or a previous version of it?
- After a schema or struct change, were the tests rewritten or just tweaked to pass?

---

## Priority 9: Gleipnir Check Accuracy

**The invariant:** Gleipnir's own AST-based checks must be correctly calibrated — rejecting actual violations without flagging legitimate code. A false positive in gleipnir is worse than a missed violation because it trains LLMs to dismiss gleipnir output.

**Why it matters:** Gleipnir runs as a post-edit hook on every file change. If a check fires falsely, every session sees the false positive, every session dismisses it, and the dismissal habit transfers to real violations. The check becomes invisible. Conversely, a check that misses common violation patterns provides false confidence.

**Current gleipnir check categories (Rust):**
- `no_unwrap` — .unwrap()/.expect() in production code (exempts static/LazyLock/const/test)
- `no_println` — println!/dbg! in library code (exempts main.rs, test code, output-named functions)
- `no_clone_spam` — .clone() without ownership justification (exempts return position, collection insert, struct fields, match arms, iterators, test code)
- `no_string_abuse` — "literal".to_string() / String::from("literal") allocations
- `no_pub_overuse` — too many pub functions in non-entry-point files
- `no_underscore_prefix` — _variable used as actual variable (not genuinely unused)
- `function_length_rs` — functions exceeding line threshold
- `nesting_depth_rs` — deeply nested control flow
- `short_names` / `numbered_names` — single-letter or numbered variable/param names
- `suppression` — #[allow(...)], clippy suppression attributes outside test code

**Questions to ask:**
- Does a check fire on code patterns that are genuinely correct? Document the pattern and propose an exemption.
- Does a check miss common violation patterns? Document the missed case and propose a detection addition.
- Are the exemption rules correct? A check that exempts "test code" should correctly identify test modules, test functions, and test helper functions — not just `#[test]` annotations.
- Is the check's error message actionable? A message that says "violation found" without explaining what to do is useless. A message that explains the principle and gives a concrete fix direction is valuable.

**Current known areas for gleipnir improvement:**
- `no_clone_spam` — ownership-transferring call patterns (`.push()`, `.insert()`) could be recognized more broadly
- `no_string_abuse` — same ownership-transfer recognition applies to `.to_string()` in return position
- `nesting_depth_rs` — `match` arms inside `for` loops are counted as nesting but are often the idiomatic pattern
- `no_println` — `main()` function in binary crates and output-named functions should be exempt

---

## Priority 10: Orphaned Artifacts

**The invariant:** When files are renamed, moved, or deleted, their associated artifacts must be cleaned up.

**Why it matters:** Nornir generates `.qa` sidecar files for every source file it scans. When a file is renamed or deleted, the old `.qa` file remains. Downstream tools read `.qa` files at face value — an orphaned sidecar appears as real violations in a file that no longer exists.

**This extends beyond .qa files:** Dead entries in deploy script crate lists, workspace Cargo.toml members pointing to deleted crates, symlinks pointing to missing binaries, schema entries for removed schemas, documentation referencing renamed crates.

**Questions to ask:**
- Does every crate listed in a deploy script still exist? Does every workspace member in Cargo.toml point to a real crate?
- Are there schema files in `schemas/` that nothing references?
- Has a crate been renamed but old references to the previous name survive in error messages, logging, comments, or documentation?
- Do documentation files reference tools, crates, or workflows that no longer exist?

---

## What Gleipnir Covers (Do NOT Audit These)

Gleipnir runs automatically as a post-edit hook via `syn`. Every check is a signal about code quality and architectural health — function length signals accumulated responsibilities, short names signal the author didn't think about the reader, println in library code signals misunderstanding of the orchestration boundary. Gleipnir detects these mechanically via tree-sitter AST analysis. Do not duplicate this coverage in architectural audits.

**Python checks:**
- print() calls, None returns, single-letter variables, function length, parameter count
- Nesting depth, short/numbered names, underscore prefixes
- Bare exceptions, broad exceptions, type suppression (noqa, pyright ignore, pylint disable)
- Import violations (cross-zone, relative, parent), type safety (Any, bare dict/list, large unions)
- Architecture (classes outside structures/, god classes, dataclass usage)

**Rust checks:**
- .unwrap()/.expect(), println!/dbg!, .clone() spam, string literal allocation
- Function length, nesting depth, short/numbered names, underscore prefixes
- pub overuse, suppression attributes (#[allow(...)])

**TypeScript checks:**
- console.log/warn/error, function length, nesting depth
- Short/numbered names, underscore prefixes, eslint/ts suppression comments

---

## Reference Documents

- **NORNIR_CONVENTIONS.md** — single source of truth for naming, organization, composition, building, deploying, testing
- **CONTEXT_MAP.md** — the router: which document answers which need. It stores no inventory; what crates exist is read live from the tree

**This guide is the "why" and "what to look for." NORNIR_CONVENTIONS.md is the "how to do it right."**

---

## For the Auditor

You are looking for violations of the invariants above. The most valuable findings are the ones that look correct — code that compiles, passes tests, produces right answers, but violates an architectural invariant that will cause damage across future sessions.

Trust your reading of the code. If something feels like it's in the wrong place, it probably is. If a binary feels too large, it probably contains logic that should be extracted. If you see similar code in two places, one of them shouldn't exist.

The damage from architectural drift is not immediate. It's cumulative. Each small violation makes the next session's code slightly worse. Your audit prevents the next rebuild.
