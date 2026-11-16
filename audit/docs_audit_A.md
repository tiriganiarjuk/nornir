# Documentation Audit A — nornir

Auditor A · 2026-11-16 · against `master` at `97dc2c9` (clean tree at start).
Method: `audit/DOCUMENTATION_AUDIT_GUIDE.md`. Every disagreement is reported neutrally: the doc
says X, the code says Y. Which side is right is for a human to decide.

Paths are relative to `/Users/johnny/ai/smidja/nornir/` unless they start with `~` or `/`.

---

## 0. Read this first: what this audit did besides reading

> **An accidental daemon run, with a side effect on disk.** While checking how binaries behave when
> run with no arguments, I ran `~/ai/tools/bin/record_datagrams` with no arguments. It does not exit:
> it starts as the datagram archive daemon on its default socket `/tmp/ai_logger.sock`
> (`daemons/record_datagrams/src/main.rs:27,43,139-147`). It ran for about a minute until I stopped
> it. It did three things:
>
> - At startup it called `cleanup_socket` on `/tmp/ai_logger.sock`. No `record_datagrams` process
>   was running afterwards and no socket existed, so as far as I can tell no live daemon was
>   displaced.
> - It created `~/ai/intercept/datagrams/`. That directory and its parent are both stamped
>   2026-11-16 20:09.
> - It recorded the datagrams sent in that window into
>   `~/ai/intercept/datagrams/datagrams_2026-11-16-Z.jsonl` (11,917 bytes).
>
> I have left the directory and the file in place, because deleting them needs your approval.
> `intercept_replay` and `watch_and_diff_exchange_intercepts` were queued in the same loop and never
> ran.

> **Hook notifications.** `hook_pre_llm_tool` flagged as "probing" my reads, and the subagents'
> reads, of `capability/hook_io/`, `hooks/*`, both `rules.toml` files, the gleipnir sources and
> `SYN_DESIGN.md`. You were notified each time. These reads were needed for this audit. One subagent
> read of `~/.claude/settings.json` triggered an approval prompt.

> **Commands run.**
>
> - `cargo test -p failure_mode_core`: 7 passed and 1 doc-test passed. The error-type table in
>   MUST_READ is in step with the registry.
> - `--help`, or a no-argument run, on deployed binaries in `~/ai/tools/bin/`.
> - The subagents ran the deployed tomlx readers on scratch input piped in through `/dev/stdin`.
>
> Nothing in the repository was modified apart from this file.

> **Where each finding came from.** §1 to §3, the Priority 3 contradictions, Priority 4 and
> Priority 2 are my own work. Each was verified first-hand against the code.
>
> §4 to §7 are the reports of four read-only subagents, condensed here with their references. I
> re-checked a sample at the source and each one held:
>
> - TOMLX: reader_io:345-369 and the metadata finding.
> - gleipnir: matrix.rs:7 against the matrix TOML header, and the absence of `v2_projects.toml`.
> - SYN: config discovery, main.rs:70-75 against SYN_DESIGN.md:199-217.
> - HOOK_DESIGN: `hook_post_llm_read` uses `run_pre_hook` (main.rs:35) against doc :49; `hush`
>   is listed at :51; the "symlinks" wording is at :243.
>
> The other delegated items are references to verify, not established fact.

---

## Priority 1 — Disagreements between documentation and implementation

### §1 Root documents: NORNIR_CONVENTIONS.md

**C1 · What a bare cargo build skips.**
- Doc: `NORNIR_CONVENTIONS.md:266` says a bare `cargo build --release` "skips symlinks, verification…".
- Code: `nornir_deploy:13-16` and `:205-227` copy binaries and never symlink.
- Contradicts `MUST_READ_BEFORE_BUILDING.md:10`, which says "copied, never symlinked".

**C2 · Gate direction suffix.**
- Doc: `:65` says the direction "is `input` or `output`".
- Code: two gates use neither — `gates/gate_paths_verified` has no direction, and
  `gates/gate_raw_definition_defaults` uses `defaults` (`Cargo.toml:25,23`).
- `MUST_READ_BEFORE_BUILDING.md:29` speaks of "passthrough" and "the defaults gate". The two docs
  describe different naming spaces.

**C3 · "These are the only exceptions" (specialist tools).**
- Doc: `:53` says `saga` and `syn` are the only binaries without a verb prefix.
- Code:
  - These names in `cli/` carry no prefix from the table at `:32-51`: `cli/hush`, `cli/announce`,
    `cli/cc_launch`, `cli/dep_resolve_file`, `cli/dep_resolve_function`.
  - In `daemons/`, `relay_mic_to_voice` and `bifrost_proxy` do not use `record_`, the table's only
    prefix for `daemons/` (`:51`).
  - In `interceptors/`, `traffic_interceptor_rewriter` does not use `intercept_` (`:50`).
  - The table has no row at all for `cli/`.

**C4 · "Every Tier 3 binary follows this pattern" — the exit codes.**
- Doc: `:95-118` says 0 means success, 1 refusal or failure, and 2 wrong arguments. The CLI contract
  at `MUST_READ:43-47` says the same.
- Deployed binaries run with no arguments:

  | Binary | Exit code |
  |---|---|
  | `send_alert`, `send_heartbeat`, `send_notification`, `send_warning` | 1 (usage printed) |
  | `convert_json_to_toml` | 1 |
  | `hush` | 0 |
  | `send_datagram`, `split_jsonl_batches`, `dep_resolve_*`, `gate_raw_definition_input`, `append_raw_jsonl`, `read_toml_validate_json` | 2 |

- Hooks always exit 0 (`capability/hook_io/src/lib.rs:7`, `:165-186`).
- `record_datagrams` with no arguments starts the daemon (see §0).
- The doc covers none of these as exceptions.

**C5 · "Never a flag" against tools that carry flags.**
- Doc: `:119` and `HOW_A_NORNIR_TOOL_IS_MADE.md:88-92` say a choice the caller would otherwise make
  is fixed into the tool or becomes a separate tool, "never a flag on the existing one".
- Code: these tools carry such flags:
  - `saga`: `--force`, `--strip`, `--sidecar`, `--stdin`, `--project-dir`, `--kind`, `--test`
    (deployed `--help`).
  - `syn`: `--mode`, `--narrow`, `--tool`, `--level`, `--kind`, `--output`, `--stdin` (§5).
  - `announce`: `--profile`, `--voice`, `--speed`, …
  - `cc_launch`: `--update`, `--session`, `--workspace`, `--pick`, `--go`, `--spec-workspace` (§5).
  - `hook_pre_llm_bash`: per-category severity flags (`hooks/hook_pre_llm_bash/src/main.rs:105-133`).
- Nothing states whether these predate the rule or are exempt.

**C6 · The "Current:" list of workspace dependencies.**
- Doc: `:323` lists 25 dependencies.
- Code: `Cargo.toml:151-187` also declares 11 that the list leaves out: `quick-xml`, `rusqlite`,
  `coreaudio-sys`, `enigo`, `ratatui`, `crossterm`, `tokio`, `hyper`, `hyper-util`, `hyper-tls`,
  `http-body-util`.
- This is a hand-maintained copy of what `Cargo.toml` declares.

**C7 · Schema file naming.**
- Doc: `:81` says `schemas/agents/{stage-name}.schema.json`.
- On disk: the files are `agent-{stage}.schema.json` and `include-{…}.schema.json`, all symlinks into
  `../verdandi/{agent-builder,agent-output}/output/`.
- Some `schemas/tools/` files are also symlinks into `~/ai/spaces/bragi/` (`embedding-target`,
  `qc-report`).
- The doc says nothing about either kind of symlink.

**C8 · What senders do.**
- Doc: `:46` and `:217` say a sender sends a "fire-and-forget datagram to hlidskjalf socket", and that
  the socket path lives only in `datagram_io`. That second part holds: the only other occurrence is
  a doc comment in `record_datagrams`.
- Code: `capability/datagram_io/src/lib.rs:8-10` and `:109-149` use two transports:
  - the Unix socket goes to `record_datagrams`, the archive;
  - UDP multicast `239.0.0.1:9899` goes to Hlidskjalf.
- The code also offers three emit variants with different validation: `emit`, `emit_validated` and
  `emit_validated_or_alert` (`:3-6`).

**C9 · Adding a new crate: two procedures.**
- `NORNIR_CONVENTIONS.md:284-293` gives 8 steps, including adding the crate to the workspace
  `Cargo.toml` members.
- `MUST_READ_BEFORE_BUILDING.md:130-133` gives 2 steps and does not mention workspace membership.
- `nornir_deploy:64-77` (`find_crate_dirs`) locates maturin crates only through workspace members.

**C10 · Built tools without a CLI face.**
- Doc:
  - `:30`: "a CLI binary always".
  - `:124-148`: one crate, a `python` feature, `src/main.rs`.
  - `HOW_A_NORNIR_TOOL_IS_MADE.md:37`: "A built tool always has a CLI binary".
  - `HOW_A_NORNIR_TOOL_IS_MADE.md:149`: the after-build checklist.
- Code: `binary_readers/read_png_verify_heights`, `binary_writers/write_bytes_verify_png` and
  `binary_writers/write_bytes_verify_quantized_mesh` have no `[[bin]]` and no `python` feature
  (their `Cargo.toml`). So they are also outside the workspace test command at `:301`.

### §2 Root documents: MUST_READ_BEFORE_BUILDING.md and deploy_categories.toml

**M1 · The `help` verify method.**
- Doc: `:150` says it "expects exit 0".
- Code: `nornir_deploy:232-235` accepts exit 0 or 1.

**M2 · Category fields reference.**
- Doc: `:144-160` documents `verify`, `verify_input`, `valid_exit_codes`, `binary_names`,
  `pyo3_crates` and `python_version`.
