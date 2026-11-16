# Strict Structural Audit A: nornir

**Auditor:** A
**Date:** 2026-11-16 (the date the harness gave). HEAD is `97dc2c9` ("Writers: every write hands back a receipt").
**Reference:** `audit/STRUCTURAL_AUDIT_GUIDE.md` (P1–P10), plus `NORNIR_CONVENTIONS.md`, which the guide names as the "how".
**Scope:** all 147 workspace members in the root `Cargo.toml`, `deploy_categories.toml`, `schemas/`, and the top-level and in-crate docs where they make claims about code.

**Evidence marks.** Every finding carries one of these:
- **[read]**: I read the code at the cited lines.
- **[executed]**: I ran a probe in this session and quote its output.
- **[inferred]**: I reasoned from the code without running it.

Findings are flags for the user to judge. Where a document and the code disagree, the report names both sides and does not say which one is wrong.

---

## 0. Summary

| Priority | Findings | Severity cluster |
|---|---|---|
| P1 Pure logic in binaries | 11 | hooks, bifrost_proxy, dep_resolve CLIs, cc_launch |
| P2 Tier model | 3 | core tier clean; mis-tiered PyO3 crate; pure logic in capability |
| P3 exit / panic discipline | 4 | **a panic in hook_io that can be triggered by input and fails open (executed)**; process::exit in cc_launch::run; typed-error contract met by 0 of 21 hand-written binaries |
| P4 Composition over reimplementation | 14 | lock-check + announce spawn ×6, workspace identity ×5, intercept pipeline ×2–3, ai_home ×3 |
| P5 Naming | 9 | daemons/interceptors/cli prefixes; `hook_post_llm_read` is a PreToolUse hook |
| P6 Security hook coverage | 9 | **6 subagent-sandbox bypasses that come back allow (executed)**; benign-side coverage thin; silent rule loss |
| P7 Schema-first | 8 | rules.toml, fragment library, wire schema ×3 copies, announce config, intercept_io |
| P8 Stale tests / contracts | 4 | tests that write down a gap as intended behaviour; a contract carried by a string literal; untested crates |
| P9 Gleipnir accuracy | 5 | zoneless files classed as structure zone (observed live); "test" substring exemption; set-union read as a type alias (observed live) |
| P10 Orphans | 8 | intercept_io built nowhere; unreferenced schema; stale crate names in docs |

**Systemic clusters, which are where the architecture is weakest:**

> **Hook layer.** Most of the findings sit in `hooks/` + `capability/hook_io`. The hooks share a rule engine, a lock/announce routine, project-dir resolution and stdin parsing, but each binary carries its own copy. The security-critical subagent bash hook passes six classes of escape.

> **Intercept/bifrost layer.** The same capture → validate → classify → route → inject pipeline exists in `bifrost_proxy` (main.rs) and `traffic_interceptor_rewriter` (lib.rs). The two copies differ in failure direction and in fsync, and the wire schema they validate against is stored in three places.

> **The tool contract applies to generated tools only.** Gates, readers, text writers and presence tools follow the "one tool, two faces / typed error_type()" structure. Hand-written binaries don't: the binary_readers, binary_writers, interceptors and every cli/daemon/sender/converter.

---

## P1 — Pure logic in binary crates

**P1-1 Rule-matching engines live inside hook binaries.** [read]
- `hooks/hook_pre_llm_bash/src/main.rs:27-77, 239-264`: `CompiledRule`, `parse_rules`, `check_category`. A regex rule engine: compile TOML rules, match them, map severity to a decision.
- `hooks/hook_pre_llm_tool/src/main.rs:30-43, 113-186`: the same "category → rules → severity → `make_decision`" engine, done with substring matching.
- These are two implementations of one action, and both are pure. `hook_io::rules` holds only the parsing half, so the matching and decision half has been written twice.

**P1-2 Shell-command parsing inside `hook_pre_subagent_bash`.** [read]
- `main.rs:73-205, 294-298`: `has_chain_chars`, `parse_heredoc_header`, `validate_writer`, `validate_inspect`, `strip_heredoc_body`.
- This is pure shell-syntax analysis. `hook_pre_llm_bash` defines "chaining" a second way (`rules.toml:193`, the regex `\s&&\s`), so the workspace has two different definitions of one concept. See P4-9.

