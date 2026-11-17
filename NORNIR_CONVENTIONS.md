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
    gates/*, readers/*, text_writers/*, binary_readers/*, binary_writers/*, presence/*, cli/*, writers/*, hooks/*, senders/*,
    converters/*, rewriters/*, watchers/*, dispatchers/*, interceptors/*, daemons/*
    — depend on core + capability, NEVER on other binaries
```

No circular dependencies. Ever. A core crate performing I/O is misplaced. A binary importing from another binary means a core crate is missing.

---

## Naming

### Binary Names: Verb Prefix

A built tool is one crate with two faces: a CLI binary always, and a PyO3 module where Python needs it (§ Tool Structure). The table names what each kind of tool does; the face is not part of the kind. A gate is a generated variant whose use is a Python pipeline, so every gate has its PyO3 face; an input gate has its CLI face too.

| Prefix | Directory | What it does |
|--------|-----------|--------------|
| `append_` | `writers/` | Accepts data on stdin, appends to a file with fsync |
| `write_` | `writers/` | Accepts data on stdin, writes a new file atomically |
| `store_` | `writers/` | Closes an open append file as one stored session: copy, verify, index, truncate |
| `convert_` | `converters/` | Reads one format on stdin, writes another on stdout |
| `gate_` | `gates/` | Validates at a pipeline stage boundary against an embedded schema; generated from `gates/gate_recipes.toml` by `generate_gates.py`. An input gate also runs from a shell: `gate_{stage}_input <file>` |
| `read_` | `readers/` | Reads one file format, or one directory tree, validates against a schema — named at call time, or the reader's own where its source carries its shape — and returns the validated data |
| `write_json_validate_` | `text_writers/` | Takes JSON text, validates it against a schema named at call time, writes one file format atomically |
| `read_{format}_verify_` | `binary_readers/` | Reads one binary file format, verifies it against the format and the shape the caller states, returns the decoded content and its description |
| `write_bytes_verify_` | `binary_writers/` | Takes bytes, verifies them against one binary format and the shape the caller states, writes the file atomically |
| `create_` | `presence/` | Makes a thing exist where its existence is the point and its content is not: a flag (an empty file) — `create_flag_{name}` |
| `remove_` | `presence/` | Takes such a thing away, only while it is still what `create_` made: a flag only while it is empty — `remove_flag_{name}` |
| `hook_` | `hooks/` | Runs on a Claude Code hook event: gates or assesses a tool call, or acts on a session event (start, end, stop, notification, compaction) |
| `send_` | `senders/` | Fire-and-forget datagram to hlidskjalf socket |
| `rewrite_` | `rewriters/` | Reads JSON on stdin, modifies content, writes JSON on stdout |
| `split_` | `dispatchers/` | Splits input into batches for parallel processing |
| `watch_` | `watchers/` | Monitors files for changes and emits datagrams |
| `intercept_` | `interceptors/` | Intercepts and transforms live traffic streams |
| `record_` | `daemons/` | Long-running background process that records data |

**Specialist tools** (`saga`, `syn`) are proper nouns without verb prefix. Package names: `saga_cli`, `syn_cli` — the suffix is required because the bare package name `syn` is the well-known Rust parsing crate. Binary names: `saga`, `syn`. These are the only exceptions.

**Name structure:** `{verb}_{domain}_{specifics}` — e.g., `append_truth_qc_report_record`, `hook_pre_llm_bash`, `gate_universal_render_input`.

**A generated variant's name grows with its constraints:** the longer the name, the more is fixed in. `append_embedding_normalize_batch_20` fixes its schema and its destination, and the batch size besides (`HOW_A_NORNIR_TOOL_IS_MADE.md` § A generated variant adds constraints, never logic).

### Crate Names

**Core:** `{domain}_core` — signals "pure library, safe to depend on from anywhere."

**Capability:** Descriptive name, no `_core` suffix. Examples: `write_engine`, `hook_io`, `datagram_io`, `saga_runner`.

**Gates:** `gate_{stage}_{direction}` — stage matches schema name with underscores, direction is `input` or `output`. A gate is custom: it embeds one schema at compile time, so it carries that schema's name.

**Readers:** `read_{format}_validate_json` — four slots: `read` is the direction, `{format}` the input format, `validate` says a schema is a parameter (`read(file_path, schema_name, schema_dir)`), `json` the output format. A reader is general: any schema, any caller. A file that is valid in its format and holds a value JSON cannot carry — a TOML date-time, an infinity or a not-a-number in TOML or YAML — is refused as `file_value_not_json`, never converted. The markdown reader takes fences ended by a line feed or by a carriage return and a line feed; the XML reader takes one root element and refuses content after it. `read_tomlx_validate_json` validates the data after the file's annotations have converted it, so its schema describes values in their target units. `read_tomlx_json` and `read_tomlx_json_schema` have no `validate` slot: they return the converted data, the latter with the schema derived from the file's annotations. Thin wrappers over `reader_io`. `read_directory_json` reads a directory tree the same way — its listing's shape is its own, `schemas/tools/directory-listing.schema.json` — and is a thin wrapper over `directory_read_io`, the one directory walker. It takes only its root: the whole tree, hidden entries left out, no hashes, and every path it cannot read a `read_error`. `read_directory_permitted_json` is its sibling: the tree as far as the caller is permitted to see it — a directory the caller may not look into listed with `"permitted": false` and nothing beneath it, every path gone mid-walk named in `"vanished"` — under `schemas/tools/directory-permitted-listing.schema.json`. The engine carries every other choice (depth, hidden entries, hashes, pruning); a tool that makes one of them is a tool of its own, named for it.

**Text writers:** `write_json_validate_{format}` — the readers' four slots in the other direction: `write` is the direction, `json` what the caller hands over, `validate` says a schema is a parameter (`write(json_data, file_path, schema_name, schema_dir)`), `{format}` the file format written. A text writer is general: any schema, any caller — and it writes nothing without one. The JSON it takes is the JSON the matching reader returns, so a written file reads back as the same data: the markdown writer takes `{"frontmatter": {...}, "body": "..."}`, the XML writer one root key in the `@attr` / `#text` convention. YAML — the YAML writer's file and the markdown writer's frontmatter — is written in one fixed style, by `format_core::yaml_emit`: block mappings and sequences, a sequence at its key's column; a string plain only when every mainstream YAML reader (YAML 1.1, YAML 1.2 core, js-yaml) reads the plain form as that string, double-quoted otherwise, and a literal block when it holds line breaks; a float with a `.` and a signed exponent. An integer beyond ±2^53 is refused as `data_not_expressible`, because a JavaScript reader would round it. A YAML document in this style reads back as its data and is written again as the same bytes. What a format cannot carry is refused as `data_not_expressible`, never converted: TOML an integer beyond 64 signed bits; TOON and XML whatever would read back as different data (`format_core`'s TOON and XML serializers read their own output and name the path of the first value that changed, for every caller — a number or a boolean in XML, which has no types; an integer or float TOON would alter); XML a name or a character XML 1.0 does not allow. A Markdown document is written to the last byte of its body; every other format ends with a line break. Thin wrappers over `text_writer_io`, which validates through `reader_io`, so a schema is loaded and reported the same way in both directions. `write_json_validate_directory` writes a whole generated directory in one call — its files validated and rendered the same way, swapped in whole, byte-identical files kept with their time — and is a thin wrapper over `directory_write_io`; it returns the directory writer's extension of the contract (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces). Every directory it writes carries its marker, `.generated_directory.json`, holding `{"written_by": "write_json_validate_directory"}`: the writer's proof of ownership. A folder without the marker was not written by it and is never replaced.

**Binary readers:** `read_{format}_verify_{content}` — the readers' four slots with two changes of word: `verify` in place of `validate`, because the check is against the format's own standard plus the shape the caller states, and `{content}` names what comes back, since it is typed content and no longer JSON (`read_png_verify_heights` returns heights). The same strictness as the binary writers applies on the way in: a file carrying anything beyond its stated shape is refused. Thin wrappers over `binary_reader_io`.

**Binary writers:** `write_bytes_verify_{format}` — four slots, mirroring the readers: `write` is the direction, `bytes` what the caller hands over, `verify` says the check is against the format's own standard plus the shape the caller states (`validate` stays reserved for schemas), `{format}` the file format written. A binary writer is general: any caller, but every call states the exact shape it expects, and a file carrying anything beyond that shape — trailing bytes, free-form chunks or extensions — is refused. A writer with its shape compiled in is named for what it writes and built on the same engine. Thin wrappers over `binary_writer_io`.

### Identity Rule

**Directory name = package name = binary name = PyO3 module name.** All must match exactly. If the directory is `readers/read_toml_validate_json/`, the `Cargo.toml` name is `read_toml_validate_json`, the `[[bin]]` name is `read_toml_validate_json`, and Python imports `read_toml_validate_json`. One tool has one name, whichever face is called. No exceptions except specialist tools.

### Schema Files

`schemas/agents/{stage-name}.schema.json` and `schemas/tools/{tool-name}.schema.json`. Kebab-case for filenames — the only place kebab-case appears in nornir. Maps to underscores in code.

### What Never Appears

Hyphens in crate/binary names. Generic words (`utils`, `helpers`, `common`). Version suffixes (`_v2`, `_new`). Abbreviations beyond universally understood ones (`io` is OK, `fmt` is not).

### Module Contents: Grouped by Action

Functions belong together by what they do. Group by action — `schema_build`, `field_annotate`, `conditional_build`, `zone_collect` — and ask of each function "what does this DO?": two functions that both build JSON Schema nodes belong together, though one serves atoms and the other sections. A module that builds, records metadata and dispatches is three modules, one per action.

---

## Binary Structure

Every Tier 3 binary follows this pattern, which is the CLI contract (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces):

```rust
fn parse_args() -> Result<Config, String> { ... }

// The engine's typed error: error_type() names the failure class.
fn run(config: &Config) -> Result<String, EngineError> { ... }

fn main() {
    let config = match parse_args() {
        Ok(c) => c,
        Err(usage) => { eprintln!("{usage}"); process::exit(2); }
    };
    match run(&config) {
        Ok(data) => print!("{data}"),
        Err(e) => { eprintln!("{}\n{e}", e.error_type()); process::exit(1); }
    }
}
```

**Rules:**

1. `process::exit()` appears ONLY in `main()`. Helpers return `Result`.
2. Exit codes: 0 = success, 1 = refusal or failure, 2 = wrong arguments.
3. A tool takes only what it operates on; a choice the caller would otherwise make is fixed into the tool or becomes a separate tool (`HOW_A_NORNIR_TOOL_IS_MADE.md` § A different behaviour is a different tool). Arguments are parsed with clap derive where there are named options.
4. `.unwrap()` and `.expect()` are prohibited in production code (gleipnir enforces). Exemptions: `static`/`LazyLock` initializers, `const` contexts, test code.

---

## Tool Structure — one tool, two faces

A built tool is one crate. The tool is a function in the crate's library, defined once, returning `Result<Option<String>, E>` — `Some(data)` for a tool that hands data back, `None` for one whose effect is the write. Each face hands that result, with the engine's `error_type`, to the adapters in `tool_face_io`, which present it under the contracts; a face holds no logic of its own. The worked example is `readers/read_toml_validate_json/`.

```
readers/read_toml_validate_json/
  Cargo.toml              [lib] cdylib + rlib; [[bin]]; feature "python"
  src/lib.rs              pub fn read(...) -> Result<Option<String>, ReaderError>   — the tool
                          #[cfg(feature = "python")] the PyO3 face: tool_face_io::py_contract
  src/main.rs             the CLI face: tool_face_io::positional_args, cli_report, cli_usage
  tests/cli_contract.rs   the CLI face through the three outcomes of the CLI contract
```

```toml
[features]
python = ["dep:pyo3", "tool_face_io/python"]

[dependencies]
pyo3 = { workspace = true, optional = true }
tool_face_io = { path = "../../capability/tool_face_io" }
```

The PyO3 face sits behind the `python` feature because the workspace builds PyO3 as an extension module: its Python symbols are supplied by the interpreter that loads it, so a program containing PyO3 code cannot link on its own. With the feature off, the crate is an ordinary Rust crate — the CLI binary links, and `cargo test` runs. `nornir_deploy` builds the PyO3 face with the feature on and the CLI face without it, reading which faces a crate has from its own `Cargo.toml`.

Both faces carry the same information under their contracts (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces): the CLI for a shell and a pipe, the PyO3 module for a Python program that branches on `error.type`.

---

## Composition Patterns

### Writers (declarative on write_engine)

~20-line binaries. Define config, exit with `write_engine::run_cli()`, which presents the outcome under the CLI contract:

```rust
use schemas_embedded::MY_SCHEMA;
use write_engine::{OutputFormat, OutputPath, Stamps, WriteFrequency, WriterConfig};

fn main() {
    let base = write_engine::ai_home();
    std::process::exit(write_engine::run_cli(&WriterConfig {
        name: "append_my_thing",
        schema: &MY_SCHEMA,
        schema_source_path: base.join("smidja/nornir/schemas/tools/my-thing.schema.json").display().to_string(),
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Record,
        output: OutputPath::DirectoryName { dir: base.join("path/to/output"), ext: "jsonl" },
        batch_size: None,
        stamps: Stamps::NONE,
    }));
}
```

**Stamps** are fields the writer sets and the author may not: `sequence` (line count + 1),
`timestamp` (the clock, ISO 8601 Zulu), `session` (the file's session id, minted from the
first record's timestamp in basic ISO form and copied thereafter). A record that supplies a
stamped field is refused. The rules are pure in `stamp_core`; `write_engine` gathers the
context from the file and the clock.

**The receipt** is what a writer prints on stdout once the write is on disk:
`{"written": 1}`, and for a stamping writer
`{"written": 3, "last_record_stamps": {"exchange": 43, "ts": "…", "session": "…"}}` — the
last record's stamps under the writer's own field names. `write_engine` builds it and
checks it against `schemas/tools/write-receipt.schema.json` before writing; it names no
path (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces).

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

A hook that acts on a session event — SessionStart, SessionEnd, Stop, Notification, PostCompact —
reads it through `session_event_io`: the payload validated against the event's schema in
`schemas/tools/`, its session id one plain path segment, then typed.

```rust
fn main() -> ExitCode {
    session_event_io::run_event_hook(|event: SessionStart| { ... })
}
```

An event the hook cannot read is not acted on: the failure goes to stderr under the CLI
contract, the hook exits 1, and a payload that arrived and was refused is raised as an alert.

Shared rule parsing lives in `hook_io::rules`. Use `parse_toml_table()` and `parse_rule_array()` — do not reimplement TOML parsing in hook binaries. A hook's reply is built with `hook_io::response`: one response type per Claude Code hook event, exposing only what that event accepts, so an invalid reply does not compile. The module's own documentation maps each term to the wire format.

### Senders (thin wrappers on datagram_io)

~25-line binaries that construct a datagram and call `datagram_io::emit()`. The socket path `/tmp/ai_logger.sock` lives ONLY in datagram_io — never hardcode it.

---

## Dependency Lookup

Before writing logic, check if it already exists:

| Need | Use |
|------|-----|
| JSON/YAML/TOML/TOON conversion | `format_core` |
| tomlx annotation processing: units, paths, a schema derived from the annotations | `format_core::tomlx` — the format is `core/format_core/TOMLX_SPEC.md` |
| Schema validation | `schema_core` + `schemas_embedded` |
| A failure mode — its class, message and route, from the error registry | `failure_mode_core` (`failure_modes.toml`) |
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
| A tool's CLI face and PyO3 face: arguments, outcome presentation under both contracts | `tool_face_io` |
| Hook stdin/stdout/decision contract | `hook_io` |
| Hook rule parsing from TOML | `hook_io::rules` |
| A session event hook's stdin: the event validated against its schema, typed, its session id safe to join into a path | `session_event_io` |
| Gate I/O orchestration (compile-time schema) | `gate_io` |
| Reader I/O: format→JSON + schema loaded by name at call time | `reader_io` |
| Validating JSON against a schema loaded by name | `reader_io::validate_against_named_schema` |
| Text writer I/O: validate JSON against a named schema, then write atomically | `text_writer_io` |
| A generated directory replaced whole: validated files, kept identical files, removed stale ones, one report | `directory_write_io` |
| A flag — an empty file whose presence is the information — created, or removed only while empty | `presence_io` |
| Datagram emission to Hlidskjalf | `datagram_io` |
| A directory's workspace name (`@smidja:nornir`), for any tool that operates on directories | `datagram_io::workspace_from_path` |
| Schema constants | `schemas_embedded` |
| Directory listing: every entry, its kind, size and sha256, unreadable paths reported | `directory_read_io` |
| Path traversal / filename validation | `path_core::validate_path_segment` |
| The ai home, `~/ai`, of the running process | `write_engine::ai_home` |
| Where the ai home is under a home directory — the rule itself, pure | `path_core::ai_home_under` |

---

## Deploying

Do NOT use bare `cargo build --release` — it skips the copy into `~/ai/tools/bin/`, verification, and dependency coherence. Use `nornir_deploy` (available in `$PATH` via `~/ai/tools/scripts/`). See `MUST_READ_BEFORE_BUILDING.md` for rationale.

```bash
nornir_deploy --all                  # rebuild + deploy everything
nornir_deploy --non-pyo3             # all cargo crates, skip maturin gates
nornir_deploy --build gates          # single category
nornir_deploy --build readers,text_writers  # the reader and text writer modules (maturin, like gates); they share reader_io
nornir_deploy --build binary_readers,binary_writers  # the binary reader and writer modules (maturin, like gates)
nornir_deploy --build hooks,writers  # multiple categories
nornir_deploy --build session_hooks  # the hooks that act on a session event
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
- A two-faced tool tests like any crate: the feature is off by default, so its library and CLI face link. A crate whose PyO3 code is not behind the feature cannot link a test binary; `nornir_deploy --build <category>` verifies those by import.
- Full workspace: `nornir_test` (at the repo root). It leaves out the crates whose PyO3 code is not behind the feature, names them, and ends with every failed test and `TOTAL passed=N failed=N ignored=N`. `nornir_test -p {crate}` passes its arguments to `cargo test`.
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

All external crates declared in root `Cargo.toml` `[workspace.dependencies]`. Reference as `{ workspace = true }` in crate Cargo.toml. Never pin a version in a crate's own Cargo.toml. What is declared, and at which version, is read from that table.
