# Documentation fixes

Forward-only: an item is removed when it is done. Nothing here records status.

Sorted by damage: what makes a session build or run the wrong thing comes first. Within a
severity, grouped by document.

The gleipnir, saga and syn documents and the security hooks' `rules.toml` files are Tyr's; their findings are in
`~/ai/spaces/tyr/ATTENTION.md`.

**Source marks.** `[A+B]` both documentation auditors found it — the actionable set. `[A]` / `[B]`
one auditor only, unconfirmed. `[Tyr]` found by Tyr while working in the crates that are his.
**decide** — the code may be the side that drifted; settle which before editing the document. Unmarked items are a stale document against settled code.

---

## Order of work

Each step is committed on its own. A finished item leaves this list and its section below.

1. **`core/format_core/TOMLX_SPEC.md`** — §1–§5 carry every decision, and the engine is brought
   to them (`CODE_FIXES.md` Phase 4). Spec work: §6 — match the messages to
   `src/tomlx/diagnostics.rs`, and replace the `mins` unknown-unit example (`mins` is a valid
   alias); §7.1 constant generation moves to §9, having no implementation; §7.2 `max` →
   `maximum`; §8.2 — fields take several comma-separated annotations (date-time needs
   `time=local, tz=…`); the Appendix output block → the three readers' outputs from §5, and a
   date-time line.
2. **`NORNIR_CONVENTIONS.md`** — one sentence: a generated variant has the face its use requires,
   so an output gate has none. The Binary Structure and Tool Structure templates agree.
3. **`hooks/HOOK_DESIGN.md`** — rewritten against the hooks as they are (§1).
4. **The session-prompt library** (§5) — `saga.xml`, `guardrail-system.xml`, and the settled
   items in `syn.xml`.
5. **`core/format_core` promises** (§4).
6. **The cc_launch documents** (§2), after `CODE_FIXES.md` Phase 2. The copy under
   `~/ai/control/library/_orientation/` becomes a pointer to the one in the crate.
7. **The Dependency Lookup and the missing design documents** (§4) — the subsystems that are
   settled first (datagram transport, binary formats, presence tools, writers and stores, gate
   generation); those being taken apart follow `CODE_FIXES.md` Phase 7. `CONTEXT_MAP.md` gains
   each route as its document lands. `KFM.md`.

**Held for the author's decision** — every **decide** item, one at a time, each settled as
binding (the code changes) or as an exception written down with its reason.
`interceptors/INTERCEPT_DESIGN.md` is removed with its code (`CODE_FIXES.md` Phase 3).

**Waiting on the network-proxy agent** (`~/ai/projects/network-proxy/PLAN.md`):
`systems/bifrost.xml`, and Bifrost in `README.md`.

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
- the write hook drawn as a pipeline, `saga <file> --sidecar | syn --stdin` (`:204`, `:300`) —
  `hook_post_llm_tool` assesses in-process through `syn_runner::assess_written_file`. `[Tyr]`
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
- `REFRESH.md` — unfilled template, routed from nothing. Fill or delete. `[A+B]`
- `KFM.md` — entries name files and functions, against its own prelude. `[A]`

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

**`core/format_core` — what a reader and a writer promise, stated where a caller reads it.** A
session using a reader or writer has to be able to answer a question about its behaviour by
reading the description of that behaviour — in the crate's document and on the functions
themselves — and today the only way to answer one is to run a probe. Found by the site-tools
session, which needed to know whether a markdown round trip keeps the order of frontmatter keys.
It measured the answer on 2026-11-17 over the 265 key files of the first site: the markdown
reader emits frontmatter keys in file order, the markdown writer emits them in the order it is
given, and reader → writer is byte-identical on all 265. State that, and the same class of fact
for every format the crate handles:
- key order — on read, and on write;
- which values are quoted on write, and in which style (dates, numbers and booleans held as
  strings);
- how each kind of mapping and list is laid out on write (block or flow);
- whether `write(read(file)) == file` holds, and for which inputs;
- what happens to comments, blank lines and trailing whitespace;
- a markdown body: what is preserved byte for byte, the line ending, the final newline.

## 5. Outside nornir — the session-prompt library (Heimdallr's)

- `systems/bifrost.xml` — "dumb plumbing, all logic in taps"; `bifrost_proxy` does the work
  in-process. The descriptor states the design in `~/ai/projects/network-proxy/DESIGN.md`; whether
  `bifrost_proxy` is the drift or the descriptor is, **decide** `[B]`
- `systems/syn.xml` — "No staleness warning exists" (`:8`): syn reports a sidecar whose source
  has changed as a fault; a change to check logic is the case nothing warns of. "Syn has no
  dependency on gleipnir_core" (`:13`): `syn_cli` links it through `syn_runner` and
  `saga_runner`. "CLI overrides further narrow" (`:6`): in `syn_core::is_visible` a command-line
  selector is used in place of the warn filter — **decide**. `[Tyr]`
- `systems/saga.xml` — the sidecar concept (`:6`) names the content hash and none of what reads
  it: the four sidecar states, `saga <dir>` refreshing what is not current, `saga <dir> --check`.
  `[Tyr]`
- `systems/saga.xml`, `systems/guardrail-system.xml` — the rebuild after a check change is given
  as `nornir_deploy --build tools`; the write hook links gleipnir too, so it is
  `--build tools,hooks`. `[Tyr]`