**P1-3 `daemons/bifrost_proxy/src/main.rs` is a 1016-line monolith with no tests.** [read]
- Pure functions sit in the binary: `extract_session_id` (:110), `extract_workspace_path` (:139), `workspace_path_from_text` (:160), `parse_sse_event` (:666), `handle_block_start/delta/stop` (:682-788), `reconstruct_sse_event` (:789), `find_event_boundary` (:819), `is_sse_response` (:425).
- `core/intercept_core` exists and is the natural home for them.

**P1-4 `bifrost_proxy::intercept` (:227-289) duplicates `traffic_interceptor_rewriter::run_inner` (lib.rs:73-128).** [read] See P4-3.

**P1-5 `dep_resolve_file` and `dep_resolve_function` are near-identical.** [read]
- `outcome_for`, `exit_code_for` and `detect_project_root` appear verbatim in both: `cli/dep_resolve_file/src/main.rs:103-138` and `cli/dep_resolve_function/src/main.rs:108-143`. `diff` shows only names and doc strings differ.
- A third copy of `detect_project_root` is in `hooks/hook_post_llm_read/src/main.rs:75-86`.
- `dep_resolve_core` exists, and none of the three uses it for this.

**P1-6 `cli/cc_launch` holds prompt assembly and the persona security policy.** [read]
- `assembly.rs` (assemble_prompt, estimate_tokens), `model.rs` (535 lines of state and ordering logic), and `permissions.rs` (the persona → hook-exemption table).
- The exemption table is a security contract whose other half lives in `hook_pre_llm_tool`. See P4-10.

**P1-7 `syn_cli` and `saga_cli` each define their own source-kind → extension mapping.** [read]
- `matches_kind` (`syn_cli/src/main.rs:230`), `find_source_files` and `kind_label` (`saga_cli/src/main.rs:84-101`).
- A third copy is `hook_post_llm_tool::classify_file` (`main.rs:36-50`), and gleipnir_core has its own `classify`.

**P1-8 `dispatchers/split_jsonl_batches`: `compute_batches` (:70) is pure batching arithmetic in a binary.** [read]

**P1-9 `watchers/watch_and_diff_exchange_intercepts` has pure helpers in the binary.** [read]
- A hand-rolled PRNG (`jitter_sleep`, :60), `parse_pace_arg` (:52) and `transcript_path_for` (:330), across 498 lines.

**P1-10 `hook_session_start_inject` has pure helpers in the binary.** [read]
- `integrity_banner` (:62) and a local `workspace_name` (:51). See P4-2.

**P1-11 `hook_post_llm_tool` matches syn's success text by literal.** [read]
- `quality_report` matches the literal `"All checks passed."` (:114). The literal is produced in `core/report_render_core/src/lib.rs:354`. See P8-2.

---

## P2 — Three-tier dependency model

The dependency edges are clean. A script over every member's `[dependencies]` found no core→capability, core→binary, capability→binary or binary→binary edge [executed]. `core/` performs no I/O outside `build.rs`, examples and tests [read via grep].

**P2-1 `capability/intercept_io` is a PyO3 extension module (`#[pymodule]`, unconditional pyo3) placed in the capability tier.** [read]
- Conventions class "Python extension modules" as Tier 3.
- No crate depends on it. No deploy category lists it. NORNIR_CONVENTIONS.md:301 has to `--exclude` it from the workspace test command. See P10-1.

**P2-2 Pure decision logic sits in capability rather than core.** [read]
- `hook_io` holds `make_decision` and `format_event_preamble` (lib.rs:96-159), `category_phrase` and `build_speech` (:313-344), and all of `rules.rs`. All of it is pure.
- No `hook_core` exists. The result is that the pure security-decision vocabulary is coupled to a crate that spawns processes and opens sockets.

**P2-3 Tier-3 crates whose PyO3 code is not behind the `python` feature.** [read]
- `binary_writers/write_bytes_verify_png`, `binary_writers/write_bytes_verify_quantized_mesh`, `binary_readers/read_png_verify_heights` and `interceptors/traffic_interceptor_rewriter` take `pyo3` as a plain dependency.
- So they cannot link a test binary. Three of the four have no tests at all (P8-4).

---

## P3 — process::exit and panic discipline

