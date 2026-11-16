# Strict Structural Audit — Auditor B

**Scope:** every crate in the nornir workspace (143 crates: 21 core, 21 capability, 101 tier 3), checked against `audit/STRUCTURAL_AUDIT_GUIDE.md` invariants P1–P10. Reference standard is `NORNIR_CONVENTIONS.md`. Nothing else in `audit/` was read.

**Method:** I read the code: every `Cargo.toml` (path deps resolved and tier-classified by script), every `src/main.rs` and `src/lib.rs` of the non-generated binaries, and the security hooks in full. I checked the generated crates (gates, readers, text writers, writers, senders) by sampling, then grepping for deviations. I probed the hook regexes against benign commands and walked the tree for orphans. File:line references are given so each finding can be checked in one lookup.

**Status of findings:** these are flags, not verdicts. Where docs and code disagree, both sides are named and neither is judged.

**Hook notices raised during the audit:** reading `gleipnir_core/src/checks_rs/prohibited.rs` and the `hooks/hook_pre_*` sources tripped the probing detector. Those reads were required by P6 (security hook coverage) and P9 (gleipnir check accuracy). One `hook_pre_llm_bash` truncation notice also fired on a `grep` of a hook source file.

---

## Summary

| Priority | Findings | Severity cluster |
|---|---|---|
| P1 Pure logic in binaries | 11 | cc_launch, bifrost_proxy, security hooks, announce |
| P2 Tier model | 3 | Cargo tiers clean; one misplaced crate |
| P3 exit / panic discipline | 6 | cc_launch exits in `run()`; untyped errors in most hand-written binaries |
| P4 Composition | 16 | intercept pipeline ×3, ai-root ×4, workspace identity ×4, lock/announce ×6 |
| P5 Naming | 10 | hook/daemon/interceptor prefixes, gate stage≠schema |
| P6 Security hook coverage | 17 | **subagent sandbox escapes; floor bypass via Glob; fail-open input** |
| P7 Schema-first | 11 | cross-binary contracts with no schema file |
| P8 Stale / vacuous tests | 7 | tests that pass by construction, or that encode gaps as the spec |
| P9 Gleipnir accuracy | 8 | Rust checks bypass the matrix; panic macros undetected; tier map incomplete |
| P10 Orphans / doc drift | 13 | dead alias, undeployed crate, stale doc references |

**Total: 102 findings.**

### Clusters: where the arrows point

> **The interception subsystem** (bifrost_proxy, traffic_interceptor_rewriter, intercept_replay, intercept_io) accounts for findings in P1, P2, P4, P5, P7 and P10. The three crates carry one pipeline three times, with three copies of one schema and two different failure directions. This is one missing primitive, not many local problems.

> **The security hooks** (P6) cluster around one root: hook input is matched as raw strings and never parsed into a typed, normalized shape. Whatever `HookInput::target_path()` does not extract, and whatever `has_chain_chars()` does not list, is allowed. Most of the P6 findings are symptoms of that missing boundary.

> **Session identity** (workspace name, ai root, prompt-file names, lock checks) is derived independently in a dozen places. P4 and P5 findings scatter across nearly every hook because no single crate owns "who/where is this session".

> **cc_launch** is a 2287-line binary with no core crate. It holds the prompt-assembly rules that three hooks depend on by file-name convention.

---

## P1 — Pure logic in binary crates

**P1.1 — cc_launch carries the whole prompt-assembly model with no core crate.**
`cli/cc_launch` is 2287 LOC across 7 modules. Pure logic includes:
- the layering and order rules: `assembly.rs:83` `assemble_prompt`, with foundation, then persona, then always-loaded, then space, then descriptors, then coding, then languages, then expertise
- `assembly.rs:8` `estimate_tokens`
- `model.rs:317` `ordered_descriptor_indices` (primary first, overview last)
- `model.rs:257` `sync_languages_from_descriptors` (languages derived from descriptors)
- `model.rs:183` `matching_space_index`
- `library.rs:299` `parse_xml_root_attrs`
- `library.rs:329` `id_to_display`
- `permissions.rs:24-58`, the persona-to-grant table

The assembled-prompt format is consumed by `hook_session_start_orient` and `hook_session_start_inject` through file-name convention only. Anything else that wants to assemble or validate a prompt would have to reimplement these rules.

**P1.2 — bifrost_proxy holds a pure SSE state machine and request parsing (1016 LOC, zero tests).**
`daemons/bifrost_proxy/src/main.rs` contains:
- request parsing: `:110` `extract_session_id`, `:139` `extract_workspace_path`, `:160` `workspace_path_from_text`
- SSE handling: `:646` `route_sse_event`, `:666` `parse_sse_event`, `:682/:720/:754` `handle_block_start/delta/stop`, which strip thinking blocks and renumber indices
- `:789` `reconstruct_sse_event`
- `:819` `find_event_boundary`

All of this is deterministic transformation that `intercept_core` exists to hold. `intercept_core` holds only `classify_exchange`, `has_tool` and `tool_count`.

**P1.3 — The rule engine of the security hooks lives in each hook binary.**
- `hooks/hook_pre_llm_bash/src/main.rs:28-77` (`CompiledRule`, `parse_rules`), `:239` `check_category`
- `hooks/hook_pre_llm_tool/src/main.rs:36` `parse_rules`, `:113` `decide_inner`
- Each hook re-implements three things: matching rules in a category, applying the per-rule severity override, and building a `make_decision`.
- Both hand-roll the `--category severity` argument loop (`bash:93-155`, `tool:55-84`).
- `hook_io::rules` provides only raw TOML-to-`RawRule` parsing. The matching engine that is the security policy is duplicated, and is already divergent: bash compiles regexes, while tool does substring `contains`.

