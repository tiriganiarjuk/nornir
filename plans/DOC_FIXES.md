# Documentation fixes

Forward-only: an item is removed when it is done. Nothing here records status.

Sorted by damage: what makes a session build or run the wrong thing comes first. Within a
severity, grouped by document.

The gleipnir, saga and syn documents and the security hooks' `rules.toml` files are Tyr's; their findings are in
`~/ai/spaces/tyr/ATTENTION.md`.

**Source marks.** `[A+B]` both documentation auditors found it — the actionable set. `[A]` / `[B]`
one auditor only, unconfirmed. **decide** — the code may be the side that drifted; settle which
before editing the document. Unmarked items are a stale document against settled code.

---

## 1. High — a session following the document builds or runs the wrong thing

**`core/format_core/TOMLX_SPEC.md`** — the spec and the engine disagree on its central rules. All **decide**:
- `type=` is never enforced; a schema-reader result can violate its own schema. `[A+B]`
- metadata blocks (`section_units`, `section_paths`, `section_types`, `schema`) never reach
  reader output; the schema reader returns `{"data","schema"}`. `[A+B]`
- any trailing comment that is not `key=value` is refused; the spec says plain comments pass. `[A+B]`
- trailing `/` stripped, join-then-expand order, reserved base names, later-section bases dropped
  silently, `section_types` canonicalised, no draft-07 marker, case-sensitivity contradicting
  itself, §6 messages differing from `diagnostics.rs`. `[A+B]`
- two TOML→JSON converters treat date-times and NaN differently (`format_core::parse` refuses,
  `tomlx` converts). `[A+B]`

**`hooks/HOOK_DESIGN.md`**:
- `hook_post_llm_read` listed as PostToolUse — it is PreToolUse. `[A+B]`
- PermissionRequest wire format, `HookDecision` (missing `AllowWithContext`), PostCompact row,
  side effects (pre path only), `read_stdin()` (does not exist), `run_hook` "deprecated". `[A+B]`
- the settings example (`--revert warn`, no `--chaining`, `timeout 30000`, missing hooks). `[A+B]`
- 6 hooks listed against 12; `hush` listed as a hook; `hook_compact_session_preserve` does not
  exist; the `hook_intercept_*` rename note. `[A+B]`

**`interceptors/INTERCEPT_DESIGN.md`** — describes the PyO3 module under mitmproxy as the live
path and never names `bifrost_proxy`; raw log, watcher spawning, schema copies, file layout and
deploy category all describe one path of two. Neither path is running now (no `bifrost_proxy`,
mitmproxy or nginx process). Which design the document describes — the PyO3 path,
`bifrost_proxy`, or the bridge in `~/ai/projects/network-proxy/DESIGN.md` — **decide** `[A+B]`

## 2. Medium — a reader builds a wrong model

**`NORNIR_CONVENTIONS.md`**:
- "saga and syn are the only exceptions" to the verb prefixes — cli/, daemons/, interceptors/ say
  otherwise; cli/ has no row. **decide** `[A+B]`
- gate direction is "input or output" — passthrough and defaults gates exist. **decide** `[A+B]`
- "a built tool always has a CLI binary" (also HOW) — the binary readers and writers have none.
  **decide** `[A+B]`
- "never a flag", the exit-code contract, typed errors, pure `decide` — most hand-written tools
  break them; nothing states whether they are exempt. **decide** `[A+B]`
- the `hook_` row says "intercepts a tool call" — seven hooks fire on session events. `[B]`
- Binary Structure template (`Result<String, EngineError>`) vs Tool Structure
  (`Result<Option<String>, E>`). `[B]`

**`HOW_A_NORNIR_TOOL_IS_MADE.md`**:
- adversarial mode and alerting — `adversarial_message()` exists, nothing calls it, no tool has a
  mode. **decide** `[A+B]`
- one registry with user and lens answers — two registries (`tool_registry.toml`,
  `gate_recipes.toml`), neither with those fields; the gate-recipe schema forbids them. **decide** `[A+B]`
- "no engine writes failure text of its own" — several engines outside `failure_mode_core` do. **decide** `[B]`

**`cli/cc_launch/CC_LAUNCH_DESIGN.md`** — assembly order (persona 2nd, `foundation/`), language
handling, expertise path, `<system>` tag, prompt path, CLI flags, launch sequence, pending items
the code has passed. `[A+B]` The separate copy at `~/ai/control/library/_orientation/CC_LAUNCH_DESIGN.md`
differs throughout. `[A]`

**`cli/cc_launch/explainers/About This Project.md`** — env-var names "await replacement" (they are
live); prompt re-injection mechanism; breaks HOW_TO_WRITE_EXPLAINERS headings and rules. `[A+B]`

**`core/compaction_inject_core/instructions/compaction_summary.md`** — its HTML header
("for the SDK", `ccVersion`) is injected into live requests. **decide** `[A+B]`

## 3. Low — stale facts

- `README.md` — lists Bifrost among the Python projects; `~/ai/smidja/bifrost` is Python, and
  `daemons/bifrost_proxy` is Rust. **decide** `[A+B]` It advertises gaming detection, whose rules
  are empty — follows Tyr's fix. `[A+B]`
- `NORNIR_CONVENTIONS.md` — the "Current:" dependency list omits 11 (derive it, or remove it). `[A+B]`
- `REFRESH.md` — unfilled template, routed from nothing. Fill or delete. `[A+B]`
- `KFM.md` — entries name files and functions, against its own prelude. `[A]`
- `tool_registry.toml:5` — "appended by generate_writer.py"; it prints the entry. `[A]`
- `STRUCTURAL_AUDIT_GUIDE.md` P9 known-areas list — the `no_println` exemptions are built; the
  Rust check list omits `param_count_rs`. `[A+B]`

## 4. Missing — subsystems with no design document

Datagram transport · dep_resolve · binary formats and elevation · conditional defaults and
gate generation · presence tools · voice / locks / queue · session-prompt lifecycle · bifrost_proxy ·
writers and stores. `[A+B]`

Crates absent from the Dependency Lookup — core: `announce_core`, `compaction_inject_core`,
`datagram_core`, `default_apply_core`, `dep_resolve_core`, `intercept_core`, `saga_core`,
`stamp_core`, `text_core`, `time_core`; capability: `default_apply_io`, `intercept_io`,
`keystroke_io`, `session_io`, `store_engine`, `workspace_registry`. `format_core`'s row omits XML,
Markdown and tomlx. `[A+B]`

`CONTEXT_MAP.md` routes to none of the subsystem design documents. `[A]`

## 5. Outside nornir — the session-prompt library (Heimdallr's)

- `systems/bifrost.xml` — "dumb plumbing, all logic in taps"; `bifrost_proxy` does the work
  in-process. The descriptor states the design in `~/ai/projects/network-proxy/DESIGN.md`; whether
  `bifrost_proxy` is the drift or the descriptor is, **decide** `[B]`