**P3-1 PANIC that can be triggered by input in the security hook path, failing open.** [read + executed]
- `capability/hook_io/src/lib.rs:99`: `format!("{}...", &input.subject[..max.saturating_sub(3)])` slices by **byte**.
- `hook_pre_llm_bash` passes `max_subject_len: Some(60)` (main.rs:259). Any matched command longer than 60 bytes with a multi-byte character spanning byte 57 panics.
- Probe: a destruction-category command with `é` padding fed to the deployed `~/ai/tools/bin/hook_pre_llm_bash --destruction block`:
  ```
  exit: 101
  stdout:
  stderr: thread 'main' panicked at capability/hook_io/src/lib.rs:99:44:
          end byte index 57 is not a char boundary; it is inside 'é' (bytes 56..58 of string)
  ```
- No decision JSON is written. Per Claude Code's documented hook contract, an exit code other than 0 or 2 is a non-blocking error, so the command proceeds. [inferred from that documented contract; not run end-to-end here]
- Net effect: a block-level rule (destruction, subversion, chaining) can be bypassed by padding with non-ASCII text.
- It also contradicts hook_io's header claim "Always exits 0" (lib.rs:7).

**P3-2 `process::exit` outside `main()`.** [read]
- `cli/cc_launch/src/main.rs:106` and `:122` call `process::exit(EXIT_NO_LAUNCH)` inside `run()`.

**P3-3 The typed-error contract is unmet across every hand-written binary.** [read]
- Conventions § Binary Structure: `run()` returns `Result<_, EngineError>` and `error_type()` names the failure class.
- Of the 21 hand-written binaries under cli/, senders/, dispatchers/, watchers/, converters/, rewriters/, daemons/ and interceptors/intercept_replay, **none** has an `error_type`. All return `Result<_, String>`, or `(i32, String)` in the dep_resolve pair.
- The contract holds only for generated or `tool_face_io` crates.

**P3-4 Senders use the wrong exit code for usage errors.** [read]
- `send_alert`, `send_heartbeat`, `send_notification` and `send_warning` exit **1** on wrong arguments (e.g. `senders/send_alert/src/main.rs:5-7`). Rule 2 says 2.
- They parse `std::env::args()` by hand.

`.unwrap()`/`.expect()`/`panic!` outside tests occur only in `LazyLock`/`static` initializers, which are exempt (gleipnir matrix.rs:156-188, saga_runner:39, checks_*/style.rs). [executed scan + read]

---

## P4 — Composition over reimplementation

**P4-1 The SILENT/QUIET lock check plus the announce spawn are written six times.** [read]
- `capability/hook_io/src/lib.rs:406-438`
- `hooks/hook_compact_session_log/src/main.rs:87-96`
- `hooks/hook_notify_llm_tts/src/main.rs:76-91`
- `hooks/hook_session_start_inject/src/main.rs:32-41`
- `hooks/hook_stop_llm_tts/src/main.rs:73-92` (QUIET)
- `cli/announce/src/main.rs:104-112`
- `daemons/relay_mic_to_voice/src/main.rs:181, 233` spawns `hush` and `announce` through `$PATH`. Every other site uses `ai_home()/tools/bin/announce`.

**P4-2 Workspace identity is derived five ways.** [read]
- `datagram_io::workspace_name` (lib.rs:241): basename of `CLAUDE_PROJECT_DIR`.
- `datagram_io::workspace_from_path` (:258): `@a:b`.
- `hook_session_start_inject::workspace_name` (:51): basename, falling back to `"unknown"`.
- `hook_io::notify_and_log` (:271-275): basename.
- `workspace_registry::resolve_workspace_from_path`: registry longest-prefix.
- Hlidskjalf datagrams get basename identity, while syn's datagrams use the `@` form.

**P4-3 The intercept pipeline is duplicated, and the copies disagree.** [read]
- Copies: `bifrost_proxy::intercept` (main.rs:227-289) and `traffic_interceptor_rewriter::run_inner` (lib.rs:73-128).
- On schema failure, bifrost **fails open** and forwards (`:248-255`), while the rewriter returns `Err` (`:94-99`).
- Bifrost captures raw input through `write_engine::append_line_fsync`. The rewriter uses its own `append_raw` with `flush()` and no fsync (`:35-51`).
- `rewrite_compaction_summary` is a third consumer of `inject_compaction_system_block` and does no wire validation (P7-4).