**P1.4 — Shell-grammar logic in hook_pre_subagent_bash.**
`:73` `has_chain_chars`, `:93` `parse_heredoc_header`, `:294` `strip_heredoc_body`, and the inspect-path extraction at `:178` are pure shell-syntax analysis inside a binary. `hook_pre_llm_bash` has its own, different chaining detection (regex `\s&&\s`). There are two shell analysers with different rules (see P6).

**P1.5 — announce: caching policy and queue encoding inline in the binary.**
- `cli/announce/src/main.rs:212-234` is the ElevenLabs hit-count state machine: 0 streams, 1 downloads, ≥2 streams.
- `:244-284` is the queue-entry encoding (argv reconstruction) that `relay_mic_to_voice` decodes.
- Both are pure, and the second is a cross-binary contract. `announce_core` exists but holds neither.

**P1.6 — `detect_project_root` exists three times, byte-identical.**
`hooks/hook_post_llm_read/src/main.rs:75`, `cli/dep_resolve_file/src/main.rs:127`, `cli/dep_resolve_function/src/main.rs:132`. It belongs in `dep_resolve_core`, or is I/O for a capability crate.

**P1.7 — dep_resolve_file and dep_resolve_function are near-identical binaries.**
`diff` shows them differing only in doc text, one positional arg and the resolve call. `run`, `outcome_for`, `exit_code_for`, `Outcome` and `detect_project_root` are duplicated.

**P1.8 — Source-kind classification by extension exists in four places.**
- `cli/syn_cli/src/main.rs:104,230` (`Kind`, `matches_kind`)
- `cli/saga_cli/src/main.rs:25,84-101` (`Kind`, `find_source_files`, `kind_label`)
- `hooks/hook_post_llm_tool/src/main.rs:36-50` (`FileKind`, `classify_file`)

`gleipnir_core::classify` is described as classifying files "exactly once".

**P1.9 — Session-start helpers are duplicated.**
- `hook_session_start_inject:51` and `hook_session_start_orient:71` each derive the workspace name inline (also `hook_session_end_handoff:50`, `hook_compact_session_log:131`, `hook_notify_llm_tts:65`).
- `hook_session_start_inject:62` `integrity_banner` and `:108-118`, the read/fallback policy for the clear-handoff race, are pure decision logic in a binary.

**P1.10 — split_jsonl_batches `compute_batches` (`:70`) is still in the binary.**
`plans/IMPROVEMENT_PLAN.md` S2 defers this item. I confirmed it is unchanged.

**P1.11 — The watcher's transcript functions are still in the binary.**
`watchers/watch_and_diff_exchange_intercepts/src/main.rs:330-358`. `plans/IMPROVEMENT_PLAN.md` S1 lists this as a finding to move into `session_io`; it is still present (see also P4.9).

---

## P2 — Three-tier dependency model

Cargo tiers are clean: no core crate depends on a non-core crate, no capability crate depends on a binary, and no binary depends on another binary. Every `core/` crate has `_core` and no crate outside `core/` does. Core crates contain no runtime I/O; `fs`/`env` use is confined to `build.rs`, `examples/` and `#[cfg(test)]`.

**P2.1 — `capability/intercept_io` is a Python extension module in the capability tier.**
- It is a PyO3 `cdylib` with `#[pymodule]` (`src/lib.rs:50`).
- `NORNIR_CONVENTIONS.md` § Architecture puts Python extension modules in tier 3.
- No crate depends on it, and it is not in `deploy_categories.toml`. It is absent from `~/ai/tools/lib`, so it is never built or deployed by `nornir_deploy`.
- Its `append_jsonl_line` appends unvalidated JSON for Python callers, bypassing the "a writer writes nothing without a schema" rule.

**P2.2 — The jsonschema workspace feature `resolve-file` is enabled for core crates.**
`Cargo.toml` `[workspace.dependencies] jsonschema … features = ["resolve-file"]`, consumed by `core/schema_core` and `core/error_core`. That feature lets a `$ref` read the filesystem during validation, which is a latent I/O path inside tier 1. Low severity; flagged for the owner's judgment.

**P2.3 — `traffic_interceptor_rewriter` is a PyO3 crate whose PyO3 code is not behind the `python` feature.**
`interceptors/traffic_interceptor_rewriter/Cargo.toml` has a hard `pyo3` dep. So do `binary_readers/read_png_verify_heights`, `binary_writers/write_bytes_verify_png` and `binary_writers/write_bytes_verify_quantized_mesh`. None of them can link a test binary, and they are excluded from `cargo test --workspace` (Conventions § Testing). This is the "one tool, two faces" structure not yet applied, not a tier violation.

---

## P3 — process::exit and panic discipline

**P3.1 — `process::exit` inside `run()`.**
`cli/cc_launch/src/main.rs:106` and `:122` call `process::exit(EXIT_NO_LAUNCH)` from `run`. These are the only two `process::exit` sites in the workspace outside `main()`.

**P3.2 — Most hand-written binaries return `Result<_, String>`, not a typed engine error with `error_type()`.**
The Binary Structure contract requires `run() -> Result<String, EngineError>` and prints `error_type()` on the first line of stderr. Untyped binaries:
- `cli/announce`, `cli/cc_launch`, `cli/syn_cli` (`run -> Result<i32, String>`), `cli/saga_cli`
- `converters/*`, `dispatchers/split_jsonl_batches`, `rewriters/rewrite_compaction_summary`
- `watchers/*`, `daemons/*`, `senders/send_datagram`, `interceptors/intercept_replay`

