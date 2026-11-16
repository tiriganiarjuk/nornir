# Documentation Audit B — nornir

Audited against HEAD `97dc2c9` (commit date 2026-11-16 19:06 +0200), working tree clean at start.
Method: `audit/DOCUMENTATION_AUDIT_GUIDE.md`. Every claim below names the document line and the code
it was checked against. **Disagreements are reported, not resolved** — in each case either the
document or the implementation may be the one that drifted; that is a human decision.

Scope: every tracked document outside `audit/` (27 files). Three slices were checked by delegated
readers and spot-verified by me (marked *[delegated]*): `core/format_core/TOMLX_SPEC.md`, the
`core/gleipnir_core/*.md` set with `.claude/plans/gleipnir_false_positive_reduction.md`, and the
cc_launch / announce / interceptor documents. Spot checks I re-ran myself are noted inline.

Note for the operator: reading `cli/syn_cli/SYN_DESIGN.md`, and the delegated reads of
`gleipnir_core` and `saga_runner`, tripped `hook_pre_llm_tool`'s probing rule and notified you. The
reads were required to compare those documents against their code. Nothing in the repo other than
this file was written; no build was run.

Severity tags: **[H]** a session following the doc builds or runs the wrong thing; **[M]** misleads
a reader into a wrong model; **[L]** minor, low damage.

---

## Priority 1 — Disagreements between documentation and implementation

### 1.1 Root documents

> **[H] Workspace identity format — three sources, two answers.**
> `CLAUDE.md:28` — `datagram::workspace_from_path(path)` gives `@smidja/nornir` (slash).
> `NORNIR_CONVENTIONS.md:256` — `datagram_io::workspace_from_path` gives `@smidja:nornir` (colon).
> Code `capability/datagram_io/src/lib.rs:252-276` — crate is `datagram_io`, separator is `:`
> ("Colon separators avoid `/` which breaks Svelte 5's reactive proxy"). CLAUDE.md disagrees with
> the code on both the crate path and the separator.

> **[H] README build command does not exist.** `README.md:143` — `nornir_deploy --build all`.
> `nornir_deploy:431-435` — `--all` is a flag; `--build` takes category names from
> `deploy_categories.toml`, which has no `all` category.

> **[M] README inventory.** `README.md:7` "97 crates" — workspace `Cargo.toml` has 145 members.
> `README.md:112` "35 compiled Rust/PyO3 gate modules" — 37 gate crates in `gates/` and
> `gates/gate_recipes.toml`. `README.md:124-136` architecture tree omits `readers/`, `text_writers/`,
> `binary_readers/`, `binary_writers/`, `presence/`, `converters/`, `dispatchers/`; says `cli/` holds
> "pipeline check utilities" (none exist: `cli/` is announce, cc_launch, dep_resolve_file,
> dep_resolve_function, hush, saga_cli, syn_cli); lists `daemons/` as "(bifrost_proxy)" — it also
> holds `record_datagrams`, `relay_mic_to_voice`. `README.md:13` lists Bifrost among "Python
> projects" — `daemons/bifrost_proxy` is a Rust crate.

> **[M] Naming table vs binaries that fit no prefix.** `NORNIR_CONVENTIONS.md:53` — `saga` and `syn`
> "are the only exceptions" to the verb-prefix table. Binaries outside the table:
> `cli/announce`, `cli/cc_launch`, `cli/hush`, `cli/dep_resolve_file`, `cli/dep_resolve_function`
> (no `dep_`/`resolve_` row), `daemons/bifrost_proxy` (table: `record_` → `daemons/`),
> `daemons/relay_mic_to_voice` (`relay_` not in table), `interceptors/traffic_interceptor_rewriter`
> (table: `intercept_` → `interceptors/`). `hook_` row (`:45`) reads "Intercepts an LLM tool call for
> security/quality" — 7 of 12 hooks are SessionStart/SessionEnd/Stop/Notification/PostCompact hooks
> (TTS, registry, prompt re-injection), and one (`hook_post_llm_read`) injects dependency context.

> **[M] Gate naming rule vs gate kinds.** `NORNIR_CONVENTIONS.md:65` — "direction is `input` or
> `output`", stage "matches schema name". `schemas/tools/gate-recipes.schema.json` — kinds are
> `input`, `output`, `passthrough`, `defaults`. `gate_paths_verified` (passthrough, no direction) and
> `gate_raw_definition_defaults` (direction slot holds `defaults`) break the stated rule;
> `gate_paths_verified` and `gate_paths_verified_input` embed `PATHS_RESOLVED`
> (`gates/gate_recipes.toml`), so their stage name does not match their schema name.

> **[H] "Every built tool has a CLI binary" vs the binary tools.** `HOW_A_NORNIR_TOOL_IS_MADE.md:35-39`
> "A built tool always has a CLI binary"; `NORNIR_CONVENTIONS.md:124-148` one tool, two faces, PyO3
> behind the `python` feature, faces hand results to `tool_face_io`. Code:
> `binary_readers/read_png_verify_heights`, `binary_writers/write_bytes_verify_png`,
> `binary_writers/write_bytes_verify_quantized_mesh` have no `src/main.rs`, no `python` feature
> (non-optional `pyo3`), and build the result dict by hand in the PyO3 function
> (`binary_readers/read_png_verify_heights/src/lib.rs:88-110`) rather than through `tool_face_io`.
> `NORNIR_CONVENTIONS.md:300-301` acknowledges such crates only as test exclusions.

> **[H] Hooks: "a pure `decide` over `HookInput`, replies built with `hook_io::response`".**
> `MUST_READ_BEFORE_BUILDING.md:35`, `NORNIR_CONVENTIONS.md:200-213`. Code: 7 of 12 hooks do not
> depend on `hook_io` at all (`hook_compact_session_log`, `hook_notify_llm_tts`,
> `hook_session_end_handoff`, `hook_session_start_inject`, `hook_session_start_orient`,
> `hook_stop_llm_tts` — see their `Cargo.toml`). `hook_session_start_inject/src/main.rs:133-140`
> builds `hookSpecificOutput.additionalContext` JSON by hand instead of `SessionStartResponse`.
> `hook_post_llm_read`'s `decide` (`hooks/hook_post_llm_read/src/main.rs:38-60`) reads the
> filesystem (`is_file`, project-root detection, dependency resolution) — not pure.

> **[M] Error registry: adversarial mode and alerting described as a build-time mechanism; no tool
> uses it.** `HOW_A_NORNIR_TOOL_IS_MADE.md:120-130` — tools are built in standard or adversarial
> mode; an adversarial tool sends each refusal to Hlidskjalf as an Alert. Code:
> `core/failure_mode_core/src/lib.rs:48` defines `adversarial_message()`; no other crate calls it
> (grep for `adversarial_message` finds only that file and its test), and no tool has a mode switch
> or refusal-alert path tied to it.

> **[M] Error registry coverage.** `HOW_A_NORNIR_TOOL_IS_MADE.md:104-105` — "no engine writes failure
> text of its own". Engines with their own error text and no `failure_mode_core` dependency include
> `hook_io` (`capability/hook_io/src/lib.rs:111-160` builds decision messages), `syn_core`,
> `format_core` (TOMLX diagnostics, `core/format_core/src/tomlx/diagnostics.rs`), `datagram_io`,
> `session_io`, `intercept_io`, `workspace_registry`, `gleipnir_core`. `syn_cli` mixes both
> (`cli/syn_cli/src/main.rs:41-67` registry; `:303-307` own strings). Whether the rule is meant to
> cover these engines is not stated.