**P4-4 `ai_home` has three homes with different fallbacks.** [read]
- `write_engine::ai_home` (lib.rs:147) falls back to `/tmp/ai` when `HOME` is unset.
- `store_engine::ai_home` (lib.rs:286) is a re-export.
- `datagram_io::ai_base_dir` (lib.rs:291) falls back to `"/ai/"`.
- Runtime path resolution living in a write engine is also why `workspace_registry` and many hooks depend on `write_engine` without writing anything.

**P4-5 Project-dir resolution and stdin parsing are reimplemented in six hooks that bypass `hook_io`.** [read]
- `hook_stop_llm_tts`, `hook_notify_llm_tts`, `hook_compact_session_log`, `hook_session_start_orient`, `hook_session_start_inject` and `hook_session_end_handoff` each read stdin, `serde_json::from_str` a local event struct, and repeat "CLI --project-dir > stdin cwd > None".

**P4-6 `hook_session_start_inject` builds its reply by hand.** [read]
- It assembles the JSON with `serde_json::json!` (:133-139) although `hook_io::response::SessionStartResponse` exists (response.rs:616).
- CLAUDE.md says those builders exist so that invalid replies do not compile.

**P4-7 Own file writing instead of `write_engine`.** [read]
- `split_jsonl_batches::write_batch` (`File::create`, :137)
- `watch_and_diff…::append_transcript` (`OpenOptions`, :343)
- `rewrite_compaction_summary::write_debug_snapshot` (`fs::write`, :66)
- `bifrost_proxy::save_unparseable_request` (`fs::write`, :523)
- `traffic_interceptor_rewriter::append_raw`
- `hook_io::notify_and_log` (`OpenOptions` to `~/.claude/intercept.log`, :288)
- `announce::save_hits` (non-atomic `fs::write`, :338)
- `relay_mic_to_voice::create_recording_lock` (`File::create`, error discarded, :172). `presence_io` exists for this.

**P4-8 Own directory walking.** [read]
- `cli/cc_launch/src/library.rs` uses `read_dir` directly. Conventions call `directory_read_io` the one walker.

**P4-9 Two definitions of "chaining".** [read]
- `hook_pre_llm_bash` matches the regex `\s&&\s`.
- `hook_pre_subagent_bash::has_chain_chars` matches `; \` && || $(`.
- Neither covers newline, single `&`, `|`, `<(` or `>(`. See P6-1.

**P4-10 The persona → exemption contract is set in two divergent code paths.** [read]
- `cc_launch/src/main.rs:146-153` goes through `permissions::*` directly.
- `:224-233` goes through `assembly::build_allow_paths` plus `permissions::allow_patterns_for_persona`.
- The env-var names `HOOK_LLM_ALLOW_PATHS` and `HOOK_LLM_ALLOW_PATTERNS` are string literals in cc_launch and in `hook_pre_llm_tool`, with no shared constant.

**P4-11 The gate contract dict is hand-built where `tool_face_io::py_contract` exists.** [read]
- `write_bytes_verify_png/src/lib.rs:49-64` and the other binary reader/writer and `traffic_interceptor_rewriter` (`:140-156`, with its own `{"ok","rewritten","error"}` shape).

**P4-12 `intercept_io::json_to_toml` duplicates `convert_json_to_toml`.** [read]
- Both do strip_nulls then `to_toml`.

**P4-13 The numbered-name regex is copied three times with different allowlists.** [read]
- `gleipnir_core/src/checks_py/style.rs` (~:340-364), `checks_rs/style.rs` (~:300-322) and `checks_ts/style.rs` (~:225-246).
- They already differ (`step_\d+` in Rust, `step\d+` in TS, different leading terms).

**P4-14 `hook_io::HookInput::target_path` and `PostHookInput::target_path` are identical copies.** [read] (`lib.rs:32-37`, `:570-575`)

---

## P5 — Naming encodes architecture

**P5-1 `hook_post_llm_read` is a PreToolUse hook.** [read]
- Its doc line 1 says "PreToolUse hook", it calls `run_pre_hook` (:35), and it is configured under PreToolUse. The name says "post".

**P5-2 `daemons/bifrost_proxy` has no verb prefix.** [read] The table gives `daemons/` the `record_` prefix.

**P5-3 `daemons/relay_mic_to_voice` uses `relay_`.** [read] That prefix is not in the table.