Only the generated tools (gates, readers, text writers, presence, writers via `write_engine::run_cli`) follow the contract.

**P3.3 — Wrong exit code for usage errors.**
- `senders/send_alert|send_heartbeat|send_notification|send_warning/src/main.rs:7` exit 1 on wrong arguments. Conventions rule 2 says 2.
- `converters/convert_json_to_*` exit 1 for every failure, including usage.

**P3.4 — Panics in gleipnir_core production code that are not initializers.**
`core/gleipnir_core/src/checks_py/architecture.rs:1132` and `:1144` call `unreachable!()` inside `check_unknown_file_in_zone`. These sites are invisible to `no_unwrap` (see P9.2).

**P3.5 — Bare-value helpers that swallow errors on the critical prompt chain.**
- `cli/cc_launch/src/assembly.rs:78`: `let _ = fs::write(&workspace_copy, &prompt);`. If the `.SYSTEM_PROMPT.xml` copy fails, `hook_session_start_orient` has nothing to move, and the session is never re-injected after compaction. Nothing reports it.
- `announce/src/main.rs:338` `save_hits` and `:311` `load_config` turn a parse error into defaults (the config parse error is only `eprintln!`'d).

**P3.6 — `run_pre_hook` swallows unreadable or unparseable stdin as Allow.**
`capability/hook_io/src/lib.rs:169-182`: no `Result` reaches any caller and nothing is emitted. Hook "silent exit" is documented, but for blocking hooks this converts an error into a permission (see P6.11).

---

## P4 — Composition over reimplementation

**P4.1 — The intercept pipeline is implemented three times, with divergent failure behaviour.**
The pipeline is: raw capture, then validate against the CC wire schema, then `classify_exchange`, then route to `session_io`, then compaction inject.
- `daemons/bifrost_proxy/src/main.rs:227-289` fails **open** on schema mismatch (`Ok(None)`, `:248-254`) and appends raw with `write_engine::append_line_fsync`.
- `interceptors/traffic_interceptor_rewriter/src/lib.rs:73-128` fails **closed** (`Err`, `:94-99`) and appends raw with its own `append_raw` (`:35-51`), flush only, no fsync.
- `interceptors/intercept_replay/src/main.rs:85-115` silently counts schema-invalid lines as "skipped" (`:91-97`).

`interceptors/INTERCEPT_DESIGN.md:38-39` requires the live and replay paths to "produce identical derived files from the same input". The routing match should be one function in `session_io` or `intercept_core`.

**P4.2 — The CC wire schema has three copies outside `schemas/`, embedded around `schemas_embedded`.**
- `daemons/bifrost_proxy/cc_wire_schema.json` is a regular file.
- `interceptors/traffic_interceptor_rewriter/cc_wire_schema.json` is a regular file.
- `interceptors/intercept_replay/cc_wire_schema.json` is a symlink to the rewriter's copy.

All three are byte-identical today (md5 66e1ee2c…). Each crate builds its own `static WIRE_SCHEMA: EmbeddedValidator` (`bifrost:83`, `rewriter:25`, `replay:21`) instead of a `schemas_embedded` constant. The bifrost copy can drift without anything noticing.

**P4.3 — The ai root is resolved four ways.**
- `write_engine::ai_home` (`capability/write_engine/src/lib.rs:147`) falls back to `/tmp`.
- `datagram_io::ai_base_dir` (`capability/datagram_io/src/lib.rs:291`) falls back to `""`, which gives `/ai/`.
- `saga_runner::strip_ai_root` (`capability/saga_runner/src/lib.rs:48`) accepts both `~/ai/` and `~/.ai/`.
- `hook_io::log_path` (`capability/hook_io/src/lib.rs:305`) uses `$HOME/.claude/intercept.log`, written with raw `writeln!`.
- `store_engine::ai_home` (`:286`) is a pass-through re-export, so a second public name for one function.
- `cli/cc_launch/src/permissions.rs:27-29` hard-codes `/Users/johnny/ai/...` and `/Users/johnny/.claude/`.

**P4.4 — Workspace identity has four incompatible schemes.**
- **Last path component** (`nornir`): `datagram_io::workspace_name()` (`:241`), used by `senders/*`, `hook_io` datagrams and `hook_post_llm_tool:144`. Also `hook_session_start_orient:71`, `hook_session_start_inject:51`, `hook_session_end_handoff:50`, `hook_compact_session_log:131` and `hook_notify_llm_tts:65`, each inline.
- **Registry longest-prefix**: `workspace_registry::resolve_workspace_from_path`, used by `hook_stop_llm_tts:70`, `hook_notify_llm_tts:82`, `hook_io:411`, `announce:101`, `cc_launch:215` and `bifrost_proxy:190`.
- **`@smidja:nornir`**: `datagram_io::workspace_from_path` (`:258`), used only by `syn_cli:418`.
- **`"unknown"`/`"workspace"`/`""` fallbacks** differ per site.

Consequences:
- Hlidskjalf receives datagrams whose `workspace` field uses two formats.
- Two projects with the same basename collide in the registry and in `control/workspaces/{name}/`. Orient registers by basename, so the session's prompt directory is keyed by basename.

`NORNIR_CONVENTIONS.md` § Dependency Lookup names `workspace_from_path` as *the* identity for directory tools.

**P4.5 — Lock checks and the announce spawn are duplicated six times.**
The SILENT/QUIET check (global, then workspace) plus spawning `~/ai/tools/bin/announce` appears in:
- `capability/hook_io/src/lib.rs:404-436`
- `hooks/hook_session_start_inject/src/main.rs:31-49`
- `hooks/hook_notify_llm_tts/src/main.rs:78-95`
- `hooks/hook_compact_session_log/src/main.rs:86-105`
- `hooks/hook_stop_llm_tts/src/main.rs:73-82,116`
- `cli/announce/src/main.rs:104-112`

`daemons/relay_mic_to_voice/src/main.rs:180,232` instead resolves `hush` and `announce` through `$PATH`, so two binary-resolution rules coexist.

**P4.6 — The prompt-file name contract is spread across five crates as string literals.**
- `.SYSTEM_PROMPT.xml`: `cc_launch/src/assembly.rs:77`, `hook_session_start_orient:95`
- `SYSTEM_PROMPT.xml`: `cc_launch/src/assembly.rs:192`, `hook_session_start_orient:93`, `hook_session_start_inject:98`, `hook_session_end_handoff:56`

`workspace_registry` already owns the sibling path (`clear_handoff_path`, `:167`) but not this one.

**P4.7 — `hook_session_start_inject` builds its hook reply by hand.**
`:134-140` hand-builds `hookSpecificOutput` JSON with `serde_json::json!`. `hook_io::response::SessionStartResponse` (`response.rs:616`) exists for exactly this. It is also the only lifecycle hook emitting output, and it bypasses the compile-time event typing the CLAUDE.md note describes.

**P4.8 — cc_launch does its own file I/O.**
- Directory walking with `fs::read_dir` (`library.rs:120,158,194,259`) instead of `directory_read_io`, "the one directory walker".
- XML attribute parsing by regex on the first line (`library.rs:299-325`) instead of `format_core` XML.
- Non-atomic `fs::write` (`assembly.rs:73,193,267`) instead of `write_engine::write_file_atomic`. cc_launch already depends on `write_engine`, but only for `ai_home`.

**P4.9 — Hand-rolled file writes elsewhere.**
- `dispatchers/split_jsonl_batches/src/main.rs:137-150` `write_batch` writes with fsync but is not atomic.
- `watchers/watch_and_diff_exchange_intercepts/src/main.rs:338-358`: `open_transcript` and `append_transcript` append with no fsync and swallow write errors.
- `cli/announce/src/main.rs:338-342` `save_hits` uses non-atomic `fs::write`.

**P4.10 — Session IDs from hook stdin are joined into paths without `path_core::validate_path_segment`.**
`hook_compact_session_log:60-62`, `hook_session_start_orient:92`, `hook_session_start_inject:97`, `bifrost_proxy:233` (session id from the request's `metadata.user_id`). `split_jsonl_batches` and `hook_pre_subagent_bash` do use `validate_path_segment`.

**P4.11 — `target_path()` is implemented twice.**
`HookInput::target_path` (`hook_io/src/lib.rs:32`) and `PostHookInput::target_path` (`:570`) are identical. Both have the coverage gaps in P6.

**P4.12 — hook_post_llm_tool shells out to saga and syn, and parses syn's human text.**
`hooks/hook_post_llm_tool/src/main.rs:79-120` spawns `~/ai/tools/bin/saga` and `syn`, then string-compares stdout to `"All checks passed."` (`:114`). That string is produced by `core/report_render_core/src/lib.rs:354`. The capability crates `saga_runner` and `syn_core` provide the same result as typed data.

**P4.13 — The announce queue contract is encoded on one side and decoded on the other with no shared type.**
`announce/src/main.rs:244-284` writes `{"args": [...], "text": "..."}`. `relay_mic_to_voice/src/main.rs:202-262` reads it with `entry["args"]` indexing. Also, `drain_queue` reads the file and then removes it (`:204-209`), so a line appended between the read and the remove is lost.

**P4.14 — `hush` matches announce's internal argv by regex.**
`cli/hush/src/main.rs:51-54` builds `pkill -f "announce.*--play.*--source {dir}"`. It couples to announce's hidden `_play_*` clap flags, and interpolates `{dir}` unescaped into a regex.

**P4.15 — Converters write a file without a schema.**
`converters/convert_json_to_toml/src/main.rs:34-35` takes an optional output path and writes via `write_engine::write_file_atomic`. The `convert_` contract is "writes another on stdout". File writing with validation is `text_writers/write_json_validate_toml`'s job.

**P4.16 — The hook_pre_llm_bash allow-list env var is parsed by its own `split(':')` loop.**
`:138-143`. `hook_pre_llm_tool:86` has a separate `split_env_list` that does the same thing.

---

## P5 — Naming encodes architecture

**P5.1 — `hook_post_llm_read` is a PreToolUse hook.**
Its own doc comment (`src/main.rs:1`, "PreToolUse hook") and `~/.claude/settings.json` (registered under `PreToolUse`, matcher `Read`) agree on that. The name says `post`.

**P5.2 — `hush` is a Claude Code hook living in `cli/` without `hook_`.**
It is registered under `UserPromptSubmit` and reads hook JSON on stdin (`cli/hush/src/main.rs:1-10`).

**P5.3 — `interceptors/traffic_interceptor_rewriter` lacks the `intercept_` prefix.**
The table maps `interceptors/` to `intercept_`. The name also reads as a `rewrite_` tool.

**P5.4 — `daemons/bifrost_proxy` and `daemons/relay_mic_to_voice` lack the `record_` prefix.**
The table defines only `record_` for `daemons/`. Neither crate records anything as its purpose.

**P5.5 — `cli/` has no row in the verb-prefix table.**
`announce`, `cc_launch`, `hush`, `dep_resolve_file` and `dep_resolve_function` carry no category prefix. Conventions state `saga` and `syn` are "the only exceptions".

**P5.6 — Gate stage names do not match their schema.**
`gate_paths_verified` and `gate_paths_verified_input` embed `PATHS_RESOLVED` (`gates/gate_recipes.toml:156-162`). Conventions § Crate Names: "stage matches schema name". `gate_paths_resolved_output` also exists, so the same schema carries two stage names.

**P5.7 — Gate direction slot outside the documented set.**
`gate_paths_verified` (kind `passthrough`) and `gate_raw_definition_defaults` (kind `defaults`) break the rule in Conventions § Crate Names: "direction is `input` or `output`". The nornir-gates descriptor in the session prompt says "omitted for passthrough". The two sources disagree, and neither covers `defaults`.

**P5.8 — `capability/intercept_io` carries a binary verb prefix.**
`intercept_` signals a tier-3 interceptor. See also P2.1.

**P5.9 — clap field names that mislead.**
- `cli/announce` `--quiet` means dry-run (`main.rs:132-138`), while `QUIET.lock` in the same system means "suppress response playback".
- Hidden fields `_play_pcm`/`_play_file` (`:70-77`) use the underscore-means-unused convention for live flags.
- `syn_cli --silent` means "suppress Hlidskjalf broadcast", while `SILENT.lock` means mute TTS.

**P5.10 — A stale binary name in a usage comment.**
`converters/convert_json_to_toml/src/main.rs:4-5` documents the binary as `json_to_toml`. The YAML twin likely matches; I did not check it.

---

## P6 — Security hook coverage

### hook_pre_subagent_bash (sandbox boundary)

**P6.1 — The heredoc check reads only the first and last lines, so commands after an early delimiter run.**
`validate_writer` (`:103-114`) requires only that line 0 is the header and the *last* line equals the delimiter. `strip_heredoc_body` (`:294-298`) checks chaining on line 0 only. For example:

```
cat <<'R' | allowed_writer
R
<any command>
R
```

bash ends the heredoc at the first `R` and executes `<any command>`, and the hook allows it. No test covers an early delimiter.

**P6.2 — Inspect commands can pipe, background or start a new line.**
`has_chain_chars` (`:73-81`) checks only `;`, `` ` ``, `&&`, `||` and `$(`. It misses:
- single `|`, so `ls /allowed/ | sh` passes
- `&`
- newline, which is a bash command separator
- `>`/`<` redirection
- `${…}`

**P6.3 — Tests encode the gaps as the specification.**
`has_chain_chars_single_pipe_allowed` (`:342`) and `has_chain_chars_single_ampersand_not_detected` (`:358`) assert that `|` and `&` are *not* detected. `warn_chaining`'s message (`:220`) tells the user that `;  &&  ||  \`` are what is checked.

**P6.4 — Path confinement covers absolute paths only.**
`PATH_RE` (`:29`) extracts `(/\S+)`, so relative paths and bare `..` escape the prefix check. `validate_inspect_no_paths_at_all_allows` (`:675`) tests `ls` with no args as Allow, but nothing tests `ls ..` or `find . -delete`. There is also no test of an allowed inspect command whose flags write or execute (`find /allowed -delete`, `find /allowed -exec … +`).

### hook_pre_subagent_tool

**P6.5 — Missing tool name or path is Allow.**
`decide_with_map` returns Allow when `tool_name` is absent (`:53-55`) and when there is no target path (`:69-72`, "Grep/Glob with no path = cwd"). A sandboxed subagent can Grep or Glob its whole cwd regardless of its grants.

**P6.6 — Path fields the hook never sees.**
`HookInput::target_path` reads only `file_path` and `path` (`hook_io/src/lib.rs:32-37`). Glob's `pattern` (which may be absolute), Grep's `glob`, and `notebook_path` are never inspected.

### hook_pre_llm_tool

**P6.7 — The floor layer is bypassed by Glob.**
The settings matcher is `Read|Grep|Glob|Write|Edit`. A Glob carrying `pattern: "/Users/johnny/.ssh/*"` and no `path` gives `target_path() == None`, which returns Allow at `:114-117` before the floor loop. Glob listing of every floor directory is therefore unprotected.

**P6.8 — The `gaming` category has zero rules, and its tests pass vacuously.**
`rules.toml` has only `[[floor]]` (6) and `[[probing]]` (12). The doc comment (`:3`), the usage line, the settings flag `--gaming warn` and the rules header (`rules.toml:13`) all describe gaming detection. `decide_gaming_block_denies` (`:530-536`) returns early "if rules.gaming.is_empty()", as does its warn twin.

**P6.9 — The allow-path prefix is not normalized.**
`is_allowed_path` (`:180`) is `starts_with` with no `..` handling, unlike the subagent hooks. A path such as `/Users/johnny/ai/tools/../control/...` inherits the Tyr exemption from probing and gaming for any path. Floor still applies.

**P6.10 — The probing list omits some of the security system's own sources.**
`hooks/hook_pre_subagent_*` and `cli/cc_launch/src/permissions.rs`, the persona-to-grant table, are not listed. `/hook_pre_llm` covers only the LLM hooks. This is an observation; intent is the owner's call.

### hook_io (all four blocking hooks)

**P6.11 — Unreadable or unparseable input fails open.**
`run_pre_hook` prints Allow on a stdin read error or JSON parse error (`lib.rs:169-182`). For the subagent sandbox hooks this means any payload shape change by Claude Code silently disables the sandbox. Bifrost's own design (session descriptor) states schema mismatch must fail *closed* in a tap that acts.

### hook_pre_llm_bash

**P6.12 — Invalid rule regexes are dropped silently.**
`CompiledRule::from_raw` returns `None` (`:37`) and `parse_rules` uses `filter_map` (`:64`). A typo in `rules.toml` deletes that security rule without any signal. The test meant to guard this is tautological (P8.2).

**P6.13 — One exempted rule exempts the whole category, and the key is regex text.**
`check_category` returns `None` on the first matching rule that is allow-listed (`:248-250`), so the remaining rules in that category are never evaluated. The allow-list (`HOOK_LLM_ALLOW_BASH`, documented at `:11` as "pattern_name") is matched as a substring of the regex *source*, not a rule name. `exempted_pattern_returns_none` (`:1095`) enshrines that.

**P6.14 — A mistyped severity silently disables its category.**
`parse_config` maps an unknown severity string to `None` through `parse_severity`, which turns the category off (`:106-134`). Unknown flags are ignored. A settings typo such as `--subversion blok` removes subversion detection with no signal.

**P6.15 — The patterns are overbroad.**
I ran each pattern against an ordinary command with Python `re`, which agrees with Rust `regex` for these patterns:

| Pattern | Benign command | Result |
|---|---|---|
| `rm.*\.lock` (subversion, **block**) | `git log --format=%h -- Cargo.lock` | matched |
| `tail.*CLAUDE\.md` | `ls docs/detail/CLAUDE.md` | matched |
| `cp.*CLAUDE\.md` | `wc -l ~/ai/cpp/CLAUDE.md` | matched |
| `mv.*CLAUDE\.md` | `cat ~/ai/mvp/CLAUDE.md` | matched |
| `env.*HOOK_LLM` | `grep -rn env::var hooks \| grep HOOK_LLM` | matched |
| `git.*config.*hooks` | `git log -- config/ hooks/` | matched |
| `\s&&\s` (chaining, **block**) | `&&` inside a heredoc body or a quoted string | matched |

The live hook also blocked one of my own audit commands as "subversion: Deleting lock files". That command was a `python3 -c` probe whose text contained the regex source `rm.*\\.lock`; nothing was being removed.

**P6.16 — The benign-side tests miss every near-miss.**
For subversion, truncation and evasion (19 rules), the only negative tests use commands that share no tokens with any pattern: `cargo build`, `cat src/main.rs`, `ls`, `git status`, `git diff` (`:927-960`). No rule has a near-miss benign test. Destruction, revert, workflow and chaining do have near-miss negatives. Two rules have no positive test at all: `xattr.*-d.*rules\.toml` (subversion) and `head.*guardrails` (truncation).

**P6.17 — False negatives and coverage gaps.**
- `cat CLAUDE.md | head -20` is not caught; the truncation patterns require the tool word *before* the filename.
- `rg` is uncovered.
- There is no floor layer for Bash at all, so `cat ~/.ssh/id_rsa` matches no category. Floor protection exists only for file tools.
- `decide()` reads process args and environment (`:160`) rather than taking a `Config`, so `decide` itself is untested. Conventions § Hooks show `decide` as pure.

---

## P7 — Schema-first data validation

**P7.1 — The CC wire schema is outside `schemas/`** and is not a `schemas_embedded` constant (see P4.2).

**P7.2 — Claude Code hook payloads have no schema.**
- `HookInput` and `PostHookInput` extract fields from `serde_json::Value` (`hook_io/src/lib.rs:32-45,570-577`).
- Each lifecycle hook declares its own serde struct with `#[serde(default)]` on every field, so missing fields become `""` silently: `SessionEvent` in orient and inject, `StopEvent`, `CompactEvent`, and others.
- `PostHookInput.tool_result` (`:565`) may not match the payload. From my own knowledge, not verified against a captured payload, Claude Code's PostToolUse field is `tool_response`. Nothing reads `tool_result`.

**P7.3 — The announce queue (`~/ai/voice/queue.jsonl`) is a cross-binary contract with no schema** (see P4.13). Malformed entries are skipped silently unless `--verbose` is set.

**P7.4 — cc_launch inputs are shape-checked procedurally with silent defaults.**
- Launch spec: `assembly.rs:272-310` with `table.get(..).and_then(as_str)`.
- Profiles: `profiles.rs:27-77`, where a malformed profile is dropped by `filter_map` and a missing `coding` defaults to true.
- Library fragment attributes: `library.rs:299-325`, where a missing `id` becomes `"unknown"` and an unrecognized `load` becomes `Manual`.

The fragment library is what the session prompt's own schema-first principle uses as its example of a schema-defined structure.

**P7.5 — announce configuration has no schema, and parse failures become defaults.**
`announce.toml` (`:311`), `VOICE.lock` (`:147-152`, parse error ignored), `hits.toml` (`:331`) and `lookups/{lang}.toml` (`:182-188`, `unwrap_or_default`).

**P7.6 — Security policy files have no schema.**
The hook `rules.toml` files are validated only by `hook_io::rules::parse_rule_array`. Invalid entries are dropped (P6.12) and unknown categories are ignored.

**P7.7 — The `.syn/warn.toml` and `deny.toml` policy files have no schema file.** They are parsed into `SynFilterToml` (`syn_cli:48-53`).

**P7.8 — Records written without schemas.**
- `compact_summary.jsonl` (`hook_compact_session_log:56-66`)
- `thinking_log.jsonl` (`bifrost_proxy:794-816`)
- `running_transcript.jsonl` (watcher)
- `~/.claude/intercept.log` (hook_io)

All are written with `json!` or text and are validated by nothing.

**P7.9 — `hook_post_llm_tool` reads syn's pass/fail from human prose** (`:114`, see P4.12).

**P7.10 — No code in nornir validates `tool_registry.toml` against `schemas/tools/tool-registry.schema.json`.** It may be validated downstream in regin; I did not check.

**P7.11 — A schema-less config TOML is parsed procedurally.** `print_spec_workspace` (`cc_launch/src/main.rs:131-140`) re-parses the launch spec a second time with its own `table.get`, which duplicates `consume_launch_spec`.

---

## P8 — Stale and vacuous tests

**P8.1 — The gaming tests pass by returning early** (`hook_pre_llm_tool:530-536` and its warn twin). See P6.8.

**P8.2 — The rule-compilation test is tautological.**
`parse_rules_subversion_rules_compile` (`hook_pre_llm_bash:279-291`) asserts `is_match("") || !is_match("")`, which is always true. Its comment claims "All rules compiled successfully". It never compares the compiled count to the TOML count, so a dropped rule passes.

**P8.3 — The truncation and evasion compile tests assert only `!is_empty()`** (`:294-310`). Losing six of seven rules passes.

**P8.4 — Tests enshrine implementation gaps as expected behaviour** (`hook_pre_subagent_bash:342,358`), as described in P6.3. These verify the implementation, not the specification. Conventions § Testing: "Tests verify specification, not implementation".

**P8.5 — `exempted_pattern_returns_none` (`hook_pre_llm_bash:1095`) verifies regex-substring exemption semantics, while the doc (`:11`) describes pattern names.** This is doc and test disagreeing. I am not judging which is wrong.

**P8.6 — No tests at all on contract-bearing crates.**
- `capability/workspace_registry` (the identity resolver; longest-prefix match untested)
- the whole session-prompt chain: `hook_session_start_orient`, `hook_session_start_inject`, `hook_session_end_handoff`
- `hook_compact_session_log`
- `daemons/bifrost_proxy` (1016 LOC, SSE rewriting of the stream the user watches)
- `interceptors/intercept_replay`, whose design requires it to match the live path
- `capability/tool_face_io`, the CLI and Python contract adapter (its consumers' `cli_contract.rs` tests cover it indirectly)
- `cli/announce`, `cli/saga_cli`, `cli/hush`, `daemons/relay_mic_to_voice`

**P8.7 — Binary readers and writers have no `tests/cli_contract.rs`, because they have no CLI face.**
`binary_readers/read_png_verify_heights` and `binary_writers/write_bytes_verify_*` have only `lib.rs` and zero tests. See also "Other conventions".

The most recent contract change (writer receipts, commit 97dc2c9) was checked. The `write_engine` tests were rewritten to the receipt contract (`write_engine/src/lib.rs:1023-1063`), and no stale assertion was found.

---

## P9 — Gleipnir check accuracy

**P9.1 — Rust and Svelte checks bypass the check matrix.**
`run_checks_rust` (`core/gleipnir_core/src/lib.rs:132-162`) runs a hard-coded list of checks, and `gleipnir_matrix.toml` covers Python classifications only. The gleipnir descriptor in the session prompt says the matrix "is the single source of truth for which checks run on which file types. A check not in the matrix never runs." That holds for Python only.

**P9.2 — `no_unwrap` misses the explicit panic macros.**
`check_no_unwrap` (`checks_rs/prohibited.rs:143-183`) inspects method calls named `unwrap` or `expect` only. `panic!`, `unreachable!`, `todo!`, `unimplemented!` and `.unwrap_or_else(|| panic!(..))` are not detected; the last is classed as a "safe" method (`:132-136`). Live instances in gleipnir itself are at `checks_py/architecture.rs:1132,1144`. The guide treats panics as a soft `process::exit`.

**P9.3 — The tier map omits nine binary directories.**
`classify_rust_tier` (`prohibited.rs:241-259`) knows `cli/hooks/writers/senders/daemons/interceptors` as binaries. The missing ones are `readers/`, `text_writers/`, `binary_readers/`, `binary_writers/`, `presence/`, `converters/`, `rewriters/`, `watchers/` and `dispatchers/`. They classify as `Unknown`, so the "`run()` owns stdout" exemption for `println!` does not apply there. That is a false-positive source in every newer tool category. The gates tier gets no `run` exemption either. It is also substring-based, so any path containing `/core/`, including outside nornir, classifies as Core.

**P9.4 — Function-level test detection uses `contains("test")` on attributes.**
`has_preceding_attribute` (`prohibited.rs:68-82`) is called with target `"test"` for functions, so `#[cfg(not(test))] fn …` is classed as test code and skipped, a false negative. There are no instances today, so this is latent. It also stops at the first non-attribute sibling, so a comment between `#[test]` and `fn` defeats detection.

**P9.5 — Integration-test directories are not exempt by path.**
Files under `*/tests/*.rs`, such as the 43 `cli_contract.rs` files, are checked as production code. Helper functions there carry no `#[test]` and sit in no `#[cfg(test)]` module, so `.unwrap()` in them is flagged. The guide asks exactly this: "should correctly identify … test helper functions".

**P9.6 — `no_println` checks only `println!` and `dbg!`.**
`FORBIDDEN_MACROS` (`prohibited.rs:190-193`). `print!` in library code is not detected.

**P9.7 — The Python v1 classifier treats any filename containing "test" as a test file.**
`classify_file` (`classify.rs:20-23`) does this, so `latest.py`, `attestation.py` and `contest.py` are classified `Test` and their checks change. This is a false-negative source.

**P9.8 — The guide's check list and known-areas section are out of date with the code.**
- The guide's "Current known areas" says `no_println` should exempt `main()` and output-named functions. The code already exempts `main`, `print_*`/`emit_*`/`display_*`, and `run`/`run_*` in binaries (`:196-316`).
- The guide's check list names `suppression` and `short_names`/`numbered_names`. The registered Rust checks also include `param_count_rs` (`lib.rs:147`), which the guide does not list.

This is doc drift, and the guide is the side that changes.

---

## P10 — Orphaned artifacts and doc drift

The following are clean: workspace members against disk (exact match), `deploy_categories.toml` against members (every tier-3 crate listed), `.qa` sidecars (290 present, 0 orphaned), symlinks (0 broken), schema constants (every `schemas_embedded` static has a consumer), and deployed binaries against crates.

**P10.1 — `capability/intercept_io` is a workspace member that is never deployed and never depended on** (P2.1).

**P10.2 — `hook_io::run_hook` is a "Backward-compatible alias" for `run_pre_hook`.**
`capability/hook_io/src/lib.rs:349-354`. All four security hooks still call the alias, while Conventions § Hooks documents `run_pre_hook`.

**P10.3 — `hooks/HOOK_DESIGN.md:62` lists `hook_compact_session_preserve` (PreCompact, "Handover generation").** No such crate exists.

**P10.4 — `hooks/HOOK_DESIGN.md:274` carries a rename note: "The four `hook_intercept_*` hooks were renamed to `hook_pre_*`".** It keeps the dead names alive in the document.

**P10.5 — `cli/announce/ANNOUNCE_NOTES.md:27` names `hook_start_session_orient`.** The crate is `hook_session_start_orient`.

**P10.6 — `interceptors/INTERCEPT_DESIGN.md` references code that is not in the repo.**
- `:108` references bifrost's `intercept_and_route.py`; no such file is in the repo.
- `:187` references `write_truncate_fsync`; no such function exists.
- `:79` states `raw_session_log.jsonl` is written by "`traffic_interceptor_rewriter` only", but `bifrost_proxy:240-241` also writes it.

**P10.7 — `CONTEXT_MAP.md` and the gates disagree on how a gate embeds its schema.**
CONTEXT_MAP says "which schema a gate embeds → the gate's `src/lib.rs` — one `include_str!` through a symlink into Verdandi's output". In code, gates contain no `include_str!`. They import a constant from `schemas_embedded`, which does the `include_str!` through `schemas/agents/*` symlinks.

**P10.8 — `CLAUDE.md` and `NORNIR_CONVENTIONS.md` give different workspace-identity formats.**
CLAUDE.md "Workspace identity derivation" gives `@smidja/nornir`. `NORNIR_CONVENTIONS.md:256` and the code (`datagram_io:255-269`, tests at `:305`) give `@smidja:nornir`.

**P10.9 — The Conventions § Workspace Dependencies "Current:" list is incomplete.**
It omits deps declared in the root `Cargo.toml`: `quick-xml`, `rusqlite`, `coreaudio-sys`, `enigo`, `ratatui`, `crossterm`, `tokio`, `hyper`, `hyper-util`, `hyper-tls` and `http-body-util`. This is a hand-maintained copy of a list the manifest already declares.

**P10.10 — Descriptors in the session prompt disagree with the code.**
These descriptors live in the library, not nornir.
- The syn descriptor gives `syn --gate <file>`; the code has `--mode gate`.
- The bifrost descriptor describes "dumb plumbing" with all logic in hot-swappable tap processes, but `daemons/bifrost_proxy` performs in-process request interception, compaction rewriting, and inbound SSE thinking-block stripping (`:464-513`, `:570-789`).

**P10.11 — `plans/IMPROVEMENT_PLAN.md` S1 (watcher transcript I/O) is still open in code** (`watch_and_diff…:330-358`).

**P10.12 — `core/gleipnir_core/__pycache__/` is present in the source tree.** It is untracked by git, so this is a cleanliness flag only.

**P10.13 — A stale binary name in a comment** (P5.10).

---

## Other convention deviations observed (outside P1–P10)

**Tool Structure, "a CLI binary always".**
- `binary_readers/*` and `binary_writers/*` have no CLI face, and their PyO3 code is not behind the `python` feature.
- Output gates and `gate_paths_verified` have no CLI face either. The conventions text says only "an input gate has its CLI face too", so the doc is ambiguous about output gates.

**Binary Structure rule 3, "a choice the caller would otherwise make is fixed into the tool or becomes a separate tool".** These tools expose such choices as flags:
- `dep_resolve_*` (`--mode`, `--max-depth`, `--simulate`, `--digest`)
- `announce` (`--profile`, `--voice`, `--speed`, `--lookup`, `--language`, `--translate`, `--no-cache`, `--quiet`)
- `convert_json_to_*` (optional output path)
- the hook binaries (per-category severity flags)

**Arguments "parsed with clap derive where there are named options".** `hook_pre_llm_bash`, `hook_pre_llm_tool`, `hook_pre_subagent_bash` and `hook_pre_subagent_tool` hand-roll their parsing, and unknown flags are ignored silently (P6.14).