- Fields not documented:
  - `deploy_categories.toml` uses `gate_crates`, `gate_verify`, `pyo3_verify` and `build`.
  - `nornir_deploy:18-22` and `:318-333` read `cli_crates` and `cli_verify`.
  - The automatic CLI-face build is undocumented: a PyO3 crate with a `python` feature and a
    `[[bin]]` gets its CLI face built and checked for exit 2 (`nornir_deploy:24-27`, `:352-367`).
- Keys present but never used for anything:
  - `build = "maturin"` (`deploy_categories.toml:121`) is never read by `nornir_deploy`.
  - `pyo3_verify` and `gate_verify` are read only by `_pyo3_verify` (`nornir_deploy:87-88`), which
    nothing calls. PyO3 verification is always `verify_python_import` (`:342`).

**M3 · The header comment in deploy_categories.toml.**
- Doc: `deploy_categories.toml:10` says "Gates are special: separate cli_crates (cargo) and
  gate_crates (maturin)".
- Code: the `[gates]` table has no `cli_crates`. The input gates' CLI faces come from each crate's
  `Cargo.toml` (`nornir_deploy:352-367`).

**M4 · `--bootstrap`.**
- Code: `nornir_deploy:435` exposes `--bootstrap`.
- Doc: `MUST_READ:118-126` and `NORNIR_CONVENTIONS.md:268-276` do not list it.

**M5 · A three-way disagreement over `read_tomlx_json_schema`'s data shape.**
- `MUST_READ:29` says `{"data": …, "schema": …}`.
- The code agrees (`capability/reader_io/src/lib.rs:363-369`).
- `core/format_core/TOMLX_SPEC.md:609,653-685` describes a different shape (§4, T2).

### §3 Root documents: CONTEXT_MAP.md, CLAUDE.md, README.md, HOW_A_NORNIR_TOOL_IS_MADE.md, KFM.md, REFRESH.md, tool_registry.toml

**X1 · CONTEXT_MAP: which schema a gate embeds.**
- Doc: `CONTEXT_MAP.md` (refresh table) says "the gate's `src/lib.rs` — one `include_str!` through a
  symlink into Verdandi's output".
- Code: gate `lib.rs` files hold no `include_str!`. They use a static from `schemas_embedded`, e.g.
  `gates/gate_raw_definition_input/src/lib.rs:12` (`use schemas_embedded::RAW_DEFINITION`). The
  `include_str!` calls live in `capability/schemas_embedded/src/lib.rs:17-210`.

**X2 · CONTEXT_MAP: state is "read live — nothing is stored".**
- These stored plans hold pending-item lists:
  - `plans/IMPROVEMENT_PLAN.md`
  - `core/gleipnir_core/V2_MIGRATION_PLAN.md`
  - `core/gleipnir_core/RUST_FALSE_POSITIVE_REDUCTION.md`
  - `.claude/plans/gleipnir_false_positive_reduction.md`
- The subagents report that many of those items are already in the code (§6 G19, G33, G47; §7 I35).

**L1 · CLAUDE.md: workspace identity.**
- Doc: `CLAUDE.md` (Project Notes) says `datagram::workspace_from_path`, giving
  `/Users/johnny/ai/smidja/nornir` → `@smidja/nornir`.
- Code: `capability/datagram_io/src/lib.rs:252-274` defines `datagram_io::workspace_from_path`, which
  returns `@smidja:nornir` with colon separators. The test at `:305` asserts this, and the comment at
  `:255-256` gives the reason: Svelte 5's reactive proxy.
- `NORNIR_CONVENTIONS.md:256` agrees with the code. CLAUDE.md differs from both.

**R1 · README: crate count.**
- Doc: `README.md:7` says "97 crates".
- Code: `Cargo.toml:2-148` has 145 workspace members.

**R2 · README: gate count.**
- Doc: `README.md:112` says "35 compiled Rust/PyO3 gate modules".
- Code: 37 gate crates (`Cargo.toml:22-58`; `deploy_categories.toml:123-161`).

**R3 · README: build all.**
- Doc: `README.md:143` gives `nornir_deploy --build all`.
- Code: there is no `all` category. `--build` with an unknown name exits 1 ("unknown category",
  `nornir_deploy:463-468`). The flag for everything is `--all`.

**R4 · README: architecture tree.**
- Doc: `README.md:124-136`.
- It leaves out these top-level directories: `readers/`, `text_writers/`, `binary_readers/`,
  `binary_writers/`, `presence/`, `converters/`, `dispatchers/`, `schemas/`, `examples/`.
- It describes `cli/` as "saga, syn, cc_launch, and pipeline check utilities". The crates in `cli/`
  are also `hush`, `announce`, `dep_resolve_file` and `dep_resolve_function`, and none of them is a
  pipeline check utility.
- It describes `daemons/` as "(bifrost_proxy)", but `daemons/` also holds `record_datagrams` and
  `relay_mic_to_voice`.

**R5 · README: Bifrost called a Python project.**
- Doc: `README.md:13` lists Bifrost among "Python projects in the ecosystem".
- The repo shows two sides:
  - Rust: `daemons/bifrost_proxy` (`README.md:98` itself says "async Rust reverse proxy").
  - Python: `~/ai/smidja/bifrost/src/bifrost/...` still holds `intercept_and_route.py` (§7 I27).

**R6 · README: floor rules.**
- Doc: `README.md:79` says the floor blocks the directories `.kube/` and `.docker/`.
- Code: the patterns are the specific files `/.kube/config` and `/.docker/config.json`
  (`hooks/hook_pre_llm_tool/rules.toml:34,38`).

**R7 · README: gaming detection.**
- Doc: `README.md:81` lists "Gaming detection — flags attempts to circumvent constraints".
- Code: `hooks/hook_pre_llm_tool/rules.toml` has no `[[gaming]]` entries (only 6 `[[floor]]` and 12
  `[[probing]]`), so the layer is empty (§7 H23).

**H1 · HOW: "the registry".**
- Doc: `HOW_A_NORNIR_TOOL_IS_MADE.md:22-23` and `:64-65` speak of one registry of every generated
  variant and its recipe.
- Code: there are two registries.
  - `tool_registry.toml` covers writers only. Its header (`:1-3`) says it exists "so regin can match
    an agent's output spec to the correct tool".
  - `gates/gate_recipes.toml` covers gates.
- Stores (`store_*`) are in neither; NORNIR_CONVENTIONS:198 says "for a writer" only.

**H2 · tool_registry.toml: how entries arrive.**
- Doc: `tool_registry.toml:5` says "Generated entries are appended by generate_writer.py".
- Code: `generate_writer.py:55` and `:248` print the entry for a person to add.
  `NORNIR_CONVENTIONS.md:198` says "add the printed entry".

**H3 · HOW: recipes state their user and the three lenses.**
- Doc: `:85-86` and `:143-144` say each new variant's recipe states its user and the answer to each
  lens.
- Code: no entry in `tool_registry.toml` or `gates/gate_recipes.toml` carries such fields.
- `schemas/tools/gate-recipes.schema.json` and `tool-registry.schema.json` exist. I did not check
  whether they allow such fields.

**H4 · HOW: adversarial mode and alerting.**
- Doc: `:120-130` says a tool can be built in adversarial mode. In that mode each refusal goes to
  Hlidskjalf as an Alert datagram.
- Code:
  - `core/failure_mode_core/src/lib.rs:19-48` defines `adversarial_message()` and an `alert`
    priority.
  - Outside `failure_mode_core`, no Rust or Python file calls `adversarial_message`.
  - No tool face (`capability/tool_face_io`) chooses a mode or sends an alert.
  - The word "adversarial" also appears in the two bash hooks, but with a different meaning.

**K1 · KFM.md: the rule against concrete facts versus its own entries.**
- Doc: `KFM.md:9-12` says entries are "failure shape with concrete facts routed out. An entry that
  names files and functions is a clock".
- The salvaged entries name files and functions:
  - `:84-88` names `hook_io::rules` and `write_engine::ai_home()`.
  - `:101` names `hook_pre_llm_bash`, its embedded `rules.toml` and the `git stash` rules.
  - `:68` names `cargo build --release && ln -s`.
- Those facts are currently true. `write_engine::ai_home` is at `capability/write_engine/src/lib.rs:147`.
  `hook_io::rules` is at `capability/hook_io/src/rules.rs`.

**F1 · REFRESH.md: the workspace section is unfilled.**
- Doc: `REFRESH.md:39-86` contains only template placeholders ("_Fill in: …_", "[Concern Area 1]").
- Nothing in the repo routes to `REFRESH.md`: neither CONTEXT_MAP nor CLAUDE.md mentions it.

**Verified as matching (root documents):**
- Every function named in the Dependency Lookup and in the composition examples exists, with the
  signature shown:
  - `write_engine`: `run_cli`, `write_bytes_atomic`, `append_line_fsync`, `ai_home`;
    `WriterConfig` and its 8 fields; `OutputFormat`, `WriteFrequency` and
    `OutputPath::DirectoryName`; `Stamps` re-exported from `stamp_core`, and `Stamps::NONE`.
  - `reader_io::validate_against_named_schema`.
  - `path_core::validate_path_segment`.
  - `datagram_io::emit` and `workspace_from_path`.
  - `hook_io`: `run_pre_hook`; `HookDecision` (the doc names `Allow`); `rules::parse_toml_table`
    and `parse_rule_array`.
  - `tool_face_io`: `py_contract`, `positional_args`, `cli_report`, `cli_usage`.
  - `format_core::yaml_emit::emit`.
- The worked example `readers/read_toml_validate_json/` matches `NORNIR_CONVENTIONS.md:128-144`
  exactly.
