# HANDOVER — from Heimdallr's audit session, 2026-11-17

Read this, act on it, then delete it. It points at the files that hold the work; it does not
repeat them. Orient through `CONTEXT_MAP.md` first, as always.

## Where the work lives

- `plans/CODE_FIXES.md`, `plans/DOC_FIXES.md` — the audit findings, forward-only, by severity.
- `REQUIRED_UPGRADES.md` — moves onto existing tools. Its binary-tools entry is a **stop**: the
  author must approve the byte-carrying CLI shapes and the reader's payload form first.
- `audit/` — the guides, the dispatch template, and the 2026-11-16 reports the lists cite.
- `~/ai/spaces/tyr/ATTENTION.md` — every finding in Tyr's tools (security hooks, `hook_io`'s
  decision path, the quality hook, gleipnir, saga, syn). Not ours to touch.
- `~/ai/projects/network-proxy/ATTENTION.md` (+ two companion reports) — the old interception code,
  handed to the bridge's agent to decide what becomes a tap.

## Decided, not yet written into the plans

The user asked to see two **order-of-work plans** (one per list) before approving them. They were
drafted in conversation and not approved yet. Re-draft them from the lists with these decisions
folded in, show them, and on approval put each at the top of its list as "Order of work":

- **Code, Phase 0 first:** `nornir_deploy` verify steps get stdin from `/dev/null` and a timeout
  (Tyr's hang on `hush`); remove `_pyo3_verify` and the unread keys.
- **cc_launch refuses to launch** when it cannot write `.SYSTEM_PROMPT.xml`.
- **Interception is superseded** by the bridge plan. Its repair items leave CODE_FIXES and
  DOC_FIXES. `intercept_core` and `compaction_inject_core` stay. Removing `intercept_io`,
  `traffic_interceptor_rewriter` and the `smidja/bifrost` repo is decided — first tag nornir
  `archive/interception-v1` and push it. Everything else waits on the network-proxy agent's
  answer in its `PLAN.md`. `INTERCEPT_DESIGN.md` goes with the code.
- **"A built tool is a CLI binary first" is binding** (recorded in REQUIRED_UPGRADES). Still to
  write: one sentence in CONVENTIONS — a generated variant has the face its use requires, so an
  output gate has none.

## tomlx

`core/format_core/TOMLX_SPEC.md` 0.4.0 is the authority; §1–§5 carry every decision.

**Spec work left:** §6 — match the messages to `src/tomlx/diagnostics.rs`, and drop the `mins`
unknown-unit example (it is a valid alias); §7.1 constant generation has no implementation, move it
to §9; §7.2 `max` → `maximum`; §8.2 — fields take several comma-separated annotations (date-time
needs `time=local, tz=…`); the Appendix output block → the three readers' outputs from §5, and a
date-time line.

**Engine work, test-first against the spec** (`format_core::tomlx`, then rebuild
`readers` — every `read_tomlx_*` crate): enforce `type=` before deriving the schema (§4.6);
accept any comment in unannotated sections, ` // ` notes on headers too (§1.3–1.4); case-insensitive
annotations and references (§1.1); refuse date-times, NaN and infinities exactly as the TOML reader
does (§1.6); path mode — keep trailing `/`, expand bases only, refuse any base or `expand=` in a
later header, error on unresolved home or variables, `config` for a bare filename (§3.2, §3.5);
several annotations per field (§8.2); the date-time mode, built on `core/time_core` (§2.7). Make
one change per commit, `cargo test -p format_core`, then `nornir_deploy --build readers`.

## Decisions still open, one at a time with the user

3.2 gate suffixes · 3.3 verb-prefix exceptions (`cli/`, daemons, interceptors) · 3.4 "never a
flag" · 3.5 exit codes and typed errors for hand-written binaries · 3.6 the error registry's
adversarial mode, which nothing calls · 3.7 the variant registry (two registries, no user/lens
fields) and engine-written failure text. Then 4: the compaction prompt's HTML header sent to the
model; Bifrost in README's Python list; syn's CLI override replacing the warn filter (Tyr's
descriptor item); `REFRESH.md` fill or delete. For each: binding (code changes) or an exception
written down with its reason. The user wants context and a lean for each, one question per turn.

## Waiting on others

- **Tyr** is working in the security hooks. `hooks/HOOK_DESIGN.md` is rewritten only after that
  lands (DOC_FIXES §1).
- **The network-proxy agent** answers its ATTENTION.md.
- The local CA's `api.anthropic.com` leaf expires **2026-11-21** (network-proxy PLAN.md).
- `projects/server-management/servers/hosts/vps/default.nix` had a live uncommitted edit by
  someone else; it was left alone.
