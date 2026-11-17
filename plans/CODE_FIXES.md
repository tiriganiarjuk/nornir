# Code fixes

Forward-only: an item is removed when it is done. Nothing here records status.

Sorted by severity, then by type. Sections 1–6 are fixes in place. Section 7 is decomposition —
logic that has to be taken apart and moved into the right crate — set aside as its own body of
work.

The security hooks, `hook_io`'s decision path, the quality hook, gleipnir, saga and syn are Tyr's; their findings are in
`~/ai/spaces/tyr/ATTENTION.md`. A tool owed a move onto an existing, better tool is in
`REQUIRED_UPGRADES.md`, not here.

**Source marks.** `[A+B]` both structural auditors found it — the audit's actionable set.
`[A]` / `[B]` one auditor only: placed by severity, but unconfirmed. `[run]` the auditor
executed a probe. `[inferred]` reasoned from code, not run. `[Tyr]` found by Tyr while working
in the crates that are his. Every item carries the reference to
check it in one lookup.

---

## Order of work

Each phase is committed on its own. A finished item leaves this list and its section below.

- **Phase 2 — cc_launch.** Refuse to launch when `.SYSTEM_PROMPT.xml` cannot be written (§3);
  `process::exit` only in `main()` (§5); the hard-coded home paths (§5). One commit each.
- **Phase 3 — interception removal.** Tag nornir `archive/interception-v1` and push it. Remove
  `capability/intercept_io`, `interceptors/traffic_interceptor_rewriter`,
  `interceptors/INTERCEPT_DESIGN.md` and the `~/ai/smidja/bifrost` repo; their items leave this
  list and `DOC_FIXES.md`. `intercept_core` and `compaction_inject_core` stay.
- **Phase 4 — the tomlx engine** (§3), after the spec work in `DOC_FIXES.md`.
- **Phase 5 — remaining malfunctions.** The announce queue; announce's config and state files,
  each given a schema; `hush`'s `pkill -f` regex; the two ways to find `announce`.
- **Phase 6 — small confirmed fixes.** Confirm the jsonschema `resolve-file` finding, then turn
  the feature off for core crates; the `convert_json_to_toml` usage comment; the `announce --help`
  text.
- **Phase 7 — decomposition** (§7). Workspace identity first — it carries the collision in §3 —
  then `ai_home`, lock check plus announce spawn, the cc_launch core crate, dep_resolve, the
  smaller ones. Each crate gets its tests as it is taken apart; `workspace_registry` and
  `tool_face_io` get theirs at the start of the phase.

**Held for the author's decision**, one at a time, each settled as binding or as an exception
written down with its reason: typed errors and exit codes for hand-written binaries, the senders
included; the naming in §6 — gate suffixes, the verb-prefix exceptions (`cli/`, daemons,
interceptors); "never a flag" and announce's flags; the error registry's adversarial mode; the
variant registry and engine-written failure text. PyO3 behind the `python` feature for the binary
readers and writers waits on the binary-tools stop in `REQUIRED_UPGRADES.md`.

**Waiting on the network-proxy agent** (`~/ai/projects/network-proxy/PLAN.md`): everything in
`bifrost_proxy` and `intercept_replay`.

---

## 1. Critical — security

- **A session id from outside is joined into a path unchecked.** No `validate_path_segment` in
  `daemons/bifrost_proxy/src/main.rs:233` (id from request metadata). `[B]`

## 2. Critical — fail open

- **The live interception path fails open on schema mismatch; the PyO3 path fails closed.**
  `bifrost_proxy/src/main.rs:248-255` forwards; `traffic_interceptor_rewriter/src/lib.rs:94-99`
  errors. `[A+B]`

## 3. Malfunction — wrong behaviour today

- **cc_launch swallows a failed write of `.SYSTEM_PROMPT.xml`.** `cli/cc_launch/src/assembly.rs:78`
  (`let _ = fs::write`). If it fails, the session is never re-injected after compaction. `[B]`

- **Workspace identity collides.** Orient registers by basename, so two projects with the same
  last path component share `control/workspaces/{name}/` and a registry row; Hlidskjalf receives
  two identity formats. `[A+B]` (the primitive is §7)

- **The announce queue loses entries.** `relay_mic_to_voice/src/main.rs:204-209` reads then
  removes the file; a line appended in between is lost. `[B]`

- **The wire schema is stored three times.** `daemons/bifrost_proxy/cc_wire_schema.json` and
  `interceptors/traffic_interceptor_rewriter/cc_wire_schema.json` are separate files (replay
  symlinks to the second); identical today, nothing keeps them so. Belongs in `schemas/` and
  `schemas_embedded`. `[A+B]`

- **Config and state files fall back to defaults on a parse error.** `cli/announce/src/main.rs`
  `announce.toml` (`:311`), `hits.toml` (`:331`, resets all counts), `VOICE.lock` (`:147-152`,
  ignored). `[A+B]`

- **`hush` interpolates a directory unescaped into a `pkill -f` regex.** `cli/hush/src/main.rs:51-54`. `[B]`