- The identity rule holds for all 145 members (directory = package = bin = lib). The only
  exceptions are `saga_cli`→`saga` and `syn_cli`→`syn`.
- No crate pins a version locally.
- Dependency direction (my scripted check of path dependencies):
  - no core crate depends on a capability crate;
  - no library crate depends on a binary;
  - no binary depends on another binary.
- Core purity: `std::fs`, `std::env` and process use in `core/` appear only in `build.rs`, examples,
  `#[cfg(test)]` code and doc comments.
- Deploy coverage: every non-library workspace member is in `deploy_categories.toml`, and every
  crate listed there is a workspace member.
- Input gates have the `python` feature and a `[[bin]]`. Output gates have the feature only.
- The `error.type` table is current with the registry (`cargo test -p failure_mode_core`).
- The CLAUDE.md `hook_io::response` summary matches `capability/hook_io/src/response.rs`:
  - `PreToolUseResponse::{allow,deny,ask}` plus `with_reason` and `with_context`;
  - `PostToolUseResponse::allow().with_context` emits `hookSpecificOutput.additionalContext`;
  - PermissionRequest is `decision.behavior` / `decision.message`;
  - `WithUniversal` offers `suppress_output`, `system_message`, `stop_session`;
  - `Ask` exists on PreToolUse only.
- The CLAUDE.md gleipnir note is accurate:
  - `clone_is_ownership_transfer` is at `core/gleipnir_core/src/checks_rs/prohibited.rs:519`;
  - it is used by `check_no_clone_spam` (`:337`, call `:358`) and `check_no_string_abuse` (`:576`,
    calls `:587,593`);
  - the FUZZY RETURNS text is in `gleipnir_messages.toml`.
- Gate Python entry points are named `validate` (CLAUDE.md "call `validate()`").
- `nornir_deploy` is reachable on PATH as `~/ai/tools/scripts/nornir_deploy`, a symlink to
  `./nornir_deploy`.
- The CONTEXT_MAP external routes exist:
  - `~/ai/smidja/galdr/context/{AGENT_BUILD_SYSTEM,NORNIR_GATES_ELEMENT,REGIN_ELEMENT,GALDR_ELEMENT}.md`;
  - § "The output tool builder" (NORNIR_GATES_ELEMENT:172);
  - § "The forward cascade" (AGENT_BUILD_SYSTEM:367).
- No schema symlink is broken.
- The seven `tool_registry.toml` entries match the seven writer crates' schema paths and outputs.

### §4 core/format_core/TOMLX_SPEC.md *(delegated; T1/T2 re-verified)*

"spec" = `core/format_core/TOMLX_SPEC.md`. "src" = `core/format_core/src/tomlx/`.

**Output and metadata**

- **T1 · The readers return no metadata.** The spec says the format is read by the `read_tomlx_*`
  readers (spec:4). Its output examples carry `section_units`, `section_paths`, `section_types` and
  `schema` (spec:358-362, 400-402, 516-522, 609-635, 653-685). `read_tomlx_json` serialises only
  `output.data` (reader_io:345-348). `TomlxOutput::to_json()` (src/types.rs:216-269) has no caller
  anywhere in the repo.
- **T2 · The schema reader's output shape.** The spec merges the schema at the top level next to
  `section_types` (spec:609, 664). The code returns `{"data","schema"}` with no `section_types`, and
  refuses a file with no `type=` sections as `tomlx_without_annotations` (reader_io:363-369;
  `failure_modes.toml:89-93`).
- **T3 · `section_types` is not stored as declared.** The spec stores the type string as declared
  (spec:696). The code stores a canonical lowercase form: `integer` becomes `int`, and
  `LIST[STRING]` becomes `list[str]` (src/types.rs:93-106).
- **T4 · No draft-07 marker.** The spec says "draft-07" (spec:691). The emitted schema has no
  `$schema` key (src/processor.rs:220-223).

**Type mode**

- **T5 · §4.6 type validation is absent.** The spec requires values to match the declared type, and
  a mismatch is an error (spec:641-649). The code only records the type (src/processor.rs:153-168),
  and src/diagnostics.rs:138-152 has no error for a mismatch. The subagent ran three cases and all
  were accepted:
  - `timeout = "fifteen"` under `type=int`;
  - `ports = [80, "http"]` under `type=list[int]`;
  - `type=int` with `1500 # unit=ms`, which gave `1.5`.
- **T6 · The §6.4 type-mismatch messages do not exist.** Spec:829-854; nothing matching in
  src/diagnostics.rs.
- **T7 · The "Invalid type syntax" message differs.** Spec:819-827 against src/annotation.rs:143-145
  and src/diagnostics.rs:226-236. The code's Fix line suggests `target=seconds`.
- **T8 · `unit=` in a type-only section is silently ignored.** Spec:142 says an annotation that
  cannot be applied is an error. Code: src/validation.rs:204-212 and src/processor.rs:197. Tested:
  `[s] # type=int` with `a = 15 # unit=minutes` returned 15 and no error.

**Paths**

- **T9 · Trailing slashes are removed.** The spec's §3.5 output keeps them (spec:509-519).
  `normalize_path` drops them (src/paths.rs:157-184, 199-200), and the unit test at src/paths.rs:282
  expects the version without the slash.
- **T10 · The §1.3 "safe" examples get mangled.** The spec calls `http://example.com` and
  `//network/share` safe (spec:107-109). The code collapses `//`, producing `/srv/http:/example.com`
  and `/network/share` (src/paths.rs:162-164). Spec:490 itself says normalisation removes duplicate
  `/`, which conflicts with spec:107-109.
- **T11 · Base declarations in later path sections.** The spec says any base definition in a later
  section is an error (spec:451). The code errors only when the name already exists
  (src/validation.rs:132-142). New names and `expand=` in later sections are dropped silently
  (tested).
- **T12 · An unresolved home directory is not an error.** The spec requires every part of a path to
  resolve, including the home directory (spec:161). With HOME and USERPROFILE both unset, the code
  returns the path unchanged (src/paths.rs:74-85, 199).
- **T13 · Expansion order.** The spec expands, then concatenates (spec:488-489). The code
  concatenates, then expands (src/paths.rs:24-32; src/processor.rs:101).
- **T14 · `path=config` with a relative file path.** The spec says "directory containing the .tomlx
  file" (spec:476, 486). For a bare filename `Path::parent()` is `""`, which the code treats like
  `absolute` (reader_io:327; src/paths.rs:24-25). This was inferred from the code and not run.
- **T15 · Reserved base names.** The spec calls base names arbitrary (spec:446). The code refuses
  ten reserved names, including `home`, `absolute`, `config`, `target`, `type` and `schema`
  (src/types.rs:179-190; src/annotation.rs:153-155).
- **T16 · No separate "Invalid path reference" error.** Spec:871-877 describes one. The code uses
  the generic undefined-reference error (src/processor.rs:510-517).

**Case-sensitivity and annotation syntax**

- **T17 · Case-sensitivity.**
  - Spec:50 says keys and values are case-sensitive. Spec:313 and spec:554 say unit, target and
    type names are not, so the spec contradicts itself.
  - The code lowercases annotation keys (src/annotation.rs:106, 288) and the values of `target`,
    `type`, `expand` and units (src/types.rs:20, 61, 106; src/units.rs:286, 291).
  - It matches path references exactly (src/processor.rs:510), even though `resolve_base`
    lowercases them (src/paths.rs:44) and base names are stored lowercase.
  - Tested: `Logs=/var/` with `path=Logs` failed.
- **T18 · The ` // ` split works on fields only.** The spec describes the split in general terms
  (spec:92-103). The code splits field annotations only (src/annotation.rs:274), not section headers
  (src/annotation.rs:53, 92).
- **T19 · Plain comments are refused.** The spec says every .tomlx file is valid TOML, that
  non-annotated sections pass through as TOML, and that comments on `[[…]]` headers are for people
  (spec:29, 138, 146). The code refuses any trailing comment that is not `key=value`, on fields and
  on `[x]` headers (src/annotation.rs:283-299, 101-104). Tested.
- **T20 · Fields take one annotation.** The spec allows several, comma-separated (spec:978). Fields
  read a single `key=value` (src/annotation.rs:283). Tested.

**Units and angles**

- **T21 · Time targets.** The spec lists `seconds`, `ms`, `milliseconds` (spec:203). The code also
  accepts `s`, `sec`, `secs` and `millis` (src/units.rs:101-102). Because spec:203 lists the names
  without grouping, the "first listed" rule at spec:313 is ambiguous for `ms`.
- **T22 · The "unknown unit" example uses a valid unit.** Spec:762 shows "Unknown unit 'mins'", but
  spec:198 lists `mins` as an alias, and the code accepts it (src/units.rs:107).
- **T23 · Angle parsing.** The spec allows N/S/E/W letters or a leading minus (spec:379). The code
  also accepts a leading `+` and lowercase letters (src/angle_parse.rs:91, 105-107).
- **T24 · Whole-number results.** Converted values with no fractional part are stored as integers
  (src/processor.rs:456-462). The spec does not say so.

**Error messages (§6)**

- **T25 · The §6.1 error structure.** The spec gives every error `ERROR: <file>:<line>`, `Rule:` and
  `Fix:` (spec:720-730). Several code messages leave parts out (src/diagnostics.rs:26, 217, 226-248,
  300-312, 373-384, 386-402).
- **T26 · Missing-annotation wording.** Spec:748, 753 against src/diagnostics.rs:275, 278.
- **T27 · Incompatible-unit wording.** Spec:807, 811 against src/diagnostics.rs:376, 378.
- **T28 · Undefined-reference Fix wording.** Spec:866 against src/diagnostics.rs:397.
- **T29 · TOML parse errors.** The spec shows a caret display and a Fix (spec:897-906). The code
  wraps the toml crate's message (src/processor.rs:50-51; core/error_core/src/lib.rs:71-72).