**P5-4 `interceptors/traffic_interceptor_rewriter` lacks the `intercept_` prefix.** [read] Its name also carries the word for another category (`rewrite_` → `rewriters/`).

**P5-5 `cli/` crates use no prefix from the table.** [read]
- `announce`, `hush`, `cc_launch`, `dep_resolve_file` and `dep_resolve_function`.
- Conventions say "Specialist tools (`saga`, `syn`) … These are the only exceptions." `cli/` appears in the tier list but not in the prefix table.
- `dep` is an abbreviation (Conventions § What Never Appears).

**P5-6 Gate direction slot.** [read]
- `gate_raw_definition_defaults` uses direction `defaults`. `gate_paths_verified` has no direction.
- Conventions § Crate Names: "direction is `input` or `output`".
- The generated tool function in `gate_raw_definition_defaults` is called `validate` but reads, applies defaults, **writes** and returns (`src/lib.rs:16-18`).

**P5-7 `hook_io::run_hook` is labelled "Backward-compatible alias" (lib.rs:348).** [read]
- Four of the five PreToolUse hooks call the alias. Conventions document `run_pre_hook`.

**P5-8 Converters' doc usage names `json_to_toml`.** [read] (`converters/convert_json_to_toml/src/main.rs:4-5`)

**P5-9 `announce --translate` is a declared flag that only prints "not yet implemented" (`cli/announce/src/main.rs:125-127`).** [read]

---

## P6 — Security hook coverage

**P6-1 The subagent bash sandbox allows six classes of escape.** [executed]
- Setup: crafted PreToolUse payloads fed to the deployed `~/ai/tools/bin/hook_pre_subagent_bash --writer my_writer --inspect ls=/tmp/allowed/ find=/tmp/allowed/`. The only thing read was the decision.

| Case | Command shape | Decision |
|---|---|---|
| early heredoc delimiter | `cat <<'RECORD' \| my_writer` / `{}` / `RECORD` / *second command* / `RECORD` | **allow** |
| echo pipe + newline | `echo '{}' \| my_writer` ⏎ `whoami` | **allow** |
| inspect + newline | `ls /tmp/allowed/` ⏎ `whoami` | **allow** |
| inspect pipe to shell | `ls /tmp/allowed/ \| sh` | **allow** |
| inspect background | `ls /tmp/allowed/ & whoami` | **allow** |
| find -exec, relative | `find . -name x -exec echo hit {} +` | **allow** |

- Causes [read]:
  - `validate_writer` checks only that the *last* line equals the delimiter (`main.rs:109`), so an earlier delimiter line ends the heredoc.
  - `strip_heredoc_body` checks only line 1 for chaining (`:294-298`).
  - `ECHO_PIPE_RE`'s optional name-arg group accepts a newline-separated second word (`:26`).
  - `has_chain_chars` omits `\n`, `&` and `|` (`:73-81`).
  - `PATH_RE` sees only absolute paths (`:29-31`), so relative operands and `-exec`/`-delete` pass.
- Tests record two of these gaps as intended: `has_chain_chars_single_ampersand_not_detected` and `has_chain_chars_single_pipe_allowed`.

**P6-2 Fail-open defaults in the sandbox hooks.** [read]
- `hook_io::run_pre_hook` emits **allow** on unreadable stdin or JSON that fails to parse (lib.rs:169-181).
- `hook_pre_subagent_tool` allows when `tool_name` is absent (:53-56) and when neither `file_path` nor `path` is present (:69-72).
- `target_path` reads only `file_path` and `path` (hook_io lib.rs:32-37). So `NotebookEdit.notebook_path`, a `Glob.pattern` that is an absolute path, and Grep `glob` are never prefix-checked.
- The behaviour is tested (`no_tool_name_allowed`, `no_path_in_input_allowed`), which means it is deliberate. No document explains the fail-open choice for a sandbox.

**P6-3 Invalid rules disappear silently.** [read]
- `CompiledRule::from_raw` drops any regex that fails to compile (`hook_pre_llm_bash/src/main.rs:37-38`, used via `filter_map`).
- `parse_rule_array` drops entries missing `pattern` or `description` (`rules.rs:51-72`).
- `parse_severity` returns `None` for a mistyped value. A mistyped CLI flag (`--subversion blok`) disables the whole category, and a mistyped per-rule severity falls back to the category default.
- No test asserts the rule count. The `parse_rules_*_compile` tests check only "not empty", and line 288 asserts a tautology (`is_match("") || !is_match("")`).

