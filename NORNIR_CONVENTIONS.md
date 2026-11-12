# Nornir Conventions

Single source of truth for naming, organization, composition, and building rules. Every fact stated once.

---

## Architecture: Three Tiers

```
Tier 1: CORE (pure libraries, no I/O)
    core/*_core — depend only on other core crates + workspace external deps

Tier 2: CAPABILITY (feature libraries, may have I/O)
    capability/* — depend on core + other capability crates

Tier 3: BINARIES (executables and Python extensions)
    gates/*, readers/*, binary_readers/*, binary_writers/*, cli/*, writers/*, hooks/*, senders/*,
    converters/*, rewriters/*, watchers/*, dispatchers/*, interceptors/*, daemons/*
    — depend on core + capability, NEVER on other binaries
```

No circular dependencies. Ever. A core crate performing I/O is misplaced. A binary importing from another binary means a core crate is missing.

---

## Naming

### Binary Names: Verb Prefix

| Prefix | Directory | What it does |
|--------|-----------|--------------|
| `append_` | `writers/` | Accepts data on stdin, appends to a file with fsync |
| `write_` | `writers/` | Accepts data on stdin, writes a new file atomically |
| `store_` | `writers/` | Closes an open append file as one stored session: copy, verify, index, truncate |
| `check_` | `cli/` | Validates a file against a schema, exits 0/1/2 |
| `convert_` | `converters/` | Reads one format on stdin, writes another on stdout |
| `gate_` | `gates/` | PyO3 module: validates at a pipeline stage boundary |
| `read_` | `readers/` | PyO3 module: reads one file format, validates against a schema named at call time, returns the gate contract (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces) |
| `read_{format}_verify_` | `binary_readers/` | PyO3 module: reads one binary file format, verifies it against the format and the shape the caller states, returns the binary reader contract — decoded content as `payload`, its description as `data` (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces) |
| `write_bytes_verify_` | `binary_writers/` | PyO3 module: takes bytes, verifies them against one binary format and the shape the caller states, writes the file atomically, returns the gate contract (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces) |
| `hook_` | `hooks/` | Intercepts an LLM tool call for security/quality |
| `send_` | `senders/` | Fire-and-forget datagram to hlidskjalf socket |
| `rewrite_` | `rewriters/` | Reads JSON on stdin, modifies content, writes JSON on stdout |
| `split_` | `dispatchers/` | Splits input into batches for parallel processing |
| `watch_` | `watchers/` | Monitors files for changes and emits datagrams |
| `intercept_` | `interceptors/` | Intercepts and transforms live traffic streams |
| `record_` | `daemons/` | Long-running background process that records data |

**Specialist tools** (`saga`, `syn`) are proper nouns without verb prefix. Package names: `saga_cli`, `syn_cli`. Binary names: `saga`, `syn`. These are the only exceptions.

**Name structure:** `{verb}_{domain}_{specifics}` — e.g., `append_truth_qc_report_record`, `hook_pre_llm_bash`, `check_universal_render`.

### Crate Names

**Core:** `{domain}_core` — signals "pure library, safe to depend on from anywhere."

**Capability:** Descriptive name, no `_core` suffix. Examples: `write_engine`, `hook_io`, `datagram_io`, `saga_runner`.

**Gates:** `gate_{stage}_{direction}` — stage matches schema name with underscores, direction is `input` or `output`. A gate is custom: it embeds one schema at compile time, so it carries that schema's name.

**Readers:** `read_{format}_validate_json` — four slots: `read` is the direction, `{format}` the input format, `validate` says a schema is a parameter (`read(file_path, schema_name, schema_dir)`), `json` the output format. A reader is general: any schema, any caller. Self-describing formats drop the `validate` slot (`read_tomlx_json`, `read_tomlx_json_schema`). Thin wrappers over `reader_io`.

**Binary readers:** `read_{format}_verify_{content}` — the readers' four slots with two changes of word: `verify` in place of `validate`, because the check is against the format's own standard plus the shape the caller states, and `{content}` names what comes back, since it is typed content and no longer JSON (`read_png_verify_heights` returns heights). The same strictness as the binary writers applies on the way in: a file carrying anything beyond its stated shape is refused. Thin wrappers over `binary_reader_io`.

**Binary writers:** `write_bytes_verify_{format}` — four slots, mirroring the readers: `write` is the direction, `bytes` what the caller hands over, `verify` says the check is against the format's own standard plus the shape the caller states (`validate` stays reserved for schemas), `{format}` the file format written. A binary writer is general: any caller, but every call states the exact shape it expects, and a file carrying anything beyond that shape — trailing bytes, free-form chunks or extensions — is refused. A writer with its shape compiled in is named for what it writes and built on the same engine. Thin wrappers over `binary_writer_io`.