**Contradictions inside the spec**

- **T30 · Which sections need field annotations.** Spec:81 says fields in annotated sections must
  have annotations. Spec:125 and :559 say type-only sections do not. The code follows spec:125
  (src/validation.rs:206).
- **T31 · The §7.2 schema is not JSON Schema.** Spec:933 uses `max`, but spec:691 says JSON Schema,
  where the keyword is `maximum`. §7.1 constant generation (spec:921-925) has no implementation.
- **T32 · Numbering.** Two sections are numbered 3.3 (spec:461, 467).

**Related docs**

- **T33 · `base=config` against `path=config`.** `readers/read_tomlx_json/src/lib.rs:5` and
  reader_io:324 say `base=config`. The spec's syntax is `path=config` (spec:476).
- **T34 · Values JSON cannot carry.** `NORNIR_CONVENTIONS.md:67` says readers refuse TOML date-times
  and NaN/infinity as `file_value_not_json`. The tomlx processor turns date-times into strings and
  NaN/infinity into `null` (src/processor.rs:72-76). The spec says nothing about this.

**Verified as matching:** about 30 rules. The subagent's list includes:
- the unit tables for all ten families;
- the angle examples;
- the expand modes;
- the built-in bases;
- the orphan, unknown-unit and redeclaration messages;
- `format_core::tomlx` / `parse_tomlx`.

### §5 cli/ design documents *(delegated; SYN config discovery re-verified)*

**CC_LAUNCH_DESIGN.md (`cli/cc_launch/CC_LAUNCH_DESIGN.md`).** The subagent reports that the doc's
last edit (commit `ab8f690`, 2026-11-09) predates three cc_launch source commits (2026-11-10 and
2026-11-12). Several items below (A1, A2, A14, A19, A24, A25) are already listed in the doc's own
pending section at :255-274.

- **A1 · Where the collaboration paradigm is read from.** Doc :50 says
  `cognitive-mode/00-collaboration-paradigm.xml`. The code reads it from `foundation/`
  (`library.rs:19-24`).
- **A2 · Persona position.** Doc :50-56 puts the persona 7th. The code puts it 2nd
  (`assembly.rs:88-103`). `explainers/About This Project.md:48-49` agrees with the code (A28).
- **A3 · Language context.** Doc :66 says the prompt emits `<context><language>…` tags. No code
  emits them. Languages load `coding/<lang>/*.xml` (`library.rs:54-60, 151-185`), and the root is
  `<prompt version persona workspace coding>` (`assembly.rs:165-168`).
- **A4 · Expertise location.** Doc :67 says `expertise/*/` directories. The code reads
  `expertise/*.xml` files (`library.rs:62-66, 254-267`).
- **A5 · Descriptor tag.** Doc :75 says `<workspace>`. Every descriptor in the library uses
  `<system id=…>`.
- **A6 · Descriptor families.** The lists at doc :88-91 differ from the library: `nornir-gates`
  belongs to the agent pipeline; `cc-launch` and `hook-chain` have `parent="nornir"`; `announce`,
  `control-panel`, `relay-mic-to-voice` and `workspace-registry` are standalone.
- **A7 · Python always included.** Doc :101 and :159 say Python is always included when coding is
  enabled. The code starts with no language selected and selects only what descriptors list
  (`model.rs:207-209, 257-291`).
- **A8 · Which languages exist.** A language exists only if `coding/<lang>/` holds fragments
  (`library.rs:174-177`). Only `coding/python/` exists, so the `rust`, `svelte` and `typescript`
  attributes on descriptors select nothing. Doc :226 and :101 show them as selectable.
- **A9 · The example `[nornir]` profile.** Doc :122-128 differs from the real profile, which has 6
  expertise ids and a `permissions` key.
- **A10 · Who controls permissions.** Doc :107-111 and `permissions.rs:1-11` say permissions are
  bound to the persona with no TUI control. The header comment of
  `~/ai/control/cc_launch_profiles.toml:3` says they are "always manual in TUI".
- **A11 · Profile defaults.** Doc :143 says a profile fills the persona. The code fills it only when
  no persona was chosen explicitly (`model.rs:94-97, 395-408`).
- **A12 · Auto clears profile defaults.** Doc :148 says so. The code keeps an explicit persona, and
  otherwise resets to `odinn` (`model.rs:152, 487-491`).
- **A13 · Coding toggle.** Doc :161-164 says language selections persist. Re-enabling coding
  recomputes languages from descriptors, and toggling coding also selects or deselects expertise
  with `domain="coding"` (`model.rs:504-522`).
- **A14 · Prompt output path.** Doc :179, :181, :239 and :306 say `/tmp/cc_launch_prompt.xml`. The
  code writes to `std::env::temp_dir()` (`assembly.rs:72`), which on this machine is under
  `/var/folders/...`. It also writes `<workspace>/.SYSTEM_PROMPT.xml` (`assembly.rs:76-78`).
- **A15 · Launch sequence.** Doc :172-182 describes a single exec. The code also has a two-phase
  `--pick`/`--go` path through `$TMPDIR/cc_launch_spec.toml`, and an `--update` path
  (`main.rs:34-48, 174-188`; `assembly.rs:177-197, 225-315`). `~/.zshrc:33-46` routes normal
  launches through `--pick`/`--go`.
- **A16 · CLI flags.** Doc :205 and :308-316 show no flags beyond passthrough. The code and deployed
  `--help` add `--update`, `--session`, `--workspace`, `--pick`, `--go` and `--spec-workspace`
  (`main.rs:21-53`).
- **A17 · TUI layout.** Doc :215-234 differs from the code's columns and Tab order
  (`tui.rs:198-230`; `model.rs:117-134`).
- **A18 · Summary column.** Doc :225 and :234 show "Est. tokens" and an ordered context. The code
  shows "Tokens" and an "Always N fragments" line, and lists an auto-matched space as context item 1
  (`tui.rs:455-473, 521-533, 621-635`).
- **A19 · "None" persona.** The sketch at :222 shows `○ None`. The persona list has no such entry
  (`tui.rs:327-339`), which doc :263 also notes.
- **A20 · Auto's starting persona.** Doc :252 says it is the first persona alphabetically. The code
  defaults to `odinn` (`model.rs:147-168, 211`).
- **A21 · Auto and the space block.** Doc :251 says Auto never loads the space's block. The code
  auto-matches a space when the persona id equals the space id and the path contains `/spaces/{id}`
  (`model.rs:183-193, 293-312`).
- **A22 · Heimdallr.** Doc :254 says it has no space descriptor and no profile. Both now exist
  (`~/ai/control/library/spaces/heimdallr.xml`, the `[heimdallr]` profile).
- **A23 · CWD auto-detection.** Doc :283 lists it as not done. Auto now resolves to the launch
  directory, but no profile is matched from it (`model.rs:98-100, 216, 293-302`).
- **A24 · Key Files table.** Doc :299-305 lists files that do not exist at those paths:
  `~/ai/control/library/PROMPT_DESIGN.md` (it is under `_orientation/`), `systems/README.md`,
  `spaces/README.md`, `systems/DISPATCH_PROTOCOL.md` and `~/ai/tools/scripts/start_tyr`.
- **A25 · The cc-launch system descriptor.** `~/ai/control/library/systems/cc-launch.xml:21` says
  `cc_launch --workspace nornir` pre-selects a profile. In the code `--workspace` is a path and
  requires `--update` (`main.rs:30-32`). The descriptor's line 18 says `.SYSTEM_PROMPT.md`; the code
  writes `.SYSTEM_PROMPT.xml`. (The descriptor is library content, outside this workspace.)
- **A26 · Two copies of the design doc.** `~/ai/control/library/_orientation/CC_LAUNCH_DESIGN.md`
  differs throughout from the copy in the repo.
- **A27 · Source-file table.** The rows for `library.rs` and `main.rs` (doc :205, :207) leave out
  `domain` parsing and the spec and update branches.

**explainers/About This Project.md and HOW_TO_WRITE_EXPLAINERS.md.**
`HOW_TO_WRITE_EXPLAINERS.md` is a symlink to `~/ai/sites/tiriganiarjuk.earth/HOW_TO_WRITE_EXPLAINERS.md`.

- **A28 · Persona position.** About :48-49 contradicts CC_LAUNCH_DESIGN :50-56; the code agrees with
  About.
- **A29 · Placeholder environment variables.** About :78-80 says a pending item covers env-var
  names an earlier session invented. CC_LAUNCH_DESIGN has no such item, and calls
  `HOOK_LLM_ALLOW_PATHS`/`_PATTERNS` the mechanism (:111). Those are live: set at `main.rs:147-154,
  227-234` and read at `hooks/hook_pre_llm_tool/src/main.rs:76, 391`.
- **A30 · Prompt re-injection.** About :82-85 describes the interim fix as a note in shared memory.
  The repo also has `hook_session_start_inject`, which re-injects the stored prompt
  (`hooks/hook_session_start_inject/src/main.rs:1-13`), wired at `~/.claude/settings.json:52`.
- **A31 · Required headings.** HOW_TO :95-105 requires 8 headings in a fixed order. About is missing
  three of them and adds two others (About :7-87).
- **A32 · Sibling links.** HOW_TO :102 asks for links to sibling About pages; About :69-74 has none.
- **A33 · File names.** HOW_TO :144 forbids file names. About :89 names `CC_LAUNCH_DESIGN.md`, which
  sits outside `explainers/`.
- **A34 · Unattributed verdict.** HOW_TO :60-62 forbids the writer's own opinions. About :78 says
  "working well" without attribution.