**P6-4 The `HOOK_LLM_ALLOW_BASH` exemption matches by substring of the regex source.** [read]
- `hook_pre_llm_bash/src/main.rs:248` uses `rule.pattern.contains(p)`. A value such as `.` or `\s` exempts most rules.
- rules.toml:20 and the module doc describe the value as "pattern names", but rules have no `name` field.
- Nothing in nornir sets this variable (grep). Doc vs code: flag.

**P6-5 The probing exemption has no path-boundary or `..` handling.** [read]
- `is_allowed_path` is a raw `starts_with` (`hook_pre_llm_tool/src/main.rs:180-182`). The test `is_allowed_path_partial_prefix_no_false_positive` passes only because its fixture ends in `/`.
- A path such as `<allowed>/../.claude/settings.json` is exempted from probing (the floor layer still applies). [inferred]

**P6-6 Probing is not mirrored on the Bash side.** [executed, incidentally]
- During this audit, Read calls on hook source were flagged as probing.
- The same files read through Bash (`grep … rules.toml`, `sed -n … hook_pre_llm_tool/src/main.rs`) were not flagged.
- Probing rules exist only for file tools. The bash rules cover constraint files (CLAUDE.md, guardrails), not the hook sources or `.claude/settings`.
- Probing rules also cover `/hook_pre_llm` and `/hook_post_llm` but not `/hook_pre_subagent` (`hook_pre_llm_tool/rules.toml:70-76`).

**P6-7 Benign-direction coverage is thin for subversion, truncation and evasion.** [read + executed]
- There are 19 rules across those three categories. Benign tests exist for only 5 commands (`ls -la`, `cargo build`, `cat src/main.rs`, `git status`, `git diff`), and none is per-rule.
- Probed false positives, using Python `re` on the rules.toml patterns:

| Pattern | False positives |
|---|---|
| `rm.*\.lock` | `cat format.lock`, `cargo tree --format x Cargo.lock` |
| `cp.*CLAUDE\.md` | `scp host:CLAUDE.md .`, `rg cpu docs/CLAUDE.md` |
| `git.*config.*hooks` | `git diff config/hooks.toml` |
| `grep.*CLAUDE\.md` | `ls \| grep CLAUDE.md` |
| `\s&&\s` | any quoted `&&`, including inside a heredoc body |
| `cargo\s+build` | `grep 'cargo build' docs.md` |

- The `&&` case fired live in this session: two of my Bash calls were **denied** for a quoted `&&` inside a heredoc body and inside a `for` loop's `[ ] && [ ]` test.
- None of these false-positive cases is tested.

**P6-8 There is no test of `decide()` category ordering or end-to-end behaviour for `hook_pre_llm_bash`.** [read] All tests call `check_category` directly. `decide` reads argv and env inside itself (:160), which conventions say must stay pure (§ Hooks: "Pure logic — no I/O"). The same applies to `hook_pre_llm_tool::decide` (:98) and `hook_pre_subagent_bash::decide` (:230).

**P6-9 Exemption, permission and destruction code paths have no test.** [read]
- `hook_pre_subagent_tool` has no traversal test that uses encoded or symlinked forms. That is lower priority.
- cc_launch `permissions.rs` hardcodes `/Users/johnny/...` (:24-31) while every other path site uses `ai_home()`.

---

## P7 — Schema-first data validation

**P7-1 Hook `rules.toml` files have no schema.** [read] Their shape is checked procedurally and failures are dropped silently (P6-3).

**P7-2 The cc_launch fragment library has no schema.** [read]
- `library.rs:298-326` regex-scrapes `id="…"` and `load="…"` from the first line of each XML fragment. A missing id becomes `"unknown"` and an unknown load policy becomes `Manual`, silently.
- The system descriptors have a defined five-section shape, and nothing validates it.

**P7-3 The wire schema is stored three times, outside `schemas/` and outside `schemas_embedded`.** [read + executed]
- `daemons/bifrost_proxy/cc_wire_schema.json` is a regular file.
- `interceptors/traffic_interceptor_rewriter/cc_wire_schema.json` is a second regular file.
- `interceptors/intercept_replay/` symlinks to the rewriter's copy.
- The copies are identical today (sha1 `c7b1a37…`), but two of them are authored separately.

