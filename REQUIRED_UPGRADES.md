# REQUIRED_UPGRADES — tools to move onto newer tools

Forward-only: each entry names a tool that is to be rebuilt onto a newer, better tool, what it
moves to, and what it waits on when it cannot start yet. An entry leaves this file when the
upgrade is done; the record of it is git.

---

## Hand-rolled file writes → `write_engine`

These tools write files with `File::create`, `OpenOptions` or `fs::write` and move onto
`capability/write_engine`: an atomic write for a whole file, `append_line_fsync` for an append.

- `dispatchers/split_jsonl_batches` `write_batch` (`src/main.rs:137-150`) — fsynced but not atomic.
- `watchers/watch_and_diff_exchange_intercepts` `open_transcript` / `append_transcript`
  (`src/main.rs:330-358`) — no fsync, and write errors are dropped.
- `rewriters/rewrite_compaction_summary` `write_debug_snapshot` (`src/main.rs:66`).
- `daemons/bifrost_proxy` `save_unparseable_request` (`src/main.rs:515-530`).
- `interceptors/traffic_interceptor_rewriter` `append_raw` (`src/lib.rs:35-51`) — flushes only;
  the same file's other writer, `bifrost_proxy`, already fsyncs through `write_engine`.
- `cli/announce` `save_hits` (`src/main.rs:338-342`) — non-atomic, so an interrupted write
  loses every hit count.

A write that fails is reported, never dropped.

**Waits on:** nothing.

## `relay_mic_to_voice`'s `RECORDING.lock` → `presence_io`

`daemons/relay_mic_to_voice` `create_recording_lock` (`src/main.rs:172`) makes the lock with
`File::create` and discards the error. Making a flag exist, and removing it, is `presence_io`'s
operation: a flag already there is success, a flag already gone is success, and a failure carries
its class.

**Waits on:** nothing.

## `cc_launch` directory walking → `directory_read_io`

`cli/cc_launch/src/library.rs` (`:120, 158, 194, 259`) walks the fragment library with
`fs::read_dir`. It moves onto the one directory engine, `capability/directory_read_io`, called
directly as a library.

- An unreadable directory or entry comes back as a typed error and is reported; today it is
  skipped.
- Symlinks are listed as links and not followed.
- Which files count as fragments stays cc_launch's own filter over the engine's entries.

**Waits on:** nothing.

## Binary readers and writers → one tool, two faces, on `tool_face_io`

`binary_readers/read_png_verify_heights`, `binary_writers/write_bytes_verify_png` and
`binary_writers/write_bytes_verify_quantized_mesh` are PyO3-only: `src/lib.rs` alone, `pyo3` a plain
dependency, the `{"ok", "data", "error"}` dict built by hand (`read_png_verify_heights/src/lib.rs:88-110`,
`write_bytes_verify_png/src/lib.rs:49-64`). So they have no CLI face, link no test binary, and
have no tests. The rule that a built tool is a CLI binary first is binding for them
(`HOW_A_NORNIR_TOOL_IS_MADE.md` § A built tool is a CLI binary first). Each moves to the two-faced
structure the gates, readers and text writers use.

**The target shape** — `NORNIR_CONVENTIONS.md` § Tool Structure, worked example
`readers/read_toml_validate_json/`, which every step below copies:

1. `Cargo.toml`: `pyo3` becomes optional; a feature `python = ["dep:pyo3", "tool_face_io/python"]`;
   `[lib]` is `cdylib` + `rlib`; a `[[bin]]` with the crate's name at `src/main.rs`.
2. `src/lib.rs`: the tool is one function over its engine (`binary_writer_io` / `binary_reader_io`),
   returning `Result<Option<String>, E>` where `E` is the engine's typed error with
   `error_type()`. The PyO3 face is a `#[cfg(feature = "python")]` module whose function hands
   that result to `tool_face_io::py_contract`. No dict is built by hand.
3. `src/main.rs`: the CLI face — arguments through `tool_face_io::positional_args`, wrong
   arguments through `cli_usage` (exit 2), the result through `cli_report` (exit 0 / 1, failure
   class on stderr's first line).
4. `tests/cli_contract.rs`, as the worked example has: success, refusal, wrong arguments.
5. `deploy_categories.toml` needs no new entry: the crate stays in `pyo3_crates`, and
   `nornir_deploy` builds and verifies the CLI face from the `python` feature and the `[[bin]]`
   (`MUST_READ_BEFORE_BUILDING.md` § deploy_categories.toml Reference).
6. Build with `nornir_deploy --build binary_readers,binary_writers`; run `cargo test -p <crate>`
   (the feature off, so the test binary links). Done is deployed, both faces verified.

**Waits on — a stop, by `MUST_READ_BEFORE_BUILDING.md` § Approved interfaces.** No approved
interface covers a CLI that carries bytes:

- **The writers' CLI face**: how the bytes and the stated shape arrive (bytes on stdin and the
  shape as arguments, or a path to read), and what success prints, since a write hands nothing
  back.
- **The reader, both faces**: `read_png_verify_heights` returns `payload` bytes beside `data`, and
  `tool_face_io` has no form that carries a payload — neither `py_contract` nor a CLI report.
  What the CLI face prints (the description on stdout and the payload to a path? both on stdout?)
  is a new contract.

The author decides these shapes; each becomes its own row in the Approved interfaces table.
Then the two writers convert first, and the reader after `tool_face_io` gains its payload form.
A session that reaches this entry stops here and asks.

## `hook_session_start_inject`'s reply → `hook_io::response::SessionStartResponse`

`hooks/hook_session_start_inject` assembles its `hookSpecificOutput.additionalContext` reply with
`serde_json::json!` (`src/main.rs:133-140`). It moves onto `SessionStartResponse`
(`capability/hook_io/src/response.rs:616`), where a reply the event does not accept does not
compile.

**Waits on:** nothing.