**SYN_DESIGN.md (`cli/syn_cli/SYN_DESIGN.md`).**

- **S1 · Config discovery.** Doc :197-217 and :262 say syn walks up the tree from the input path.
  The code loads only `<project_dir or cwd>/.syn/`, with no walk-up (`cli/syn_cli/src/main.rs:70-75,
  381-390`). Re-verified.
- **S2 · `--project-dir` and the scan path.** Doc :174, :252-253 and :315 say `--project-dir X`
  scans X. In the code it sets only the config root and workspace name; the scan path is the
  positional argument or "." (`main.rs:395, 417-418`). The help text differs as well (doc :232
  against `main.rs:155`).
- **S3 · Gate-mode stdin.** Doc :182, :185 and :228 describe gate mode reading the new `.qa` on
  stdin, and :343 calls this unbuilt. The code reads stdin only with an explicit `--stdin` flag, in
  either mode (`main.rs:151-153, 392-397`). The real hook pipeline is
  `saga <f> --sidecar | syn --stdin` (`hooks/hook_post_llm_tool/src/main.rs:4, 84-98`).
- **S4 · Flags.** The doc's CLI block lacks `--stdin`, `--kind` and `--narrow`. It lists fixed value
  sets for `--tool` and `--level`, which the code takes as free strings (`main.rs:151-181`;
  `report_render_core/src/lib.rs:133-141`). Gate mode rejects `--narrow` (`main.rs:200-203`), which
  the doc does not say.
- **S5 · Ratchet.** Doc :128-153 and :268 present `.syn/ratchet.toml` with three modes and a
  default. No `.rs` file contains "ratchet". The gate denies when any visible issue also passes the
  deny filter (`core/syn_core/src/lib.rs:154-173`). The doc itself says the ratchet is not built
  (:333, :341-342), so it contradicts itself.
- **S6 · Gate output.** Doc :161-162 describes the output. The code prints the issue groups plus one
  `[syn] DECISION` stderr line (`main.rs:403-414`).
- **S7 · Broadcast.** Doc :105 and :280-283 say the broadcast is on by default and its payload
  includes issues, the ratchet comparison, the decision and the workspace. The code broadcasts only
  when there are visible issues or faults; the payload is groups and faults, and the decision sits
  in `detail` (`main.rs:349-354, 416`).
- **S8 · Report-mode exit code.** Doc :106 says "Exit 0 always". The code exits 2 when a config
  file is refused or stdin JSON is invalid (`main.rs:46-55, 305-318, 445-450`).
- **S9 · Session-start report.** Doc :309-316 describes a hook-triggered syn report at session
  start. No hook and no settings.json entry invokes syn that way.
- **S10 · File types.** Doc :27-28 says saga runs "on Python files"; doc :291 and saga `--kind` cover
  py, rs and svelte.
- **S11 · Dependencies.** Doc :323-324 lists `format_core` and `error_core`, which neither syn crate
  depends on; `failure_mode_core` is a dependency but is not listed (`cli/syn_cli/Cargo.toml:10-19`,
  `core/syn_core/Cargo.toml:10-16`). Doc :335 says `report_render_core` is "shared with svalinn";
  nothing under `~/ai/smidja/yggdrasil` references it.
- **S12 · syn's own header comment.** `main.rs:12-15` mentions `--colored` and `--json`. The real
  flag is `--output` (`main.rs:143-145`), as the doc's :236 says.

**ANNOUNCE_NOTES.md (`cli/announce/ANNOUNCE_NOTES.md`).**

- **N1 · Severity as a layer.** Doc :35 lists severity as a resolution layer, and :36-37 calls it
  "metadata only", so the doc contradicts itself. The code uses `--severity` only to let `critical`
  bypass `RECORDING.lock` (`cli/announce/src/main.rs:117-123`; `announce_core/src/lib.rs:96-156`).
- **N2 · VOICE.lock.** VOICE.lock and the workspace locks are consulted only when `--source`
  resolves to a workspace (`main.rs:99-112, 145-152`). VOICE.lock does not supply the model
  (`announce_core/src/lib.rs:109-144`). The doc says neither.
- **N3 · Locks.** The doc lists `SILENT.lock` and `QUIET.lock` (:21, :30-32). The announce binary
  checks only SILENT. QUIET is checked by `hooks/hook_stop_llm_tts/src/main.rs:73-80`. The global
  `RECORDING.lock` and its queue are undocumented (`main.rs:116-124`).
- **N4 · Default backend.** Doc :11 says kokoro is the default, which the code and config confirm.
  The clap `about` says "ElevenLabs TTS CLI" (`main.rs:18`). `--no-cache` says it skips storage, but
  Kokoro always writes the cache (`main.rs:59-61, 212-216, 399`).

**core/compaction_inject_core/instructions/compaction_summary.md.** The whole file is embedded with
`include_str!` (`core/compaction_inject_core/src/lib.rs:10`) and appended last to the request's
`system` array (`:15-28`).

- **N5 · The header is sent to the model.** The HTML comment header (:1-5, including
  `ccVersion: 2.1.38`) is part of what the model receives.
- **N6 · "Described above".** Line :8 says "the work described above". The block is appended to
  `system`, which precedes `messages`, so the conversation is not above it.

### §6 core/gleipnir_core design documents and the gleipnir plan *(delegated; G1/G16 re-verified)*

"GC" = `core/gleipnir_core/`. The subagent reports that all 1,282 cells of `GLEIPNIR_MATRIX.md` match
`gleipnir_matrix.toml`.

**GLEIPNIR_MATRIX.md**

- **G1 · Where to edit the matrix.** `gleipnir_matrix.toml:3` says "DO NOT EDIT — edit
  GLEIPNIR_MATRIX.md and run generate_matrix_toml.py". `src/matrix.rs:7` says "To change which
  checks apply to a file type, edit gleipnir_matrix.toml". Re-verified.
- **G2 · `no_closures` has no row.** It is a distinct check with its own messages entry
  (`gleipnir_messages.toml:218`), raised from inside `no_nested_functions`
  (`checks_py/prohibited.rs:352`). It appears in no matrix row.
- **G3 · Orchestrate IO.** `impure_module_quarantine` is unset for Orch, OrchDis and Entry
  (MATRIX:103; toml:60-68). `V2_ZONE_ARCHITECTURE.md:159` and `GLEIPNIR_PHILOSOPHY.md:50` say
  orchestrate contains no IO.

**GLEIPNIR_PHILOSOPHY.md**

- **G4 · Gravity at CC=1 in simple.** :69 says a CC=1 function in simple/ is a gravity violation.
  The statistics set `[v2.l2] min_cc = 1` (`gleipnir_statistics.toml:77`), the check is
  `cc < min_cc` (`checks_py/style.rs:754`), and a test asserts no violation (`style.rs:1174`).
- **G5 · "Calls orchestrate only".** :67 says this of the entry point. L8 may import any lower level
  (`structures.rs:64-69, 115`).
- **G6 · "At project root".** :67 says entry points live at the project root. They are matched by
  filename at any depth (`classify.rs:136`).
- **G7 · `import json`.** :84 says it is banned in pure zones. It is banned globally
  (`checks_py/imports.rs:322`).
- **G8 · "primitive/" as a directory.** :52 describes it so. Level comes from the filename stem
  (`classify.rs:68-83`).

**RECOVERY_FROM_EMBARRASSING_FAILURE.md**

- **G9 · Return type.** :7 says `Vec<Violation>`. The code returns `Result<Vec<Violation>, String>`
  (`lib.rs:67-70, 95-98, 132`).
- **G10 · Level enum.** :30 says 8 variants. It has 10, L0–L8 plus Outside (`structures.rs:29-40`).
- **G11 · Zone enum.** :30 says 5 variants. It has 9 (`structures.rs:77-87`).
- **G12 · Level names.** :19 names levels that differ from the code's numbered L0–L8, which include
  dispatch L3/L6 and assembled L5 (`structures.rs:25-39`).
- **G13 · CC bands.** :23 gives bands that differ from the statistics (`statistics.toml:76-78, 91-93,
  113-115`).
- **G14 · Transform imports.** :21 says transform imports only from structure/. The code allows
  Transform→Transform (`structures.rs:114`).
- **G15 · Pending item already present.** :42 lists per-level thresholds and `for_v2()` as missing.
  Both exist (`structures.rs:202-238`; `statistics.toml:69-126`).
- **G16 · Routing file.** :38 and :44 refer to `v2_projects.toml` and say no project is migrated.
  That file does not exist. Routing is v2 by default with an opt-out list,
  `capability/saga_runner/v1_legacy_projects.toml` (`saga_runner/src/lib.rs:22, 59-64, 89-92`).
  Draupnir and regin are not on that list and have v2 layouts. Re-verified that the file is absent.
- **G17 · Directory-encoded levels.** :25 implies levels come from directories.
  `logic/pure/primitive/foo.py` classifies as Outside (`classify.rs:68-111, 225-230`).
- **G18 · Message count.** :37 says "4 new check types". `gleipnir_messages.toml:548-602` has 10
  v2-era entries.

**RUST_FALSE_POSITIVE_REDUCTION.md**

- **G19 · Written as pending, but in the code.** The whole plan is implemented:
  `is_value_producing_position` (:448), `RustCrateTier` and `classify_rust_tier` (:233, :241),
  `in_run_function` (:262), the `let_declaration` arm (:555), and the proposed tests (:1499-1700).
  All in `checks_rs/prohibited.rs`.
- **G20 · Stale line numbers.** At :17, :66 and :216.
- **G21 · `is_implicit_return`.** Referenced at :33, :82 and :124; it no longer exists. Its role is
  covered by `is_last_expression_in_block` (:431) and `is_value_producing_position` (:448).