### Identity Rule

**Directory name = package name = binary name.** All three must match exactly. If the directory is `writers/append_raw_jsonl/`, the `Cargo.toml` name is `append_raw_jsonl`, and the `[[bin]]` name is `append_raw_jsonl`. No exceptions except specialist tools.

### Schema Files

`schemas/agents/{stage-name}.schema.json` and `schemas/tools/{tool-name}.schema.json`. Kebab-case for filenames — the only place kebab-case appears in nornir. Maps to underscores in code.

### What Never Appears

Hyphens in crate/binary names. Generic words (`utils`, `helpers`, `common`). Version suffixes (`_v2`, `_new`). Abbreviations beyond universally understood ones (`io` is OK, `fmt` is not).

---

## Binary Structure

Every Tier 3 binary follows this pattern:

```rust
fn parse_args() -> Result<Config, String> { ... }

fn run(config: &Config) -> Result<String, String> { ... }

fn main() {
    let config = match parse_args() {
        Ok(c) => c,
        Err(e) => { eprintln!("{e}"); process::exit(2); }
    };
    match run(&config) {
        Ok(msg) => println!("{msg}"),
        Err(e) => { eprintln!("{e}"); process::exit(1); }
    }
}
```

**Rules:**

1. `process::exit()` appears ONLY in `main()`. Helpers return `Result`.
2. Exit codes: 0 = success, 1 = operational failure, 2 = usage error.
3. Complex binaries (4+ flags) use clap derive. Simple ones stay manual.
4. `.unwrap()` and `.expect()` are prohibited in production code (gleipnir enforces). Exemptions: `static`/`LazyLock` initializers, `const` contexts, test code.

---

## Composition Patterns

### Writers (declarative on write_engine)

~20-line binaries. Define config, call `write_engine::run()`:

```rust
use schemas_embedded::MY_SCHEMA;
use write_engine::{OutputFormat, OutputPath, Stamps, WriteFrequency, WriterConfig};

fn main() {
    let base = write_engine::ai_home();
    match write_engine::run(&WriterConfig {
        name: "append_my_thing",
        schema: &MY_SCHEMA,
        schema_source_path: base.join("smidja/nornir/schemas/tools/my-thing.schema.json").display().to_string(),
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Record,
        output: OutputPath::DirectoryName { dir: base.join("path/to/output"), ext: "jsonl" },
        batch_size: None,
        stamps: Stamps::NONE,
    }) {
        Ok(msg) => println!("{msg}"),
        Err(msg) => { eprintln!("{msg}"); std::process::exit(1); }
    }
}
```