> **[M] Recipes are said to record their user and the three lenses; the recipe formats cannot.**
> `HOW_A_NORNIR_TOOL_IS_MADE.md:85-86, 143-144`. `schemas/tools/gate-recipes.schema.json` —
> `additionalProperties: false`, fields `name/kind/schema/verify_paths` only. `tool_registry.toml`
> entries (and `schemas/tools/tool-registry.schema.json`) have no user/lens fields.
> Also: the doc speaks of one "registry" (`:22-23`); the code has two — `tool_registry.toml`
> (writers only; stores absent) and `gates/gate_recipes.toml` (gates).

> **[M] Deploy: copied vs symlinked.** `nornir_deploy:13-16` and `MUST_READ_BEFORE_BUILDING.md:10` —
> binaries are COPIED, never symlinked. `hooks/HOOK_DESIGN.md:243` — "builds release binaries and
> symlinks into `~/ai/tools/bin/`". `NORNIR_CONVENTIONS.md:266` — bare cargo build "skips symlinks".
> `KFM.md:68` frames the manual failure as `cargo build --release && ln -s`.

> **[L] Adding a crate — two procedures.** `NORNIR_CONVENTIONS.md:284-293` (8 steps incl. workspace
> `Cargo.toml`) vs `MUST_READ_BEFORE_BUILDING.md:130-133` (2 steps: `deploy_categories.toml`, deploy).

> **[L] Binary Structure vs Tool Structure.** `NORNIR_CONVENTIONS.md:100` `run() -> Result<String, EngineError>`
> vs `:126` the tool returns `Result<Option<String>, E>`; real tools follow the latter
> (`capability/tool_face_io/src/lib.rs:47`).

> **[L] `deploy_categories.toml` reference incomplete.** `MUST_READ_BEFORE_BUILDING.md:144-160` omits
> fields the file and `nornir_deploy:18-27` use: `gate_crates`, `gate_verify`, `pyo3_verify`,
> `cli_crates`, `build`. `deploy_categories.toml:10` header still says gates have "separate
> cli_crates (cargo)" — no such key is present; gate CLI faces are discovered from each crate's
> `[[bin]]` (`nornir_deploy:24-27`).

> **[L] Workspace dependency list.** `NORNIR_CONVENTIONS.md:323` "Current:" omits quick-xml,
> rusqlite, coreaudio-sys, enigo, ratatui, crossterm, tokio, hyper, hyper-util, hyper-tls,
> http-body-util (root `Cargo.toml` `[workspace.dependencies]`). A second hand-kept copy of the
> manifest — see Priority 3 note on derivation.

> **[L] Tier rule vs `dep_resolve_core`.** `NORNIR_CONVENTIONS.md:10-11` core = no I/O;
> `KFM.md:87`. `core/dep_resolve_core/src/resolve.rs:347-370` calls `Path::is_file()` (filesystem
> stat) inside the engine; file reads are injected (`read_file` closure, `:80-125`), stats are not.

> **[L] Schema embedding route.** `CONTEXT_MAP.md` refresh table — "which schema a gate embeds → the
> gate's `src/lib.rs` — one `include_str!` through a symlink". Gate `lib.rs` files contain no
> `include_str!`; they `use schemas_embedded::<CONST>` (e.g. `gates/gate_raw_definition_input/src/lib.rs:12`);
> the `include_str!` lives in `capability/schemas_embedded/src/lib.rs`. `schemas_embedded/src/lib.rs:3`
> says schemas "live as symlinks" — most of `schemas/tools/` are regular files.

Verified consistent (no finding): error-type table in `MUST_READ_BEFORE_BUILDING.md:61-76` matches
`failure_modes.toml` class-for-class; writer composition example matches `write_engine` API and
`writers/append_raw_jsonl`; `nornir_deploy` flags in `MUST_READ`/`CONVENTIONS`; no crate pins a
dependency version; no tier violations in `cargo metadata` (core→core, capability→core/capability,
binaries→core/capability only); `datagram_io` is the only owner of the socket path; README floor
rules match `hooks/hook_pre_llm_tool/rules.toml`; KFM `git stash` two-rule claim matches
`hooks/hook_pre_llm_bash/rules.toml:156-161`; all CONTEXT_MAP cross-references to galdr docs and
section headings exist.

### 1.2 `hooks/HOOK_DESIGN.md`