- **G22 · Scope of the `run()` exemption.** :289 and :308 say it covers only `fn run()`. The code
  also accepts `run_`-prefixed names (`prohibited.rs:270`).
- **G23 · `let mut` detection.** :410-412 proposes text matching. The code checks for a
  `mutable_specifier` child (`prohibited.rs:555-559`).
- **G24 · Where `RustCrateTier` lives.** :517 and :524 recommend `classify.rs` or a new
  `classify_rust.rs`. It is in `prohibited.rs:233`, and `classify_rust.rs` does not exist.
- **G25 · Rebuild command.** :537 says `nornir_deploy --build gates`. gleipnir_core's only dependent
  is saga_runner, which saga_cli uses, in the `[tools]` category (`deploy_categories.toml:56-71`).
- **G26 · Cited lines.** Many cited locations no longer show the described pattern: `diff_core:182`,
  `tomlx/validation.rs:128`, `report_render_core:47,330`, `convert.rs:199`,
  `tomlx/annotation.rs:176`, `error_core:129`, `saga_cli/main.rs:182`.

**V2_MIGRATION_GUIDE.md**

- **G27 · Routing.** :13 and :202 say "Add project to `saga_runner/v2_projects.toml`", which does not
  exist (see G16).
- **G28 · `logic/entry_point.py`.** :40 names it as an entry point. Only `cli.py` and `__main__.py`
  are entry points (`classify.rs:114`); `entry_point.py` gets "file outside v2 zone layout"
  (`imports.rs:670-674`).
- **G29 · Same-module import restriction.** :94 describes one. None exists (`imports.rs:728-769`).
- **G30 · CC counting.** :110 lists the constructs that count. The code also counts comprehension
  for/if, `case`, `assert` and loop/try `else` (`style.rs:625-697`).
- **G31 · Dependency injection.** :133 says it is not a valid solution; the checklist at :213 says
  "resolved (DI or level adjustment)". Internal contradiction.
- **G32 · Structure subdirectories.** The guide (:203), `V2_ZONE_ARCHITECTURE.md:22-24, 142-145` and
  the code (`classify.rs:56-60`; `architecture.rs:874`) each give a different set.

**V2_MIGRATION_PLAN.md**

- **G33 · Phases written as pending, but present.** Phases 2a–2c are in the code; :17 is
  contradicted by `check_v2_cc_level` (`style.rs:731`); the 3a/3b moves are done.
- **G34 · Opt-in or opt-out.** :26, :81-86, :121 and :171 describe an opt-in list. The code routes
  v2 by default with an opt-out list (`saga_runner/src/lib.rs:69-92`).
- **G35 · Lookup tables.** :66-67 call for static `LEVEL_IMPORTS`/`ZONE_IMPORTS` tables. The code
  uses `Level::can_import` and `Zone::can_reach` (`structures.rs:64, 108`).
  `V2_ZONE_ARCHITECTURE.md:102` says "No lookup table", so the two docs disagree too.
- **G36 · FileKind examples.** :14 gives examples that differ from the actual variants
  (`structures.rs:5-14`).
- **G37 · Path examples.** :56, :132 and :134 assume levels come from directories. The code
  classifies by filename stem (`classify.rs:69-81`).

**V2_ZONE_ARCHITECTURE.md**

- **G38 · Level numbers.** :66 gives dispatch as L2/L5 and primitive/ffi as L0. The doc's own table
  (:71-76) and the code give L3/L6 and L1 (`classify.rs:72-77`).
- **G39 · Gravity at CC=1 in L2.** :84; same as G4.
- **G40 · `__init__.py`.** :188 says it is Outside with no checks. In logic zones it gets the full
  zone check list (`classify.rs:161-162`; `matrix.rs:240-253`); in structure/ it gets L0
  (`classify.rs:156-158`).
- **G41 · `cli.py` / `__main__.py` "(project root)".** :186-187. They match at any depth
  (`classify.rs:136-141`).
- **G42 · Zone count.** :190-198 lists 5 zones; the code has 9 (`classify.rs:50-61`).
- **G43 · Constants in structure/.** :147 and :241 say any non-class module-level binding is a
  violation. The code flags only collection literals, comprehensions and frozenset
  (`architecture.rs:1251-1261`).
- **G44 · Transform limits.** The stricter `[v2.transform.l*]` limits (:157) may never take effect.
  The TOML nests under `transform` (`statistics.toml:135-145`), while the code looks up the flat key
  `"transform.l2"` (`structures.rs:287, 298-299`). The subagent inferred this from reading and did
  not run it.
- **G45 · Rule count.** :92 says "Two Rules"; :124 and :243 say "Three Rules". The checklist (:222)
  leaves out impure→transform/orchestrate (`structures.rs:113`).
- **G46 · Transform imports.** :233 says "Transform imports structure/ only"; the doc's own :111
  allows transform→transform.

**.claude/plans/gleipnir_false_positive_reduction.md**

- **G47 · Steps written as to-do.** Steps 1–5 are all present in the code (see G19). The block arm
  is at :544, not :418 (:130).
- **G48 · The tail-expression case.** :117-126 says it delegates to `is_implicit_return`. It now
  recurses through `is_value_producing_position` (`prohibited.rs:486-508`).
- **G49 · Build command.** :218 says `--build gates`; same as G25.

**Data**

- **G50 · Settings nothing reads.** `[v2.l3]`, `[v2.l6]` and `[v2.l8]` set
  `max_functions_outside_zones` (`statistics.toml:89, 111, 126`). The only consumer runs for
  `v1.outside` alone (`architecture.rs:789`; `matrix.toml:18`).

### §7 hooks/HOOK_DESIGN.md, the rules files, interceptors/INTERCEPT_DESIGN.md, plans/IMPROVEMENT_PLAN.md *(delegated; H1/H17/H18 re-verified)*

**HOOK_DESIGN.md**

- **H1 · `hook_post_llm_read`'s event.** :49 lists it under PostToolUse. The code is a PreToolUse
  hook: `hooks/hook_post_llm_read/src/main.rs:1` and `:35` (`run_pre_hook`), wired under PreToolUse
  at `~/.claude/settings.json:106-116`. The name's `post_` contradicts doc :18 ("`post_` means
  assess"). Re-verified.
- **H2 · The naming pattern.** Doc :10-16 gives `hook_{event}_{context}_{scope}` and prefixes at
  :29/:31. Actual names put context first (`hook_session_start_*`, `hook_session_end_handoff`), use
  an unlisted event (`hook_notify_llm_tts`), and use unlisted scopes (`tts`, `log`, `read`,
  `inject`, `handoff`).
- **H3 · PostCompact.** It has no row in the event table (:23-38), yet `hook_compact_session_log` is
  wired to it (:53; `settings.json:67-76`).
- **H4 · PermissionRequest wire format.** :34 says `hookSpecificOutput.permissionDecision`. The code
  writes `hookSpecificOutput.decision.behavior` (`response.rs:274-297`). Verified by me as well.
- **H5 · Notification output.** :36 says exit code only. The code emits `additionalContext`
  (`response.rs:716-729`).
- **H6 · ConfigChange output.** :37 says exit code only. The code emits `decision:"block"` and
  `reason` (`response.rs:596-609`).
- **H7 · Stop, SubagentStop and UserPromptSubmit.** :27, :28 and :35 say exit code plus side
  effects. The builders support block/reason, and UserPromptSubmit also supports `additionalContext`
  (`response.rs:453-473, 506-519, 550-563`).
- **H8 · `permissionDecision` values.** :92 shows "allow|deny". The code also emits "ask"
  (`response.rs:173`), and doc :103-105 describes Ask, so the doc contradicts itself.
- **H9 · `HookDecision` variants.** :103 lists 4. The code also has `AllowWithContext`
  (`capability/hook_io/src/lib.rs:52-55`). Verified by me as well.
- **H10 · `run_hook`.** :140 calls it a deprecated alias. It has no `#[deprecated]` attribute
  (`lib.rs:348-354`), and four pre hooks still call it.
- **H11 · `read_stdin()`.** :166 lists it as a shared internal; no such function exists.
- **H12 · Side effects.** :126-129 say every Warn/Deny/Assessment produces a datagram, a
  terminal-notifier alert and a log line. Only pre-hook decisions do (`lib.rs:193-216`);
  `run_post_hook` does none of it (`lib.rs:583-608`). Ask decisions also fire an `announce` voice
  alert (`lib.rs:400-438`), which the doc does not mention.
- **H13 · Rule parsing.** :162 says "all pre hooks" use `hook_io::rules`. The subagent hooks and
  `hook_post_llm_read` do not.
- **H14 · Subprocesses.** :188 says there are no subprocess calls. `hook_io` spawns
  `terminal-notifier` (`lib.rs:279`) and `announce` (`lib.rs:428-437`).
- **H15 · Hook dependencies.** :222 says `hook_io` + `serde_json`. The manifests list more, and
  seven hooks do not depend on `hook_io` at all.
- **H16 · Workspace member list.** :230-239 lists 6 hooks. There are 12 (`Cargo.toml:100-111`), and
  the doc's own table at :44-56 lists 13.
- **H17 · `hush`.** :51 lists it as a hook. It is at `cli/hush` and deploys in `[tools]`.
  Re-verified.
- **H18 · Deploy method.** :243 says nornir_deploy "symlinks into `~/ai/tools/bin/`". The code
  copies. Re-verified; same as C1.
- **H19 · Settings example: bash flags.** :258 shows `--revert warn` and no `--chaining`. Live
  settings have `--revert ask --chaining block` (`settings.json:102`).
- **H20 · Timeout.** :264 shows `30000`; settings has `30` (`settings.json:135`). Doc :315 says
  "30s", which contradicts the doc's own `30000`.