**Stamps** are fields the writer sets and the author may not: `sequence` (line count + 1),
`timestamp` (the clock, ISO 8601 Zulu), `session` (the file's session id, minted from the
first record's timestamp in basic ISO form and copied thereafter). A record that supplies a
stamped field is refused. The rules are pure in `stamp_core`; `write_engine` gathers the
context from the file and the clock.

**Stores** (`store_` on `store_engine`) are the counterpart: close the open file as one
session — copy to `sessions/<id>.jsonl`, verify SHA-256, append a validated index record
with the closing note from the command line, truncate the open file in place.

To add a writer or a store: create the schema in `schemas/tools/`, add it to
`schemas_embedded/src/lib.rs`, run `generate_writer.py` (`--kind store` for a store; it
prints the remaining steps), add the crate to the workspace `Cargo.toml` and to
`deploy_categories.toml` under `[writers]`, and for a writer add the printed entry to
`tool_registry.toml`.

### Hooks (pure decision functions on hook_io)

```rust
fn main() -> ExitCode {
    hook_io::run_pre_hook(decide)
}

fn decide(input: &HookInput) -> HookDecision {
    // Pure logic — no I/O, no process::exit
    HookDecision::Allow
}
```

Shared rule parsing lives in `hook_io::rules`. Use `parse_toml_table()` and `parse_rule_array()` — do not reimplement TOML parsing in hook binaries.

### Senders (thin wrappers on datagram_io)

~25-line binaries that construct a datagram and call `datagram_io::emit()`. The socket path `/tmp/ai_logger.sock` lives ONLY in datagram_io — never hardcode it.

---

## Dependency Lookup

Before writing logic, check if it already exists:

| Need | Use |
|------|-----|
| JSON/YAML/TOML/TOON conversion | `format_core` |
| Schema validation | `schema_core` + `schemas_embedded` |
| Error types with educational formatting | `error_core` |
| Path field extraction from schema | `path_core` |
| Path existence checks | `path_verify_io` |
| Atomic file writes with schema validation | `write_engine` |
| Atomic file writes of bytes | `write_engine::write_bytes_atomic` |
| JSONL append with fsync | `write_engine::append_line_fsync` |
| Binary format verification (PNG, quantized-mesh) | `binary_format_core` |
| Binary writer I/O: verify bytes, then write atomically | `binary_writer_io` |
| Binary reader I/O: read a file, verify, decode | `binary_reader_io` |
| Heights from elevation tiles, bad-pixel repair | `elevation_core` |
| QA report generation | `saga_runner` |
| QA report grouping/formatting | `report_render_core` |
| Jq filter compilation + evaluation | `syn_core` |
| AST guardrails | `gleipnir_core` |
| Line-level diffing | `diff_core` |
| Hook stdin/stdout/decision contract | `hook_io` |
| Hook rule parsing from TOML | `hook_io::rules` |
| Gate I/O orchestration (compile-time schema) | `gate_io` |
| Reader I/O: format→JSON + schema loaded by name at call time | `reader_io` |
| Datagram emission to Hlidskjalf | `datagram_io` |
| Schema constants | `schemas_embedded` |
| File-arg diagnostic CLI contract | `io_check` |
| Directory walking with skip logic | `saga_runner::walk_files` |
| Path traversal / filename validation | `path_core::validate_path_segment` |
| Runtime $HOME/ai resolution | `write_engine::ai_home` |

---

## Deploying

Do NOT use bare `cargo build --release` — it skips symlinks, verification, and dependency coherence. Use `nornir_deploy` (available in `$PATH` via `~/ai/tools/scripts/`). See `MUST_READ_BEFORE_BUILDING.md` for rationale.

```bash
nornir_deploy --all                  # rebuild + deploy everything
nornir_deploy --non-pyo3             # all cargo crates, skip maturin gates
nornir_deploy --build gates          # single category
nornir_deploy --build readers        # the reader modules (maturin, like gates)
nornir_deploy --build binary_readers,binary_writers  # the binary reader and writer modules (maturin, like gates)
nornir_deploy --build hooks,writers  # multiple categories
nornir_deploy --list                 # show categories and crate counts
```

Categories and crate lists are defined in `deploy_categories.toml`.

After schema changes (draupnir regeneration): `nornir_deploy --build gates`

---

## Adding a New Crate

1. Choose the correct category directory (see verb prefix table)
2. Name following conventions: `{verb}_{domain}` for binaries, `{domain}_core` for core libs
3. Create directory: `{category}/{crate_name}/`
4. Create `Cargo.toml` with `name` matching directory name, `{ workspace = true }` for shared deps
5. Create `src/main.rs` (binary) or `src/lib.rs` (library)
6. Add to workspace `Cargo.toml` members list
7. Add to the appropriate category in `deploy_categories.toml`
8. Run `cargo check` to verify, then `nornir_deploy --build <category>`

---

## Testing

- All tests must pass before committing. Run `cargo test -p {crate}` per crate.
- PyO3 crates (`cdylib+rlib`) fail to link without Python headers — use `nornir_deploy --build <category>` for those.
- Full workspace (excluding PyO3 crates): `cargo test --workspace $(for d in gates/*/ readers/*/; do echo "--exclude $(basename $d)"; done) --exclude intercept_io --exclude traffic_interceptor_rewriter`
- Security hooks require dual-direction tests: malicious input IS detected, benign input is NOT flagged.
- Tests verify specification, not implementation. "Does `severity_rank("error")` return 2?" not "Does the internal BTreeMap have 4 entries?"
- Pure logic must be testable: takes parameters, returns `Result`, no side effects.

---

## Schema-First Data Validation

All structured data is validated against JSON Schema files in `schemas/`. Schemas are embedded at compile time via `include_str!()` in `schemas_embedded`. Procedural shape-checking (Rust match arms checking field names/types) is not schema validation.

To add a new data shape:
1. Create `schemas/tools/my-thing.schema.json`
2. Add `pub static MY_THING: EmbeddedValidator = ...` to `schemas_embedded/src/lib.rs`
3. Reference `MY_THING` in your binary

---

## Workspace Dependencies

All external crates declared in root `Cargo.toml` `[workspace.dependencies]`. Reference as `{ workspace = true }` in crate Cargo.toml. Never pin a version in a crate's own Cargo.toml.

Current: serde 1.0, serde_json 1.0, serde_yaml 0.9, toml 0.8, toon-format 0.4, jsonschema 0.29, thiserror 2.0, pyo3 0.22, regex 1.11, jaq-interpret 1.5, jaq-parse 1.0, tree-sitter 0.25, tree-sitter-css 0.25, tree-sitter-html 0.23, tree-sitter-python 0.25, tree-sitter-rust 0.24, tree-sitter-typescript 0.23, sha2 0.10, flate2 1.1, png 0.18, clap 4 (derive), libc 0.2, reqwest 0.12 (blocking), dotenvy 0.15, rodio 0.19.