**P7-4 `rewrite_compaction_summary` rewrites an API request with no wire validation.** [read] The other two intercept paths validate.

**P7-5 Hook stdin events are typed structs with no schema.** [read]
- `HookInput` and the six local event structs (P4-5) are deserialized by serde only. Missing or renamed Claude Code fields fall back to defaults silently. This is the scenario bifrost's own design calls "fail CLOSED on schema mismatch".

**P7-6 Announce config falls back to defaults silently.** [read]
- `announce.toml`, `hits.toml` and `VOICE.lock` are parsed with `toml::from_str` and fall back to defaults (`cli/announce/src/main.rs:311-341, 147-151`).
- A malformed `hits.toml` resets every hit count. A malformed `VOICE.lock` is ignored with no message.

**P7-7 `intercept_io::append_jsonl_line` writes without a schema.** [read] It accepts any JSON object and appends it with no schema. Converters given an output path write files with no schema (`convert_json_to_toml/src/main.rs:34-36`). Conventions say a text writer "writes nothing without one".

**P7-8 `tool_registry.toml` has a schema that nothing reads.** [read + grep] `schemas/tools/tool-registry.schema.json` exists, nothing references it in .rs, .py or .toml, and nothing validates the registry. Entries are pasted in by hand from `generate_writer.py` output.

---

## P8 — Stale tests after contract changes

**P8-1 Tests that record gaps as specification.** [read]
- `has_chain_chars_single_ampersand_not_detected`, `has_chain_chars_single_pipe_allowed` and `no_tool_name_allowed` (P6-1, P6-2).
- The tautological regex assertion in `parse_rules_subversion_rules_compile` (`hook_pre_llm_bash/src/main.rs:286-291`) verifies nothing.

**P8-2 A cross-crate contract is carried by a human-readable string.** [read]
- `hook_post_llm_tool` decides "silence" by comparing against `"All checks passed."` (:114). That string is produced by `report_render_core` (:354), and tests elsewhere match it as substrings (:988, :997, :1082).
- A wording change in the renderer would make the hook inject a "clean" report as violations. No test links the two.

**P8-3 The receipt contract change (HEAD) has no CLI-level test on any writer.** [read]
- `write_engine` has unit tests for the receipt (lib.rs:1047-1079). None of the 9 `writers/` crates has a test, and their deploy verification is `--help` only.
- `hook_pre_subagent_bash`'s writer validation and help text (:163-166, :271) describe writer usage that predates the receipt. There is no conflict found, but nothing re-evaluated them.

**P8-4 Crates with no tests at all (50).** [executed scan]
- Capabilities: `tool_face_io` (the adapter every two-faced tool uses), `intercept_io`, `workspace_registry` (SQLite, used by 6+ hooks).
- All 14 output gates, plus `gate_raw_definition_defaults` and `gate_paths_verified`.
- All 3 binary readers/writers and all 9 writers.
- 6 hooks: `stop_llm_tts`, `notify_llm_tts`, `compact_session_log`, `session_start_orient`, `session_start_inject`, `session_end_handoff`.
- CLIs: `dep_resolve_file`, `dep_resolve_function`, `saga_cli`, `hush`, `announce`.
- All 4 simple senders and both converters.
- `core/datagram_core`, `intercept_replay`, `relay_mic_to_voice`, `bifrost_proxy`.

---

## P9 — Gleipnir check accuracy

**P9-1 Files outside any zone are classed as structure zone.** [read + observed live]
- `classify_file_v2` returns `zone: Zone::Structure, level: Outside` when no zone marker matches (`core/gleipnir_core/src/classify.rs:148-153`).
- Every Python file outside a v2 project therefore gets structure-zone checks. My throwaway scratchpad scripts drew `v2_structure_import_boundary` ("structure zone file imports … glob/json/re") and `v2_structure_no_logic`, alongside `v2_import_boundaries` "file outside v2 zone layout".
- The checks contradict each other: the file is classed as outside the layout and inside the structure zone at once.

**P9-2 Any filename containing "test" is classed `Test` and exempted.** [read]
- `classify.rs:20-23` uses `filename.contains("test")`. `latest.py`, `attestation.py` and `contest_rules.py` all escape production checks.