- **H21 · Settings example: missing wiring.** :249-267 leave out Notification, SessionStart,
  PostCompact, Stop, the Read hook, UserPromptSubmit and SessionEnd.

**rules.toml files**

- **H22 · `HOOK_LLM_ALLOW_PATTERNS`.** `hook_pre_llm_tool/rules.toml:17` names only
  `HOOK_LLM_ALLOW_PATHS`. The code also reads `HOOK_LLM_ALLOW_PATTERNS` (`main.rs:76, 136-139,
  184-186`).
- **H23 · Empty gaming layer.** `rules.toml:13` defines a gaming layer and settings pass
  `--gaming warn`, but the file has no `[[gaming]]` entries. Verified by me as well.
- **H24 · `HOOK_LLM_ALLOW_BASH`.** `hook_pre_llm_bash/rules.toml:20` calls its values "pattern
  names". Rules have no names; the code substring-matches against the regex text
  (`main.rs:248`).
- **H25 · "Block unconditionally".** `rules.toml:106-107` says destruction is blocked
  unconditionally. Entries in the same category carry `severity = "ask"` (:127-140), and the
  category's severity comes from a CLI flag (`main.rs:179`).
- **H26 · Rebuild instruction.** Both rules files say "After editing: cargo build --release" (:8).
  The bash workflow rule asks before exactly that command and points to `nornir_deploy`
  (`rules.toml:169-172`).

**INTERCEPT_DESIGN.md** (dated "Current as of 2026-03-19" at :3)

- **I27 · `bifrost_proxy` is never mentioned.** The doc says the live path is the PyO3 module
  (:108) and talks of restarting mitmproxy (:235). `daemons/bifrost_proxy/src/main.rs:1` is a
  "Rust reverse proxy replacing mitmproxy" running the same pipeline (`:227-285`).
- **I28 · Consumers.** :27-29 and :39-45 name two consumers. `bifrost_proxy` also uses
  `intercept_core`, `compaction_inject_core` and `session_io`, and `rewrite_compaction_summary`
  uses `compaction_inject_core`.
- **I29 · Who writes `raw_session_log.jsonl`.** :79 says the rewriter only. `bifrost_proxy` writes it
  too, with a different write primitive (`bifrost_proxy/src/main.rs:240-241` against
  `traffic_interceptor_rewriter/src/lib.rs:35-51`).
- **I30 · Who spawns the watcher.** :32 says replay. `bifrost_proxy` does as well
  (`main.rs:324-368, 503-505`).
- **I31 · Schema copies.** :211-213 count two. There is a third, non-symlinked copy embedded by
  `bifrost_proxy` (`daemons/bifrost_proxy/cc_wire_schema.json`, `main.rs:83-84`); it is identical
  today.
- **I32 · "Single code path".** :44 says `session_io` is the single code path for derived file
  writes. :84 assigns `running_transcript.jsonl` to the watcher, which writes it with its own code
  (`watchers/watch_and_diff_exchange_intercepts/src/main.rs:330-356`).
- **I33 · Files missing from the layout.** :58-85 leave out `compact_summary.jsonl`,
  `thinking_log.jsonl` and `sessions/unresolved/*_req.raw`.
- **I34 · `intercept_io`.** Not listed at :23-25. It is a workspace member (`Cargo.toml:20`), and
  nothing depends on it.

**IMPROVEMENT_PLAN.md**

- **I35 · Items still listed as pending that the code or docs show done.**
  - S4: `senders/announce/` is gone.
  - D2: done. The plan's mechanism is also wrong: `hook_session_start_inject/src/main.rs:134-140`
    builds its JSON by hand and does not use `SessionStartResponse`.
  - D3: done, but the code now has a 7th category, chaining.
  - D4 and D5: done.
  - D1/C1: `CONTEXT_MAP.md` no longer stores inventories.
- **I36 · Claims of "zero" or "no doc".** S5 says saga_runner has zero tests; it has a `#[cfg(test)]`
  module. M2 says gleipnir has no design doc and M4 that announce has none; both exist.
- **I37 · Count.** M1 (:116) says "4 crates" and then lists 8.

---

## Priority 3 — Contradictions between documents

| # | Topic | Source A | Source B | Code |
|---|---|---|---|---|
| P1 | Deploy copies or symlinks | `NORNIR_CONVENTIONS.md:266` and `HOOK_DESIGN.md:243`: "symlinks" | `MUST_READ:10`: "copied, never symlinked" | copies (`nornir_deploy:13,205-227`) |
| P2 | Workspace identity format | `CLAUDE.md`: `datagram::…` → `@smidja/nornir` | `NORNIR_CONVENTIONS.md:256`: `@smidja:nornir` | `@smidja:nornir` (`datagram_io/src/lib.rs:269,305`) |
| P3 | Adding a crate | `NORNIR_CONVENTIONS.md:284-293` (8 steps, including workspace membership) | `MUST_READ:130-133` (2 steps) | deploy needs workspace membership (`nornir_deploy:64-77`) |
| P4 | Gate naming | `NORNIR_CONVENTIONS.md:65`: input or output | `MUST_READ:29`: passthrough and defaults gates | two gates have neither suffix |
| P5 | `read_tomlx_json_schema` output | `MUST_READ:29`: `{"data","schema"}` | `TOMLX_SPEC.md:609,664`: merged with `section_types` | `{"data","schema"}` |
| P6 | Values JSON cannot carry | `NORNIR_CONVENTIONS.md:67`: refused as `file_value_not_json` | `TOMLX_SPEC.md`: silent | tomlx converts them to string or null (`processor.rs:72-76`) |
| P7 | Persona position | `CC_LAUNCH_DESIGN.md:50-56`: 7th | `About This Project.md:48-49`: early | 2nd (`assembly.rs:88-103`) |
| P8 | Matrix edit point | `gleipnir_matrix.toml:3`: edit the .md | `src/matrix.rs:7`: edit the .toml | generated by `generate_matrix_toml.py` |
| P9 | v2 routing | RECOVERY, V2_MIGRATION_GUIDE, V2_MIGRATION_PLAN: opt-in `v2_projects.toml` | nothing else | opt-out `v1_legacy_projects.toml` |
| P10 | Import lookup tables | `V2_MIGRATION_PLAN.md:66-67`: static tables | `V2_ZONE_ARCHITECTURE.md:102`: "No lookup table" | methods `can_import` / `can_reach` |
| P11 | Structure subzones | `V2_MIGRATION_GUIDE.md:203` | `V2_ZONE_ARCHITECTURE.md:22-24,142-145` | `classify.rs:56-60` (a third set) |
| P12 | `RustCrateTier` location | `RUST_FALSE_POSITIVE_REDUCTION.md:517,524`: classify.rs | `.claude/plans/…:158`: prohibited.rs | `prohibited.rs:233` |
| P13 | Gleipnir rebuild category | RUST_FPR:537 and plan:218: `--build gates` | `CONTEXT_MAP` / system descriptor: `--build tools` (saga) | gleipnir_core reaches deploy only through saga_cli, in `[tools]` |
| P14 | Gleipnir design docs exist | `IMPROVEMENT_PLAN.md` M2: none | the 4 docs in `core/gleipnir_core/` | — |
| P15 | Announce doc exists | `IMPROVEMENT_PLAN.md` M4: none | `cli/announce/ANNOUNCE_NOTES.md` | — |
| P16 | Stored plans | `CONTEXT_MAP.md`: "Nothing is stored" | 4 stored plan documents | — |
| P17 | Registry of variants | `HOW_A_NORNIR_TOOL_IS_MADE.md`: one registry | `tool_registry.toml:1-3`: writers only, for regin | two registries (tool_registry, gate_recipes) |
| P18 | How registry entries arrive | `tool_registry.toml:5`: appended by the generator | `NORNIR_CONVENTIONS.md:198`: add the printed entry | printed (`generate_writer.py:248`) |
| P19 | `HOOK_LLM_ALLOW_*` env vars | `About This Project.md:78-80`: invented, awaiting replacement | `CC_LAUNCH_DESIGN.md:111`: the mechanism | live in cc_launch and hook_pre_llm_tool |

Internal contradictions inside a single document: T17, T30, T31 (TOMLX_SPEC); S5 (SYN_DESIGN);
N1 (ANNOUNCE_NOTES); G31 (V2_MIGRATION_GUIDE); G38, G45, G46 (V2_ZONE_ARCHITECTURE); H8, H16, H20
(HOOK_DESIGN); I32 (INTERCEPT_DESIGN); I37 (IMPROVEMENT_PLAN); A19 (CC_LAUNCH_DESIGN); K1 (KFM).

---

## Priority 4 — Invisible crates (absent from NORNIR_CONVENTIONS.md § Dependency Lookup)

The lookup table is at `NORNIR_CONVENTIONS.md:221-260`. These workspace members are core or
capability crates and do not appear in it:

| Crate | Tier | What it is (from its source / dependents) |
|---|---|---|
| `default_apply_core` | core | applying schema defaults. The pure half under `gate_raw_definition_defaults` |
| `dep_resolve_core` | core | Python import and dependency closure. Under `dep_resolve_file`, `dep_resolve_function` and `hook_post_llm_read` |
| `announce_core` | core | TTS settings resolution and cache paths |
| `time_core` | core | time primitives |
| `text_core` | core | text primitives |
| `saga_core` | core | `SanityReport` / `Issue` types and sidecar naming (`saga_core/src/lib.rs:83`) |
| `datagram_core` | core | `Datagram`, `DatagramKind`, `Priority`, re-exported by `datagram_io` |
| `compaction_inject_core` | core | embeds `instructions/compaction_summary.md` and injects it into requests |
| `intercept_core` | core | exchange classification (`classify_exchange`, `ExchangeKind`) |
| `stamp_core` | core | in prose only (`:180`), not a row |
| `store_engine` | capability | in prose only (`:190`), not a row |
| `default_apply_io` | capability | read, apply defaults, validate, write (`read_apply_validate_write`) |
| `workspace_registry` | capability | SQLite registry of workspaces and sessions; used by session and TTS hooks |
| `session_io` | capability | derived session-file writes: `record_compaction` and the like |
| `intercept_io` | capability | intercept I/O functions. **No crate depends on it** (§7 I34) |
| `keystroke_io` | capability | keystroke simulation for `relay_mic_to_voice` |