- **The tomlx engine differs from its spec.** `core/format_core/TOMLX_SPEC.md` is the authority;
  §1–§5 carry every decision. Test-first against the spec in `format_core::tomlx`, one change per
  commit, `cargo test -p format_core`, then `nornir_deploy --build readers` (every `read_tomlx_*`
  crate):
  - enforce `type=` before deriving the schema (§4.6);
  - accept any comment in unannotated sections, ` // ` notes on headers too (§1.3–1.4);
  - case-insensitive annotations and references (§1.1);
  - refuse date-times, NaN and infinities exactly as the TOML reader does (§1.6);
  - path mode — keep trailing `/`, expand bases only, refuse any base or `expand=` in a later
    header, error on unresolved home or variables, `config` for a bare filename (§3.2, §3.5);
  - several annotations per field (§8.2);
  - the date-time mode, built on `core/time_core` (§2.7).

## 4. Test integrity

- **No tests on contract-bearing crates:** `workspace_registry`, `tool_face_io`, `bifrost_proxy`,
  `intercept_replay`, the binary readers and writers, the writers. About 50 crates in all. `[A+B]`

## 5. Contract and convention

- **No hand-written binary meets the typed-error contract** (`run() -> Result<_, EngineError>`,
  `error_type()` on stderr's first line): cli/ (saga_cli and syn_cli are Tyr's), converters/, dispatchers/, watchers/, daemons/,
  rewriters/, senders/send_datagram, intercept_replay. `[A+B]`
- **Senders exit 1 on wrong arguments** (`senders/send_*/src/main.rs:5-7`); convention is 2. `[A+B]`
- **`process::exit` inside `run()`** — `cli/cc_launch/src/main.rs:106, 122`. `[A+B]`
- **PyO3 code not behind the `python` feature**, so no test binary links:
  `binary_readers/read_png_verify_heights`, `binary_writers/write_bytes_verify_png`,
  `binary_writers/write_bytes_verify_quantized_mesh`, `interceptors/traffic_interceptor_rewriter`. `[A+B]`
- **`capability/intercept_io`** is a PyO3 module in the capability tier, depended on by nothing,
  in no deploy category. Decide: move, or remove. `[A+B]`
- **cc_launch hard-codes `/Users/johnny/...`** in `permissions.rs:24-31`. `[A+B]`
- **Two ways to find `announce`:** `relay_mic_to_voice` uses `$PATH` (`:181, 233`); every other
  site uses `ai_home()/tools/bin`. `[A+B]`
- **Data with no schema file:** the cc_launch fragment library, announce's `announce.toml`,
  `hits.toml` and `VOICE.lock`, the announce queue,
  `compact_summary.jsonl`, `thinking_log.jsonl`, `running_transcript.jsonl`.
  `schemas/tools/tool-registry.schema.json` exists and nothing reads it. `[A+B]`
- **The jsonschema `resolve-file` feature is on for core crates** — a `$ref` can read the
  filesystem during validation inside tier 1. `[B]`

## 6. Naming

- `hook_post_llm_read` is a PreToolUse hook. `[A+B]`
- `bifrost_proxy`, `relay_mic_to_voice` (daemons/ prefix is `record_`); `traffic_interceptor_rewriter`
  (interceptors/ prefix is `intercept_`); `intercept_io` carries a binary prefix in capability/. `[A+B]`
- cli/ has no prefix row: `announce`, `hush`, `cc_launch`, `dep_resolve_*`. `hush` is a
  UserPromptSubmit hook living in cli/. `[A+B]`
- Gate direction slot: `gate_raw_definition_defaults`, `gate_paths_verified`; the paths gates embed
  `PATHS_RESOLVED` under a different stage name. `[A+B]`
- `convert_json_to_toml` usage comment names `json_to_toml`. `[A+B]`
- Misleading flags: `announce --quiet` means dry-run beside `QUIET.lock`; live flags named
  `_play_*`. `[B]` `announce --translate` only
  prints "not yet implemented". `[A]` `announce --help` calls it an "ElevenLabs TTS CLI"
  (`cli/announce/src/main.rs:18`); kokoro is the default backend. `[A+B]`

## 7. Decomposition — logic to take apart and move

Each of these is one missing primitive, not many local problems.

- **The interception pipeline.** One routing function in `session_io` or `intercept_core`, used by
  `bifrost_proxy`, `traffic_interceptor_rewriter` and `intercept_replay`. Move bifrost's pure SSE
  and request-parsing functions (`main.rs:110-160, 646-819`) into `intercept_core`. `[A+B]`
- **Workspace identity.** Four to five schemes (basename, registry prefix, `@a:b`, per-site
  fallbacks). One owner for "who and where is this session", including the prompt-file names now
  repeated as literals across five crates. `[A+B]`
- **Lock check plus announce spawn**, written six times. `[A+B]`
- **`ai_home`**, three to four homes with different fallbacks. `[A+B]`
- **cc_launch** — 2287 lines, no core crate: assembly order, descriptor ordering, language
  derivation, the persona-to-grant table. `[A+B]`
- **dep_resolve** — `dep_resolve_file` and `dep_resolve_function` near-identical;
  `detect_project_root` three times. `[A+B]`
- **Smaller:** `compute_batches` (split_jsonl_batches:70), the watcher's helpers, announce's
  cache state machine and queue encoding. `[A+B]`
