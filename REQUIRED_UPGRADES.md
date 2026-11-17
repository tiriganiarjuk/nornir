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

## Binary readers and writers → `tool_face_io::py_contract`

`binary_readers/read_png_verify_heights` (`src/lib.rs:88-110`), `binary_writers/write_bytes_verify_png`
(`src/lib.rs:49-64`) and `binary_writers/write_bytes_verify_quantized_mesh` build the
`{"ok", "data", "error"}` dict by hand. They move onto `capability/tool_face_io::py_contract`,
which every other PyO3 tool returns through, so the result has exactly the contract's two
outcomes.

The binary reader carries `payload` beside `data`, so its move needs a `py_contract` form that
carries the payload too.

**Waits on:** the binary reader's form, for `read_png_verify_heights` only. The two writers can
move now.

## `hook_session_start_inject`'s reply → `hook_io::response::SessionStartResponse`

`hooks/hook_session_start_inject` assembles its `hookSpecificOutput.additionalContext` reply with
`serde_json::json!` (`src/main.rs:133-140`). It moves onto `SessionStartResponse`
(`capability/hook_io/src/response.rs:616`), where a reply the event does not accept does not
compile.

**Waits on:** nothing.