The lookup table and the crates' current purposes agree for the rows that exist. I checked each
named function against the source (§3, "Verified as matching").

---

## Priority 2 — Missing design rationale

Each group below shares a dependency chain or a directory and has no committed design document in
this workspace. For each I name what a fresh session would most likely get wrong.

> **D1 · The datagram subsystem.** `datagram_core`, `datagram_io`, `send_*` ×5,
> `record_datagrams`, `watch_and_diff_exchange_intercepts`, and the emitters in hooks, syn and
> bifrost.
>
> - Undocumented: the dual transport (Unix socket to the archive, UDP multicast to live consumers),
>   the three emit variants and when each applies, self-alerting on transport failure, the UDP size
>   limit, the `:` workspace separator and why it exists, and the datagram schema.
> - The rationale lives only in `datagram_io/src/lib.rs:1-17` and `:252-257`.
> - Already listed as IMPROVEMENT_PLAN M1.
> - Likely error: a sender written against "the hlidskjalf socket", or the CLAUDE.md `/` format.

> **D2 · saga.** `saga_core`, `saga_runner`, `saga_cli`, and `v1_legacy_projects.toml`.
>
> - No saga design doc exists.
> - Undocumented: v2-by-default routing with a v1 opt-out list, which tools saga runs and their
>   configs (`saga_runner/ruff.toml`, `pyrightconfig.json`), the `--test v1|v2` override, and the
>   sidecar format.
> - The only routing description is in three gleipnir docs, and it names a file that does not
>   exist (G16, G27, G34).
> - Likely error: creating `v2_projects.toml`, or "migrating" projects that are already v2.

> **D3 · Dependency resolution.** `dep_resolve_core`, `dep_resolve_file`,
> `dep_resolve_function`, and `hook_post_llm_read`.
>
> - No document mentions this subsystem anywhere in the repo.
> - Undocumented: its controls (`DEP_RESOLVE_DISABLE.lock`, `DEP_RESOLVE_MODE`,
>   `DEP_RESOLVE_MAX_DEPTH`), which appear only in `hooks/hook_post_llm_read/src/main.rs:13-16`, and
>   the reason a context-injecting PreToolUse hook is named `post_`.
> - Likely error: putting it in the PostToolUse chain, as HOOK_DESIGN does (H1).

> **D4 · Binary formats and elevation.** `binary_format_core`, `elevation_core`,
> `binary_reader_io`, `binary_writer_io`, `read_png_verify_heights`, `write_bytes_verify_png`, and
> `write_bytes_verify_quantized_mesh`.
>
> - Only the naming rules (`NORNIR_CONVENTIONS.md:71-73`) and one interface row (`MUST_READ:33`)
>   exist.
> - Undocumented: why terrain and elevation tiles are in nornir, who calls these tools, what
>   "terrarium" and the height rules are, what bad-pixel repair guarantees, and why these three
>   crates have no CLI face and no `python` feature when the built-tool rule says otherwise (C10).

> **D5 · Schema defaults.** `default_apply_core`, `default_apply_io`, and
> `gate_raw_definition_defaults`.
>
> - Undocumented: which defaults are applied, from where (the schema's `default` keywords?), why
>   this one gate writes and also returns data, and why its name has no direction suffix (C2).
> - Likely error: a second defaults path written by hand in Python.

> **D6 · Presence tools.** `presence_io`, `create_flag_generate_lock`, and
> `remove_flag_generate_lock`.
>
> - Only the naming rows (`:43-44`) and one interface row exist.
> - No document says what `generate_lock` guards or who creates or removes it. A grep of
>   `~/ai/smidja` `*.md`/`*.py`/`*.toml` finds no consumer outside the crates' own manifests and the
>   deploy list.

> **D7 · Voice I/O beyond the announce binary.** `relay_mic_to_voice`, `keystroke_io`, `hush`,
> `hook_stop_llm_tts`, `hook_notify_llm_tts`, and the `RECORDING.lock` queue.
>
> - `ANNOUNCE_NOTES.md` covers announce only.
> - Undocumented in this workspace: the RECORDING.lock and queue coordination, and QUIET.lock being
>   checked by a hook rather than by announce (N3).
> - Already listed as IMPROVEMENT_PLAN M4.

> **D8 · Session prompt lifecycle.** `workspace_registry`, `hook_session_start_orient`,
> `hook_session_start_inject`, `hook_session_end_handoff`, and cc_launch `--update`.
>
> - The move of `.SYSTEM_PROMPT.xml` into the session directory, the `CLEAR_HANDOFF.xml` slot after
>   `/clear`, and the stale-slot rule are described only in the cc-launch and hook-chain system
>   descriptors under `~/ai/control/library/`, outside this workspace.
> - `HOOK_DESIGN.md` lists the hooks but not the lifecycle. `About This Project.md` describes a
>   different mechanism (A30).

> **D9 · bifrost_proxy.**
>
> - The design authority is `~/ai/projects/network-proxy/DESIGN.md`. That file exists, but no
>   document in nornir points to it.
> - `INTERCEPT_DESIGN.md` describes the interceptor pipeline without the proxy that now runs it
>   (I27-I31).
> - The proxy embeds its own copy of the wire schema (I31).
> - Likely error: changing `traffic_interceptor_rewriter` or the schema and assuming the live path
>   follows.

> **D10 · The write engine's design.** `write_engine`, `store_engine`, `stamp_core`, and
> `generate_writer.py`.
>
> - Usage is documented (`NORNIR_CONVENTIONS.md:154-198`, `MUST_READ:36`).
> - Undocumented rationale: the `OutputPath` variants, the rule that `Jsonl` files must already
>   exist while `Json` files must not (`write_engine/src/lib.rs:29-34`), and why stores sit apart
>   from the tool registry.
> - Already listed as IMPROVEMENT_PLAN M3.

**Architectural decisions present in the code with no written reason:**

- `intercept_io` exists as a workspace member that nothing depends on (I34).
- `examples/` is an empty top-level directory. No tier or category in `NORNIR_CONVENTIONS.md:9-20`
  accounts for it.
- `cli/` holds tools with no verb prefix (`hush`, `announce`, `cc_launch`, `dep_resolve_*`), and
  the verb table has no `cli/` row (C3).
- Some `schemas/tools/*.schema.json` files are symlinks into `~/ai/spaces/bragi/`, so that space
  owns them; the rest are local (C7).
- The deploy config carries keys nothing reads (`build`, `pyo3_verify`, `gate_verify`; M2).
- PyO3 crates whose PyO3 code is not behind the feature are excluded from the workspace test
  command (`NORNIR_CONVENTIONS.md:301`). This leaves the binary readers and writers,
  `traffic_interceptor_rewriter` and `intercept_io` untested by `cargo test --workspace`.

**Routing gap.**
- `CONTEXT_MAP.md`'s refresh table routes to none of the subsystem design documents:
  `hooks/HOOK_DESIGN.md`, `interceptors/INTERCEPT_DESIGN.md`, `cli/syn_cli/SYN_DESIGN.md`,
  `cli/cc_launch/CC_LAUNCH_DESIGN.md`, `cli/announce/ANNOUNCE_NOTES.md`,
  `core/gleipnir_core/*.md` and `plans/IMPROVEMENT_PLAN.md`.
- `TOMLX_SPEC.md` is reached only through NORNIR_CONVENTIONS.
- A session sent by the map to "what crates exist" or "how a tool is made" has no route to the
  subsystem rationale that does exist.

---

## What a fresh session would most likely get wrong, ranked by damage

1. **Following the gleipnir V2 documents.** They describe a migration, a routing file and level
   semantics that the code has moved past: directory-encoded levels, an opt-in list, 5 zones,
   L0/L2 numbering, gravity at CC=1. A session could "implement" work that is already done, or
   create `v2_projects.toml` (G16-G46).
2. **Building TOMLX callers against the spec.** A session could expect metadata in reader output
   (T1-T2), type validation (T5), preserved trailing slashes (T9), or plain TOML comments (T19).
3. **Editing hooks from HOOK_DESIGN.md.** A session could wire `hook_post_llm_read` under
   PostToolUse, write the PermissionRequest reply in the wrong shape, assume hooks never spawn
   subprocesses, or copy the stale settings example (`--revert warn`, `timeout 30000`) (H1-H21).
4. **Changing the interceptor pipeline from INTERCEPT_DESIGN.md.** A session would not know that
   `bifrost_proxy` runs the pipeline, writes the same files and embeds its own schema copy
   (I27-I33).
5. **Configuring or invoking syn from SYN_DESIGN.md.** A session could rely on walk-up config
   discovery, on `--project-dir` as the scan path, or on a ratchet (S1-S7).
6. **Writing new tools by the stated rules where the tree does otherwise.** The exit-code contract
   (C4), "never a flag" (C5), "a CLI binary always" (C10) and gate suffixes (C2) each have live
   counter-examples and no written exception.
7. **Using CLAUDE.md's workspace-identity format.** The real format uses `:`, not `/` (L1/P2).
8. **Reimplementing an invisible crate.** The likeliest are `workspace_registry`, `session_io`,
   `datagram_core`, `dep_resolve_core` and `default_apply_core` (Priority 4).