**P9-3 `no_implicit_type_aliases` fires on a set union.** [observed live]
- `names = {…} | {…}` (set union of two set comprehensions) was reported as "implicit type alias names = Union[...]".

**P9-4 The guide's "known areas" list is out of date versus the code.** [read]
- `STRUCTURAL_AUDIT_GUIDE.md` lists "`no_println` — `main()` … and output-named functions should be exempt" as future work.
- `checks_rs/prohibited.rs:277-283` already exempts `main`, `run` and `print_*`/`emit_*`/`display_*`. Doc vs code.

**P9-5 The three numbered-name regexes have already diverged (P4-13).** [read] Rust allows `step_\d+`, TS allows `step\d+`, and Python allows neither. The same name is accepted or rejected depending on language for no stated reason.

---

## P10 — Orphaned artifacts

**P10-1 `capability/intercept_io` is a workspace member that nothing depends on and no deploy category builds.** [executed + read]

**P10-2 `schemas/tools/tool-registry.schema.json` is unreferenced.** [grep]

**P10-3 `examples/` is an empty directory.** [executed] `plans/IMPROVEMENT_PLAN.md` references `append_transcript` and `write_all`. I did not open it further.

**P10-4 Stale crate names in docs.** [read]
- `cli/announce/ANNOUNCE_NOTES.md:27` names `hook_start_session_orient`. The crate is `hook_session_start_orient`.
- `hooks/HOOK_DESIGN.md:62` lists `hook_compact_session_preserve`, which is not a crate. `:274` records a rename ("The four `hook_intercept_*` hooks were renamed…").
- `interceptors/INTERCEPT_DESIGN.md:108` says traffic_interceptor_rewriter is "imported by bifrost's `intercept_and_route.py`". Bifrost is now the Rust `bifrost_proxy`, which reimplements the pipeline (P4-3). Whether anything still imports the PyO3 module could not be determined from inside nornir.

**P10-5 CLAUDE.md and the code disagree on the workspace-identity format.** [read]
- CLAUDE.md § Workspace identity says `@smidja/nornir`.
- `datagram_io::workspace_from_path` produces `@smidja:nornir`, as NORNIR_CONVENTIONS.md:256 says.

**P10-6 CONTEXT_MAP and the code disagree on where gate schemas are embedded.** [read]
- CONTEXT_MAP says "which schema a gate embeds | the gate's `src/lib.rs` — one `include_str!` through a symlink".
- In the code, gate lib.rs references a `schemas_embedded` constant, and the `include_str!` lives in `capability/schemas_embedded/src/lib.rs`.

**P10-7 Adding one writer means editing four hand-kept lists.** [read]
- Workspace `members`, `deploy_categories.toml`, `tool_registry.toml` and `schemas_embedded`. `generate_writer.py` prints the steps rather than applying them.
- These four lists agree today [executed: no member missing from deploy, no deploy entry missing from members]. Nothing derives one from another.

**P10-8 No orphaned `.qa` sidecars or broken symlinks were found.** [executed] 290 sidecars, each with its source present. The repo and `~/ai/tools/bin` have no dangling links.

---

## Observations from running inside the hooks

> **Probing flags on audit reads.** Each Read of `hooks/*/src/main.rs` and `capability/hook_io/src/*.rs` returned a "flagged as probing behavior … the user has been notified" context. I read these files because P6 directs the auditor to the security hooks.

> **Live chaining false positives.** Two Bash calls were **denied** by `hook_pre_llm_bash`'s chaining rule because of a quoted `&&` (P6-7).

> **Probes executed.** Three probes ran against the deployed hooks, as follows. Each was fed only the PreToolUse JSON, and only the decision (or the crash) was read. The subagent probe used allow-expected payloads, so it raised no notifications. The panic probe crashed before any notification code ran.
> - `hook_pre_subagent_bash`: the six P6-1 payloads.
> - `hook_pre_llm_bash`: one P3-1 payload.
> - Python `re`: the P6-7 false-positive table.

## Not covered

- I did not run `cargo test`, so pass/fail state is not reported.
- I did not audit `nornir_deploy` (the script), `generate_gates.py`, `HOW_A_NORNIR_TOOL_IS_MADE.md` or `MUST_READ_BEFORE_BUILDING.md` beyond grep.
- I did not read gleipnir check bodies beyond the cited lines, so P9 covers only what the classifier and observed output show.