> **[H] `hook_post_llm_read` is a PreToolUse hook.** Doc `:49` — `hook_post_llm_read | PostToolUse`.
> Code `hooks/hook_post_llm_read/src/main.rs:1` "PreToolUse hook", `:35` `run_pre_hook`, usage
> "under PreToolUse"; `~/.claude/settings.json` wires it under `PreToolUse` (matcher `Read`).
> The name's `post_` prefix contradicts the doc's own rule (`:18` "The event prefix IS the verb.
> `pre_` means gate. `post_` means assess").

> **[M] Naming pattern vs actual names.** Doc `:10-16` pattern `hook_{event}_{context}_{scope}`,
> events `pre/post/stop/start/end/compact`, scopes `tool/bash/orient/preserve`. Actual:
> `hook_session_start_orient`, `hook_session_start_inject`, `hook_session_end_handoff` put context
> before event; `hook_notify_llm_tts` uses `notify` (doc `:36` gives Notification no prefix);
> scopes in use include `tts`, `read`, `log`, `inject`, `handoff`. `hush` (`:51`) lives in `cli/`,
> deploys with `[tools]`, has no `hook_` prefix.

> **[M] `HookDecision` variants.** Doc `:103` `Allow | Warn | Ask | Deny`. Code
> `capability/hook_io/src/lib.rs:49-78` also has `AllowWithContext { context }` (used by
> `hook_post_llm_read`).

> **[M] PermissionRequest wire format.** Doc `:34` — `hookSpecificOutput.permissionDecision`. Code
> `capability/hook_io/src/response.rs:270-297` — `hookSpecificOutput.decision.behavior` (+
> `message`). CLAUDE.md's hook_io note agrees with the code.

> **[M] PostCompact absent from the event table; `compact_` mapped to PreCompact.** Doc `:32` maps
> the `compact_` prefix to PreCompact; `:53` lists `hook_compact_session_log` as PostCompact
> (settings.json wires it under `PostCompact`). `hook_io::response` has no PostCompact type
> (`response.rs:734-790`).

> **[M] Side effects.** Doc `:126-131` — "All hooks that produce Warn/Deny/Assessment results" emit a
> datagram, send `terminal-notifier`, append `~/.claude/intercept.log`. Code: only `run_pre_hook`'s
> `emit_decision` does this, for Warn/Ask/Deny (`capability/hook_io/src/lib.rs:189-219, 256-307`);
> `run_post_hook` (`:583-608`) emits nothing; `hook_post_llm_tool` emits a datagram only when saga or
> syn could not run (`hooks/hook_post_llm_tool/src/main.rs:132-148`). The transport is unix socket
> plus UDP multicast (`datagram_io/src/lib.rs:8-10`); `hook_io/src/lib.rs:9-10` says unix socket only.

> **[M] Shared internals / dependencies.** Doc `:166` names `read_stdin()` — no such function in
> `hook_io`. Doc `:222` "Hook dependencies: `hook_io` + `serde_json`" — `hook_post_llm_tool` also
> depends on `write_engine`, `failure_mode_core`, `datagram_io`; `hook_io` itself depends on
> `datagram_io`, `write_engine`, `workspace_registry` (`capability/hook_io/Cargo.toml`).
> Doc `:140` calls `run_hook` "deprecated"; code `lib.rs:347` says "Backward-compatible alias", and 4
> of 5 pre hooks call `run_hook` (`hook_pre_llm_tool:25`, `hook_pre_llm_bash:22`,
> `hook_pre_subagent_tool:16`, `hook_pre_subagent_bash:34`).

> **[M] settings.json wiring.** Doc `:249-267` vs `~/.claude/settings.json`: bash hook has
> `--revert ask` (doc `warn`) and `--chaining block` (doc omits; `hook_pre_llm_bash/src/main.rs:130`,
> `rules.toml [[chaining]]`); PostToolUse `timeout` is `30` (doc `30000`); live wiring also includes
> `hook_post_llm_read` (PreToolUse/Read, `DEP_RESOLVE_MODE=full`), `hush` (UserPromptSubmit), the
> Session/Stop/Notification/PostCompact/SessionEnd hooks — none in the example.

> **[M] `--gaming` category has no rules.** Doc `:254` and settings.json pass `--gaming warn`;
> `hook_pre_llm_tool/src/main.rs:41,67` parses a `gaming` rule array; `hooks/hook_pre_llm_tool/rules.toml`
> contains only `[[floor]]` and `[[probing]]` entries — no `[[gaming]]`. README `:81` advertises
> gaming detection.

> **[L] Workspace member list.** Doc `:230-239` shows 6 hooks; workspace has 12.

> **[L] Timeout budget.** Doc `:313-323` — saga "runs ruff + basedpyright + gleipnir as subprocesses";
> `capability/saga_runner` depends on `gleipnir_core` and calls `run_checks`/`run_checks_v2`
> in-process (`saga_runner/src/lib.rs:188-189`). (Saga
> internals are documented nowhere in the repo — see Priority 2.)

### 1.3 `cli/syn_cli/SYN_DESIGN.md`

Checked against `cli/syn_cli/src/main.rs`, `core/syn_core/src/lib.rs`, and `~/ai/tools/bin/syn --help`.

> **[H] Config discovery.** Doc `:197-217` — syn walks UP from the positional path (or
> `--project-dir`, or cwd) to find `.syn/`. Code `main.rs:69-74, 380-389` — `.syn/` is read only at
> `--project-dir` or the current directory; no walk, and the positional path is never used for
> config.

> **[H] `--project-dir` semantics.** Doc `:245-256` — `syn --project-dir X` and `syn X` "produce the
> same result"; it is the scan root when no path is given. Code `main.rs:392-396` — scan root is the
> positional path or `.`; `--project-dir` sets only the config root and the broadcast workspace.
> `--help`: "Project root for relative paths". `syn --project-dir X` run from elsewhere scans cwd.

> **[M] Gate mode is described as a ratchet and as not having one.** Doc `:108-126, 128-153,
> 179-193` describe gate mode as baseline-vs-new ratchet with `ratchet.toml`; `:331-344` says the
> ratchet is not implemented. Code `syn_core/src/lib.rs:167` — gate decision is `deny` when any
> visible issue passes the deny filter; `ratchet.toml` is never read. Internal contradiction plus
> doc/code gap.

> **[M] Gate mode "needs stdin".** Doc `:113-116, 228` — gate mode needs new `.qa` on stdin. Code —
> stdin is a separate `--stdin` flag usable in either mode (`main.rs:390-391`); gate mode without it
> reads sidecars from disk.

> **[M] CLI reference.** Doc `:223-243` lacks `--stdin`, `--kind`, `--narrow` (all in `--help`);
> gate mode also rejects `--narrow` (`main.rs:200-204`). Code header comment `main.rs:12-15` documents
> `--colored`/`--json` flags that do not exist (`--output` does). (`plans/IMPROVEMENT_PLAN.md:100-102`
> already lists the missing flags as D6.)

> **[M] Exit codes / broadcast.** Doc `:159` exit 2 = usage error; code `main.rs:441-448` exits 2 for
> every `run()` error too, including policy-file refusals that carry a failure class
> (`main.rs:41-67`) — `MUST_READ_BEFORE_BUILDING.md:46` puts refusals at exit 1. Doc `:104, 280`
> broadcast "ON by default"; code `main.rs:410` broadcasts only when there are visible issues or
> faults.

> **[L] Scope.** Doc `:27-28` saga runs on "Python files"; it runs on `.py`, `.rs`, `.svelte`
> (`hook_post_llm_tool/src/main.rs:44-47`; `syn --kind`).

### 1.4 `plans/IMPROVEMENT_PLAN.md`

The plan is not forward-only: it still lists items whose state the code contradicts.

> **[M] Items listed open that the tree shows done or obsolete.** S4 (`senders/announce/` — absent);
> D1/C1 (CONTEXT_MAP inventory — CONTEXT_MAP now stores none, by design); D2 (SessionStart now
> `additionalContext` in `HOOK_DESIGN.md:29`); D3 (example now shows 6 categories — but live has 7,
> see 1.2); D4 (event table now has the 7 types); D5 (`hook_io/src/rules.rs:12` says "Three tiers");
> M2 (gleipnir now has `GLEIPNIR_PHILOSOPHY.md`, `GLEIPNIR_MATRIX.md`, `V2_ZONE_ARCHITECTURE.md`);
> "Stale MEMORY.md entries" (MEMORY.md index is empty). Still open per code: S1
> (`watchers/watch_and_diff_exchange_intercepts/src/main.rs:330-360` own `OpenOptions` append),
> S2, S3 (`hook_stop_llm_tts` has no `cfg(test)`), D6, D7, M1, M3, M4, C2.

### 1.5 `REFRESH.md`, `KFM.md`, `REQUIRED_UPGRADES.md`

> **[L] `REFRESH.md` is an unfilled template.** `:58-86` — every workspace-specific section is a
> placeholder. Nothing links it; `CLAUDE.md` routes recovery through `CONTEXT_MAP.md`.

> **[L] `KFM.md`** — concrete claims checked (stash rules, `hook_io::rules`, `write_engine::ai_home`)
> match code. Its prelude references `/Users/johnny/ai/CONTEXT_MANAGEMENT_SYSTEM.md` (exists). Not
> linked from anything, as it states.

> **`REQUIRED_UPGRADES.md`** — empty of entries; nothing to verify.

### 1.6 `core/format_core/TOMLX_SPEC.md` *[delegated; findings 1 and 5 re-verified by me]*

SPEC = `core/format_core/TOMLX_SPEC.md`; fc = `core/format_core/src`; rio = `capability/reader_io/src/lib.rs`.
The delegated reader ran the deployed `read_tomlx_*` binaries on scratch files to confirm behaviour.

> **[H] Type validation does not exist.** SPEC:641-649, 829-854 — values must match `type=`, mismatch
> is an ERROR. fc: no type check (`tomlx/processor.rs:153-168` records metadata only; no type-error
> variant in `tomlx/diagnostics.rs:138-152`). `[limits] # type=int` with `max = "fifteen"` is
> accepted, and `read_tomlx_json_schema` returns a schema the returned data violates.

> **[H] Metadata blocks never reach reader output.** SPEC:358-363, 516-523, 609-635, 657-684,
> 1058-1065 show `section_units`/`section_paths`/`section_types`/`schema` merged into output.
> `read_tomlx_json` and `read_tomlx_validate_json` return converted data only (rio:345-358);
> `read_tomlx_json_schema` returns `{"data","schema"}` (rio:363-369). The merging
> `TomlxOutput::to_json()` (`tomlx/types.rs:216-269`) is called only from a test. Nothing uses
> `section_units` to generate constants (SPEC:912-925).

> **[H] Any non-annotation trailing comment is an error.** SPEC:135-138, 146 — unannotated sections
> pass through; a `[[list]]` header comment "is for people". fc `tomlx/annotation.rs:52-61, 101-104,
> 207-227, 283-286`, `tomlx/validation.rs:192-199` — every field and header comment is parsed as
> `key=value`; `name = "x"  # display name` is refused (`parse_error`).

> **[M] Case.** SPEC:50 case-sensitive; SPEC:313, 554 case-insensitive (internal contradiction).
> fc lowercases keys (`tomlx/annotation.rs:106, 288`) and base names but compares `path=`
> references as written (`tomlx/processor.rs:510`) — `Logs=/var/` + `path=Logs` fails as undefined.

> **[M] Other behavioural disagreements** (SPEC line → code):
> ` // ` human comments only split on fields (SPEC:92-116 → `tomlx/annotation.rs:274-281` vs `:92`);
> later-section base redeclaration only refused for already-declared names (SPEC:450-451 →
> `tomlx/validation.rs:127-143`); trailing `/` and `.` stripped (SPEC:490, 509-519 →
> `tomlx/paths.rs:157-184`, code's own test `:282`); join-then-expand order (SPEC:488-489 →
> `tomlx/paths.rs:24-32`); `path=absolute` still expanded/normalised (SPEC:474, 485 →
> `paths.rs:24-33`); `path=config` with a relative file yields a relative value (SPEC:486, 514 →
> rio:327); reserved base names refused (SPEC:446 → `tomlx/types.rs:179-190`); no "Invalid path
> reference" error (SPEC:870-877); `section_types` stores canonical not declared type string
> (SPEC:696 → `types.rs:93-101`); no `$schema`/draft-07 marker (SPEC:691 →
> `processor.rs:211-251`); extra time aliases `s/sec/secs/millis` and `target=ms` recorded as
> `milliseconds` (SPEC:203, 313 → `units.rs:101-102, 277`); multi-line arrays converted (SPEC:155);
> whole floats become integers (SPEC:322 → `processor.rs:456-462`); multiple field annotations not
> split (SPEC:978 → `annotation.rs:283-289`); TOML parse errors are the `toml` crate's text, not the
> SPEC:893-906 format; SPEC §6 example messages differ from `tomlx/diagnostics.rs` text and several
> lack the promised `Rule:`/`Fix:` (SPEC:720-730); the semantic-validation example
> `{"max": 604800}` (SPEC:929-944) is not a JSON Schema keyword and would pass.

> **[M] Non-JSON TOML values.** `NORNIR_CONVENTIONS.md:67` — readers refuse TOML date-times,
> infinities, NaN as `file_value_not_json` (`fc/parse.rs:328-340` does). The tomlx path has its own
> converter (`tomlx/processor.rs:68-86`): date-times → strings, non-finite → `null`, exit 0. SPEC is
> silent. Two converters for one input format.

> **[L] Reader docs vs SPEC.** Reader crate docs say "`base=config` references"
> (`readers/read_tomlx_json/src/lib.rs:5` and siblings, rio:324); SPEC:476, 502 and code use
> `path=config`. `read_tomlx_json_schema/src/lib.rs:4` says it fails when the file has "no
> annotations"; it fails when there are no *type* annotations (rio:362-367). All tomlx errors reach
> callers as `parse_error`/`file_not_in_format` (rio:126, 329-333) — SPEC does not say.

> **[L] Implemented, not specified.** Extra error kinds (`diagnostics.rs:226-270, 340-353`;
> `annotation.rs:138-150, 169-185`); root-level fields can never be annotated so
> `process_root_fields` is unreachable (`validation.rs:84-88`, `processor.rs:104-109, 288-315`);
> `+` and lower-case hemisphere in angles (`angle_parse.rs:91, 105-106`); unparseable
> annotation-looking `[[list]]` comments silently kept (`annotation.rs:74-83` vs SPEC:146); bases
> with unset env vars kept unexpanded until used (`paths.rs:198-199` vs SPEC:161). SPEC:762-764 uses
> `mins` as an unknown unit though SPEC:198 and `units.rs:107` accept it.

> **[L] format_core module docs.** `fc/lib.rs:4` Markdown "parse only" — `serialize.rs:35, 333`
> write it; `fc/lib.rs:6-11` module list omits `tomlx`, `write_verify`; Dependency Lookup
> (`NORNIR_CONVENTIONS.md:227`) omits XML and Markdown.

### 1.7 `core/gleipnir_core/*.md` and `.claude/plans/gleipnir_false_positive_reduction.md` *[delegated; A1 and F1 re-verified by me]*

Docs: RECOVERY = `RECOVERY_FROM_EMBARRASSING_FAILURE.md`, ZONE = `V2_ZONE_ARCHITECTURE.md`,
GUIDE = `V2_MIGRATION_GUIDE.md`, PLAN = `V2_MIGRATION_PLAN.md`, PHIL = `GLEIPNIR_PHILOSOPHY.md`,
MATRIX = `GLEIPNIR_MATRIX.md`, RFP = `RUST_FALSE_POSITIVE_REDUCTION.md`, CPLAN =
`.claude/plans/gleipnir_false_positive_reduction.md`. Code under `core/gleipnir_core/src/` unless
noted.

Verified consistent: MATRIX ↔ `gleipnir_matrix.toml` (22 classifications, no cell differences);
every `gleipnir_messages.toml` entry referenced in code; CLAUDE.md's `clone_is_ownership_transfer()`
note (`checks_rs/prohibited.rs:519`, called by `no_clone_spam` :358 and `no_string_abuse` :587,
:593); `FileKind` 8 variants (`structures.rs:5-14`).

**V2 status**

> **[H] Opt-in vs default.** RECOVERY:38, 44; GUIDE:13, 202; PLAN:26, 81-86, 171 — projects opt in via
> `saga_runner/v2_projects.toml`; "Projects not in the list see zero change". Code: no
> `v2_projects.toml`; `capability/saga_runner/v1_legacy_projects.toml` is an opt-*out* list;
> `saga_runner/src/lib.rs:89-95` routes to V2 unless listed; `saga --test v1|v2` overrides.

> **[H] "No project migrated" vs migrated trees.** RECOVERY:44, 48 vs GUIDE:5 ("Based on the regin
> migration") and `~/ai/smidja/{regin,draupnir}/src/*/{logic,structure}` (no `functions/`).

> **[M] Plan describes as future what exists.** PLAN:13-26 "Current State", Phase 2 (:45-99), 3a/3b
> (:107-139) vs `classify_file_v2` (`classify.rs:132`), `checks_for_v2` (`matrix.rs:228`),
> `run_checks_v2` (`lib.rs:95`); PLAN:119 step already true in draupnir. RECOVERY:42 says
> `CheckConfig::for_v2()` is needed — exists (`structures.rs:202`, `gleipnir_statistics.toml:69-145`).
> PLAN:84-85 version parameter — saga_runner instead picks between two functions
> (`saga_runner/src/lib.rs:188-189`).

**Level and zone model**

> **[H] Level from directory vs from filename.** RECOVERY:25, PLAN:56, 132, 134, PHIL:52, 69 put the
> level in a directory (`logic/pure/primitive/`). Code takes a file's level from its filename
> (`classify.rs:68-83`); a level directory is honoured only for import paths, labelled "backward
> compat" (`classify.rs:88-111, 225`), and that table lacks `assembled`. `README.md:33-43` agrees
> with the code.

> **[M] Numbering.** ZONE:66 dispatch L2/L5, primitive/ffi L0 vs ZONE:70-78, 177-185 and code
> (dispatch L3/L6, primitive/ffi L1; `classify.rs:68-83`, `structures.rs:30-38`) — internal
> contradiction. RECOVERY:19 and PLAN:21 name a level set without dispatch/assembled; code `Level`
> L0-L8 + `Outside` (`structures.rs:29-40`). RECOVERY:21, 30 Level 8 variants / Zone 5; code Level 10
> / Zone 9 incl. `StructureGen/Model/Config/Example` (`structures.rs:77-87`).

> **[M] Import legality mechanism.** PLAN:22-23, 66-68 `static LEVEL_IMPORTS`/`ZONE_IMPORTS` tables vs
> ZONE:102 "No lookup table" vs code numeric compare + `match` (`structures.rs:64-69, 108-117`).

> **[M] Structure sub-zones — three answers.** ZONE:21-24, 142-145 `model/config/exception`; GUIDE:203
> `gen/schema/`, `gen/fragment/`, `model/`, `config/`; code `gen/model/config/example` + `/structure/`
> fallback, `exception/` lands on fallback (`classify.rs:56-60`); MATRIX:81 columns Gen/Model/Config/Exm.

> **[M] Entry points.** ZONE:186-187, PHIL:67 — `cli.py`/`__main__.py` at project root; code matches
> them anywhere (`classify.rs:114, 136-141`). GUIDE:40 maps to `logic/entry_point.py`, which code
> classifies Outside/Structure and flags "file outside v2 zone layout" (`classify.rs:148-153`,
> `checks_py/imports.rs:670-675`).

> **[M] `__init__.py` and unzoned files.** ZONE:188 and `classify.rs:126` say `__init__.py` gets no
> checks; code routes `Level::Outside` to the zone's full list (`matrix.rs:233-254`) with L8 settings
> (`structures.rs:212`). Files outside every zone get structure-zone checks (`classify.rs:148-153`,
> `matrix.rs:241`) — undocumented, no MATRIX column.

> **[M] Impure imports.** ZONE:128 "impure is free" vs ZONE:112 and `structures.rs:113` (impure → impure
> and pure only); ZONE:221 checklist omits the impure ban.

> **[L] Checklist and transform limits.** ZONE:218-225 lists 6 checks; MATRIX:99-135 / `matrix.rs:47,
> 79-85` enforce more (`v2_structure_bases`, `v2_structure_import_boundary`,
> `v2_dispatch_only_tables`, `v2_classes_only_in_structure`, `unknown_file_in_zone`,
> `no_before_validators`); RECOVERY:37 says "4 new check types". ZONE:157 stricter transform limits
> in general — overrides only at L1/L2/L4 (`gleipnir_statistics.toml:135-145`).

**Complexity bands**

> **[M] CC=1 in L2.** ZONE:84, PHIL:69, `gleipnir_messages.toml:215` say it is a gravity violation;
> ZONE:72, GUIDE:95, 112, 122 and code `[v2.l2] min_cc = 1` (`gleipnir_statistics.toml:77`) say not.
> RECOVERY:23 bands (simple 2-3, composed 4+, orchestrate 1-2) vs code (L2 1-3, L4 4-8, L7 1-5).
> GUIDE:110 CC counting rule omits comprehension `for`/`if`, `case`, `assert`, loop/try `else`
> (`checks_py/style.rs:640-703`). Code comment `style.rs:738` (no CC bounds for EntryPoint/Outside)
> vs `structures.rs:212` + `[v2.l8]` min 1 / max 2.

**Check scope**

> **[L]** PHIL:84 `json` banned in pure zones — banned in every V2 column and every V1 column except
> Script/Test (`imports.rs:318-323`, MATRIX:32, 97). GUIDE:133 DI "not a valid solution" vs GUIDE:213
> "resolved (DI or level adjustment)"; `no_callable_params` blocked in every V2 column (MATRIX:93).
> `max_functions_outside_zones` set for L3/L6/L8 (`gleipnir_statistics.toml:89, 111, 126`) but its
> check is in no V2 row. PHIL:5 Python-only vs Python/Rust/Svelte/TS (`lib.rs:132, 173`). RECOVERY:35
> matrix "hard-coded in matrix.rs" — data-driven from embedded TOML (`matrix.rs:143-206`). PLAN:14
> FileKind includes "project Rust" — FileKind is Python-only; Rust checks are a fixed list
> (`lib.rs:137-154`).

**Matrix editing chain**

> **[H] Which file to edit.** `matrix.rs:7-8` "edit `gleipnir_matrix.toml`" vs `gleipnir_matrix.toml:3`
> "DO NOT EDIT — edit GLEIPNIR_MATRIX.md and run generate_matrix_toml.py". The TOML carries the
> `schg` flag; the MD does not; the generator overwrites the locked file.

> **[M] Matrix scope unstated.** MATRIX covers Python only; Rust, TS-in-Svelte, CSS, HTML checks and
> their severities are fixed lists in `lib.rs:137-154, 203-213, 234-239, 260-262, 275-277`.
> `no_closures` fires from inside `check_no_nested_functions` (`checks_py/prohibited.rs:316, 352`)
> with no MATRIX row. This contradicts the system-level claim that the matrix is the single source of
> truth for which checks run.

**Fuzzy-returns policy**

> **[M] Thresholds revealed.** Policy: `gleipnir_messages.toml:17-18` ("MUST NOT reveal thresholds,
> counts, or exact limits"), CLAUDE.md note. `check_v2_cc_level` emits `min CC={min_cc}` and
> `maximum of {max_cc}` (`checks_py/style.rs:758, 765`); `gleipnir_messages.toml:215` states CC bands.

**The two Rust false-positive plans (RFP and CPLAN)**

> **[M] Both read as pending; every change is in the code.** Change 1 (`checks_rs/prohibited.rs:448-516`,
> hooked :544), Change 2 `RustCrateTier`/`classify_rust_tier`/`in_run_function` (:231-275, :311),
> Change 3 `mutable_specifier` (:555-559). `is_implicit_return` (RFP:33, 82, 124; CPLAN:53, 113, 122,
> 126) no longer exists — `is_last_expression_in_block` (:431), `is_value_producing_position` (:448).
> Stale line refs (RFP:17, 66, 216; CPLAN:130 → code :519, :547, :286, :544).
> RFP:308 / CPLAN:171 `run()` exemption "deliberately narrow" — code also accepts `run_*`
> (`prohibited.rs:270`). RFP:19 "collection methods" are transfers — code treats every
> `call_expression` (`prohibited.rs:532-535`). RFP:410-411 `contains("let mut")` vs CPLAN:55-57 and
> code `mutable_specifier`. RFP:517, 524 classifier in `classify.rs` vs CPLAN:158 and code
> `prohibited.rs:233`. RFP:54, 62 vs CPLAN:39 describe `convert.rs:199` differently (not checked).

> **[M] Rebuild after a gleipnir change — three answers.** RFP:537 `--build gates`; CPLAN:218 gates
> then tools; PLAN:150 tools. Only `saga_runner` depends on `gleipnir_core`; it ships via `saga_cli`
> in `[tools]`.

### 1.8 `interceptors/INTERCEPT_DESIGN.md` *[delegated; items re-verified: bifrost_proxy header, its raw log / thinking log / own schema copy]*

> **[H] A second live interception path the doc never names.** INTERCEPT:3 (dated 2026-03-19),
> :106-128, :235 — live interception is the PyO3 `traffic_interceptor_rewriter` imported by Python
> `intercept_and_route.py` under mitmproxy. `daemons/bifrost_proxy/src/main.rs:1-10` — a Rust reverse
> proxy "replacing mitmproxy" (nginx:443 → :4000 → nginx:8443) that re-implements the orchestration
> inline (`main.rs:227-285`, mirroring `traffic_interceptor_rewriter/src/lib.rs:73-120`). Both are
> deployed (`~/ai/tools/bin/bifrost_proxy`, `~/ai/tools/lib/traffic_interceptor_rewriter*.so`);
> `~/ai/smidja/bifrost/src/bifrost/functions/impure/intercept_and_route.py:9` still imports the
> module. No document says which path carries traffic.

> **[H] Raw capture is not "unconditional" in bifrost_proxy.** INTERCEPT:113, 130-134 vs
> `bifrost_proxy/src/main.rs:237` (UTF-8 checked first), `:471-478, 515-529` (non-JSON bodies go to
> `sessions/unresolved/{ts}_req.raw`), `:486-497` (skipped for session id `unknown`). The rewriter
> writes raw bytes before UTF-8 check and without fsync (`traffic_interceptor_rewriter/src/lib.rs:35-51,
> 84-87`); bifrost_proxy fsyncs.

> **[M] Ownership and outputs.** INTERCEPT:79 — `raw_session_log.jsonl` written by
> "`traffic_interceptor_rewriter` only"; `bifrost_proxy/src/main.rs:239-241` writes it too.
> INTERCEPT:59-85 omits `sessions/unresolved/` and `thinking_log.jsonl` — bifrost_proxy strips
> thinking blocks from the SSE response, renumbers block indices and logs them (`main.rs:535-821,
> 794-816`). INTERCEPT:32 says the watcher is "spawned by replay"; bifrost_proxy also spawns one per
> session and creates the `traffic/{ws}/{sid}` symlinks (`main.rs:291-367`).

> **[M] Schema copies.** INTERCEPT:211-213 — schema embedded in rewriter and replay (symlink).
> `daemons/bifrost_proxy/cc_wire_schema.json` is a third, regular-file copy (`main.rs:83-84`);
> byte-identical today, nothing keeps it so.

> **[M] Validation failure.** INTERCEPT:220 "returns an error to bifrost (which logs and continues)" —
> true of the rewriter (`lib.rs:93-98`); bifrost_proxy logs and forwards the original bytes
> (`main.rs:248-255`). External `~/ai/projects/network-proxy/DESIGN.md:106` calls it "a hard error".

> **[M] Deploy.** INTERCEPT:228-236 — `nornir_deploy --build interceptors`, then restart mitmproxy.
> bifrost_proxy is in `[daemons]` (`deploy_categories.toml`), not `[interceptors]`; no restart
> procedure documented for it.

> **[M] Crate boundaries omit two crates.** INTERCEPT:18-34 lacks `capability/intercept_io` (PyO3
> module: `json_to_toml`, `append_jsonl_line`; no Python consumer under `~/ai/smidja`, in no deploy
> category, excluded from tests) and `rewriters/rewrite_compaction_summary` (stdin/stdout CLI, no
> callers found).

> **[M] Replay raises live alerts.** `session_io::record_compaction` emits a High-priority Alert per
> compaction (`capability/session_io/src/lib.rs:111-125`); replay calls it for every historical
> compaction, tagged workspace `replay` (`interceptors/intercept_replay/src/main.rs:44-45`).
> INTERCEPT:197 does not mention it.

Verified consistent: classification table (INTERCEPT:91-98 ↔ `intercept_core/src/lib.rs:57-73`);
session_io function set and compaction steps; truncation keeps the inode; replay skips inject,
archives, sleeps, kills watcher; PyO3 return shape.

### 1.9 `core/compaction_inject_core/instructions/compaction_summary.md` *[delegated]*

> **[M]** Embedded whole by `core/compaction_inject_core/src/lib.rs:10` (`include_str!`), so its HTML
> header comment (lines 1-5: "for the SDK", `ccVersion: 2.1.38`) is injected into live requests
> (`lib.rs:21-24`). It is *appended* to `system[]` (`lib.rs:15-27`), so Claude Code's own compaction
> instructions remain alongside it; no nornir document says which is meant to win. Reached by
> `traffic_interceptor_rewriter` (`lib.rs:66`), `bifrost_proxy` (`main.rs:279`), and
> `rewrite_compaction_summary` (no callers).

### 1.10 `cli/cc_launch/CC_LAUNCH_DESIGN.md` *[delegated]*

Several items are already in the doc's own Pending list (DESIGN:251-269); those are marked
*(self-flagged)* and listed only where the code has moved since.

> **[M] Assembly order** *(self-flagged, :261)* — DESIGN:46-56 puts the collaboration paradigm under
> `cognitive-mode/` and persona 7th; code loads `foundation/` first (`library.rs:19-24`,
> `assembly.rs:89-91`), persona 2nd (`assembly.rs:93-98`). `ABOUT:48-49` agrees with the code.

> **[M] Languages.** DESIGN:66 — languages emit `<context><language>…</language></context>`; code
> appends `coding/<language>/*.xml` fragments, no such tag (`library.rs:53-59, 155-185`,
> `assembly.rs:128-133`). DESIGN:101, 159 — "Python always included"; code forces nothing, languages
> are the union of selected descriptors, all off at start (`model.rs:207-209, 257-290`). Only
> `coding/python/` exists on disk; `rust`/`svelte` descriptor languages match nothing and are skipped
> silently (`model.rs:282-285`) though the TUI sketch (DESIGN:221, 226-227) shows `rust`.

> **[M] Paths and tags.** Expertise `expertise/*/` (DESIGN:67) vs flat `expertise/*.xml`
> (`library.rs:61-66, 241-283`). Systems root tag `<workspace>` (DESIGN:75) vs `<system>` in every
> `~/ai/control/library/systems/*.xml`. Prompt wrapper `<prompt version=… persona=… workspace=…
> coding=…>` (`assembly.rs:165-168`) and the refusal on unfilled `{UPPER_CASE}` template variables
> (`assembly.rs:158-162`) are undocumented.

> **[M] Launch path and outputs.** DESIGN:172-182, 308-316 — one exec, `cc_launch` /
> `cc_launch -- --model sonnet`. Code/`--help`: `--update`, `--session`, `--workspace`, `--pick`,
> `--go`, `--spec-workspace` (`main.rs:22-53`); real launch is pick → shell `cd` → go via the
> `cc_launch()` function in `~/.zshrc:30-47`; `--pick` never execs. Output `/tmp/cc_launch_prompt.xml`
> (DESIGN:179, 239, 306) vs `std::env::temp_dir()` (`assembly.rs:72`); undocumented outputs
> `<workspace>/.SYSTEM_PROMPT.xml` (`:76-78`), `$TMPDIR/cc_launch_spec.toml` (`:234-236`), and
> `--update` writing `~/ai/control/workspaces/<ws>/<session>/SYSTEM_PROMPT.xml` (`:174-198`).
> Continuation (DESIGN:184-193) omits the orient/inject hooks that carry the prompt across
> compaction (`hook_session_start_orient/src/main.rs:87-99`, `hook_session_start_inject`).

> **[L] TUI and selection details.** Column layout and Tab order (DESIGN:232-233 vs `tui.rs:197-230`,
> `model.rs:119-145, 223`); profile fills persona only when none was picked (DESIGN:143 vs
> `model.rs:94-97, 398-409`); `domain` attribute on expertise and coding toggling `domain="coding"`
> modules (`library.rs:248, 264-271`; `model.rs:504-524`) absent from DESIGN:208; descriptor family
> lists incomplete (DESIGN:87-91 omit `nornir-gates`, `cc-launch`, `hook-chain`, and standalone
> `announce`, `control-panel`, `relay-mic-to-voice`, `workspace-registry`).

> **[L] Pending items the code has moved past** — DESIGN:252 (Auto starts on Bragi) vs
> `DEFAULT_PERSONA_ID = "odinn"` (`model.rs:146-168`); DESIGN:251 Auto never loads the space vs
> launch-dir fallback (`model.rs:297-312`, runtime unconfirmed); DESIGN:254 Heimdallr has no space or
> profile vs `~/ai/control/library/spaces/heimdallr.xml` and `[heimdallr]` in
> `~/ai/control/cc_launch_profiles.toml:95-98`; DESIGN:269 huginn/muninn now in `personas/pending/`
> (not scanned — `library.rs:118-150` does not recurse); DESIGN:284 CWD workspace partly done
> (`model.rs:98-100, 297-302`). Still true: DESIGN:253, 255, 265, 266, 267, 274, 300-305.

> **[M] Contradictions outside the doc.** `~/ai/control/cc_launch_profiles.toml:3` "Permissions are
> never auto-selected — always manual in TUI" vs DESIGN:107-111 and `permissions.rs:1-11`
> (persona-bound, no TUI control). `~/ai/control/library/systems/cc-launch.xml` (loaded into every
> prompt that selects it) says `.SYSTEM_PROMPT.md` (code writes `.xml`, `assembly.rs:77`), says
> cc_launch "never writes to the control panel" (`--update` does), and describes `--workspace nornir`.

### 1.11 `cli/cc_launch/explainers/*` *[delegated]*

> **[L]** `About This Project.md` breaks `HOW_TO_WRITE_EXPLAINERS.md`: missing required headings and
> order (HOWTO:95-105 vs ABOUT:20, 41); names `CC_LAUNCH_DESIGN.md` (ABOUT:89 vs HOWTO:144 "no file
> names"); no sibling link (HOWTO:102 vs ABOUT:71-73).
> **[M]** ABOUT:78-80 claims a pending module with invented env-var names; `permissions.rs` uses
> `HOOK_LLM_ALLOW_PATHS`/`HOOK_LLM_ALLOW_PATTERNS`, consumed by `hooks/hook_pre_llm_tool/src/main.rs:10`,
> and DESIGN's Pending list has no such item. ABOUT:80-85 names a memory-bootloader interim fix and
> traffic-layer injection as permanent; the SessionStart inject hook already re-injects, and
> `bifrost_proxy` has no `SYSTEM_PROMPT` handling.

### 1.12 `cli/announce/ANNOUNCE_NOTES.md` *[delegated; hook-name item re-verified]*

> **[L] Hook name.** NOTES:28 `hook_start_session_orient` — crate is `hooks/hook_session_start_orient`.

> **[M] Settings priority.** NOTES:35 "CLI > VOICE.lock > `--profile`/`--severity` > default". Code:
> `resolve_settings` takes no severity (`core/announce_core/src/lib.rs:86-94`; `main.rs:160-169`);
> `model` has no CLI/VOICE.lock layer (`lib.rs:121-125`); `speed` has an extra voice-definition layer
> (`lib.rs:128-134`). NOTES:36-37 itself calls severity "metadata only".

> **[M] Locks.** NOTES:30-32 puts SILENT and QUIET under announce; announce checks only `SILENT.lock`
> (`cli/announce/src/main.rs:104-112`); `QUIET.lock` is checked by `hooks/hook_stop_llm_tts/src/main.rs:73-80`.
> `RECORDING.lock` and `~/ai/voice/queue.jsonl` (`main.rs:116-124, 244-284`; written by
> `daemons/relay_mic_to_voice/src/main.rs:160`) are not documented.

> **[L]** Speed clamp 0.7–1.2 listed under "ElevenLabs specifics" (NOTES:43) applies to every backend
> (`lib.rs:146`). Default backend is kokoro (NOTES:11, `lib.rs:42-48`) but `--help` says "ElevenLabs
> TTS CLI" (`main.rs:18`).

> **[M] Undocumented coupling.** `hush` kills playback with `pkill -f "announce.*--play.*--source {dir}"`
> (`cli/hush/src/main.rs:47-50`), relying on announce's self-exec argument shape (`main.rs:504-507,
> 526-529`). Renaming announce's internal flags would break `hush` silently.

---

## Priority 2 — Missing design rationale

Multi-crate subsystems with no committed document explaining *why* they are shaped as they are.
"Partly" means the *what* is stated somewhere but the design decisions are not.

> **Datagram / Hlidskjalf transport** — `datagram_core`, `datagram_io`, 5 `send_*`,
> `record_datagrams`, `watch_and_diff_exchange_intercepts`, consumers in `hook_io`, `syn_cli`,
> `hook_post_llm_tool`. No doc. Undocumented decisions a session would break: dual transport (unix
> stream to the archive daemon + UDP multicast 239.0.0.1:9899 for live consumers); self-alerting on
> transport failure and on >65000-byte payloads; three emit paths (`emit` / `emit_validated` /
> `emit_validated_or_alert`) and which caller gets which; colon workspace separator (Svelte reason —
> already contradicted by CLAUDE.md, 1.1); `record_datagrams` as the only persistent record that
> adversarial-mode alerting depends on. Still listed as M1 in `plans/IMPROVEMENT_PLAN.md`.

> **Saga** — `saga_core`, `saga_runner`, `saga_cli`, `schemas/tools/sanity-report.schema.json`,
> `.qa` sidecars. `SYN_DESIGN.md` describes syn; nothing in the repo describes saga: what goes into a
> sidecar, the content hash and staleness, why gleipnir is linked while ruff/basedpyright are
> subprocesses, why `syn_cli` imports `saga_runner` (`SanityReport`, `find_files`, `qa_path`) rather
> than `saga_core`. A session changing the sidecar format has no reference for the three consumers
> (syn, svalinn, `hook_post_llm_tool`).

> **Session lifecycle / prompt re-injection** — `hook_session_start_orient`,
> `hook_session_start_inject`, `hook_session_end_handoff`, `hook_compact_session_log`,
> `workspace_registry`, plus `compaction_inject_core` / `rewrite_compaction_summary` on the proxy
> side. `HOOK_DESIGN.md:54-56` gives one line each. Undocumented: the `CLEAR_HANDOFF.xml` slot and
> its freshness rule, the integrity banner for oversized injections
> (`hook_session_start_inject/src/main.rs:7-11`), why these hooks bypass `hook_io`, the registry's
> DB location and longest-prefix resolution. `workspace_registry` (used by 6 hooks and `hook_io`)
> is not in the Dependency Lookup.

> **Dependency-context injection** — `dep_resolve_core`, `dep_resolve_file`, `dep_resolve_function`,
> `hook_post_llm_read`. No doc. Undocumented: why a dependency resolver fires as a PreToolUse Read
> hook under a `post_` name; the control surface (`DEP_RESOLVE_DISABLE.lock`, `DEP_RESOLVE_MODE`,
> `DEP_RESOLVE_MAX_DEPTH` — only in `hook_post_llm_read/src/main.rs:13-16`); why I/O is injected as a
> closure into a core crate.

> **Binary formats / elevation** — `binary_format_core`, `elevation_core`, `binary_reader_io`,
> `binary_writer_io`, `read_png_verify_heights`, `write_bytes_verify_png`,
> `write_bytes_verify_quantized_mesh`. The naming rule is documented (`NORNIR_CONVENTIONS.md:71-73`);
> what they serve, who calls them, the Terrarium encoding and repair rules, and why they have no CLI
> face (1.1) are not.

> **Gate generation and defaults** — `generate_gates.py`, `gates/gate_recipes.toml`, `gate_io`,
> `default_apply_core`, `default_apply_io`, `gate_raw_definition_defaults`. The gate concept is
> documented externally (`~/ai/smidja/galdr/context/NORNIR_GATES_ELEMENT.md`); in-repo, nothing
> explains the four gate kinds, why a defaults gate writes and returns data, or what
> `default_apply_core` does with `if/then/else` (only its `//!` header).

> **Writers and stores** — partly: `NORNIR_CONVENTIONS.md:154-198` covers the API, stamps, receipts.
> Not covered: `store_engine`'s session model (the OPEN.jsonl → `sessions/<id>.jsonl` + index cycle),
> who consumes `tool_registry.toml` ("agent definition L8 resolution" per its header — regin), why
> stores are absent from that registry. `store_engine` and `stamp_core` appear only in prose, not the
> lookup table. (Plan M3 still open.)

> **Presence tools** — `presence_io`, `create_flag_generate_lock`, `remove_flag_generate_lock`.
> Contract documented; what `GENERATE.lock` releases and which generator reads it is stated nowhere
> outside the crate header.

> **Announce / voice** — `announce_core`, `announce`, `hush`, `hook_stop_llm_tts`,
> `hook_notify_llm_tts`, `relay_mic_to_voice`, `keystroke_io`, the speech path in `hook_io`
> (`speak_alert`, `capability/hook_io/src/lib.rs:403-440`). `ANNOUNCE_NOTES.md` is 44 lines of notes
> (1.12). Undocumented: the three-lock model split across crates (SILENT in announce and hook_io,
> QUIET in the stop hook, RECORDING in relay/announce), the queue drain, the `hush` ↔ announce
> argument-shape coupling, severity→voice mapping in `hook_io`. Plan M4 still open.

> **Live traffic interception (bifrost)** — `bifrost_proxy`, `traffic_interceptor_rewriter`,
> `intercept_replay`, `intercept_core`, `intercept_io`, `session_io`, `compaction_inject_core`,
> `watch_and_diff_exchange_intercepts`, `rewrite_compaction_summary`. `INTERCEPT_DESIGN.md` explains
> why `session_io` was extracted and why replay skips inject — but only for the PyO3 path. Nothing
> in nornir explains why `bifrost_proxy` exists beside it, which is live, why it copies the
> orchestration instead of sharing it (the drift `INTERCEPT_DESIGN.md:36-45` says the extraction was
> meant to prevent), the nginx chain, the SSE thinking-strip, or what `intercept_io` and
> `rewrite_compaction_summary` are for. The external design authority
> `~/ai/projects/network-proxy/DESIGN.md` exists and covers the why; no nornir document points to it,
> and `INTERCEPT_DESIGN.md` is not routed from `CONTEXT_MAP.md`.

> **Gleipnir internals** — guidance exists (PHIL, ZONE, MATRIX) but not for: why only Python is
> matrix-driven while other languages are fixed lists in `lib.rs`; the `checks_*` split (by
> language, then action; `checks_ts` runs only on Svelte `<script>`; `checks_svelte` holds CSS/HTML);
> the compile-time chain a new check must satisfy (`build.rs:35-40` messages → constants and
> build failure, `matrix.rs:128-137` const assertion against `CHECK_REGISTRY`, `{readers}` filled
> from repo `deploy_categories.toml` at `build.rs:14-71`, `stamp()` severity rules `lib.rs:36-47`);
> the MD → `generate_matrix_toml.py` → `schg`-locked TOML chain; the V2 fallbacks (unzoned →
> Structure, `__init__`/unknown → Outside with L8 settings, scripts/tests always V1 inside
> `run_checks_v2` `lib.rs:105-112`, thread-local `--test` override); which level layout (filename vs
> directory) is canonical.

Architectural decisions with no written home:
- Why `intercept_io` (a capability crate) has a non-optional `pyo3` dependency, and why
  `traffic_interceptor_rewriter` is a PyO3 module while other interceptors are binaries — only
  visible as test exclusions in `NORNIR_CONVENTIONS.md:301`.
- Why two TOML→JSON converters exist (`format_core::parse` vs `format_core::tomlx`) with different
  treatment of non-JSON values (1.6).
- Which engines are inside the error-registry rule (1.1).

What a fresh session would most likely get wrong:
- Write a new `post_` hook expecting PostToolUse semantics from `hook_post_llm_read` as precedent.
- Run `syn --project-dir X` expecting X scanned; place a `.syn/` in a subdirectory expecting walk-up.
- Trust a `type=` annotation in a `.tomlx` file to be enforced.
- Build a workspace label with `/` from CLAUDE.md.
- Run `nornir_deploy --build all` from README.
- Model a new binary tool on the binary writers and ship it without a CLI face.
- Add a project to `v2_projects.toml` (does not exist) to "opt in" to V2, which is already the default;
  or lay out a migrated project with level *directories* per RECOVERY/PLAN/PHIL.
- Edit `gleipnir_matrix.toml` per `matrix.rs:7-8` (locked, generated) instead of `GLEIPNIR_MATRIX.md`.
- Implement the two Rust false-positive plans again, or rebuild `gates` after a gleipnir change.
- Change the live interception path in only one of `traffic_interceptor_rewriter` / `bifrost_proxy`,
  update one `cc_wire_schema.json` copy, or restart mitmproxy when the Rust daemon carries traffic.
- Expect transcripts to equal what the model saw, not knowing `bifrost_proxy` strips thinking blocks.
- Rename announce's internal `--play` arguments and silently break `hush`.

## Priority 3 — Contradictions between documents

> **Deploy mechanism** — `MUST_READ:10`, `nornir_deploy:13` (copy) vs `HOOK_DESIGN.md:243`
> (symlink), `NORNIR_CONVENTIONS.md:266` ("skips symlinks").

> **Deploy-everything command** — `README.md:143` `--build all` vs `NORNIR_CONVENTIONS.md:269`,
> `MUST_READ:121` `--all`.

> **Workspace identity** — `CLAUDE.md:28` (`datagram::`, `/`) vs `NORNIR_CONVENTIONS.md:256`
> (`datagram_io::`, `:`).

> **Adding a crate** — `NORNIR_CONVENTIONS.md:284-293` vs `MUST_READ:130-133`.

> **Hook naming** — `HOOK_DESIGN.md:10-18` (event prefix is the verb) vs
> `NORNIR_CONVENTIONS.md:45` (`hook_` = intercepts a tool call) vs the 12 hook names.

> **Syn gate mode** — `SYN_DESIGN.md:108-153` (ratchet) vs `:331-344` (not implemented);
> `HOOK_DESIGN.md:338` agrees ratchet is unbuilt; `README.md:61` describes gate mode as a
> config-locked pass/fail, which matches the code.

> **TOMLX case sensitivity** — `TOMLX_SPEC.md:50` vs `:313, 554`.

> **V2 level/zone model** — level set by directory (RECOVERY, PLAN, PHIL) vs by filename (README,
> code); dispatch L2/L5 (ZONE:66) vs L3/L6 (ZONE:70-78); structure sub-zones (ZONE vs GUIDE vs
> MATRIX); CC=1 at L2 (ZONE:84, PHIL:69 vs ZONE:72, GUIDE); opt-in (RECOVERY, GUIDE, PLAN) vs
> migrated (GUIDE:5).

> **Gleipnir rebuild category** — RFP:537 vs CPLAN:218 vs PLAN:150.

> **Matrix edit target** — `matrix.rs:7-8` vs `gleipnir_matrix.toml:3`.

> **Interception** — `INTERCEPT_DESIGN.md` (PyO3 + mitmproxy live) vs `bifrost_proxy` header, vs
> `README.md:96-98` (Bifrost proxy "operational and handling live traffic") vs external
> `network-proxy/DESIGN.md:192` (SSE code "never carried live traffic") and `:106` (validation
> failure a hard error).

> **cc_launch** — `CC_LAUNCH_DESIGN.md:46-56` (persona 7th) vs `ABOUT:48-49` and code (persona
> 2nd); `cc_launch_profiles.toml:3` vs DESIGN:107-111 on permissions; `library/systems/cc-launch.xml`
> vs code on `.SYSTEM_PROMPT` extension and control-panel writes.

> **Announce locks** — `ANNOUNCE_NOTES.md:30-32` vs code, which splits them across announce, the stop
> hook and relay.

> **Hand-maintained duplicates (derivation candidates, not reconciled):** the workspace-dependency
> list (`NORNIR_CONVENTIONS.md:323`) restates `Cargo.toml`; `HOOK_DESIGN.md`'s member list and
> settings example restate `Cargo.toml` and `~/.claude/settings.json`; `README.md` counts restate the
> tree; the deploy command block appears in three files. Each has already drifted. The error-type
> table is the one generated copy and it matches.

## Priority 4 — Invisible crates (not in `NORNIR_CONVENTIONS.md` § Dependency Lookup)

Core: `announce_core`, `compaction_inject_core`, `datagram_core`, `default_apply_core`,
`dep_resolve_core`, `intercept_core`, `saga_core`, `stamp_core` (prose only, `:180`), `text_core`,
`time_core`.
Capability: `default_apply_io`, `intercept_io`, `keystroke_io`, `session_io`, `store_engine` (prose
only, `:190`), `workspace_registry`.

Listed entries whose description disagrees with the code: `format_core` ("JSON/YAML/TOML/TOON") —
also XML, Markdown, TOMLX (1.6). All other listed crates exist and their one-line descriptions match.
