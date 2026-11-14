# REQUIRED_UPGRADES — tools to move onto newer tools

Forward-only: each entry names a tool that is to be rebuilt onto a newer, better tool, what it
moves to, and what it waits on when it cannot start yet. An entry leaves this file when the
upgrade is done; the record of it is git.

---

## `saga_runner` directory walking → `directory_read_io`

`capability/saga_runner` `walk_files` and `find_files` (called by `cli/saga_cli`,
`cli/syn_cli`) move onto the one directory-walking engine, `capability/directory_read_io`.

- An unreadable directory, and an entry the system cannot report, come back as typed errors
  and are reported; the walk names what it could not read.
- Symlinks are listed as links and not followed.
- File names are matched as the system gives them.
- The skip list (hidden directories, `__pycache__`, `node_modules`, `.venv`, `venv`,
  `target`) becomes saga's own filter over the engine's entries, not part of the walk.

Expect saga to report directories it could not read on its first run after the upgrade.

**Waits on:** descent pruning in the engine, `directory_read_io`. Saga keeps hidden *files* (the
`.qa` sidecars) and never enters hidden or junk *directories*; listing with hidden entries
included and filtering afterwards would walk `.git`, `.venv` and `node_modules` whole, and fail on
anything unreadable inside them.

**The shape, approved by the author:**

- `ListOptions` gains two fields, both defaulting to "enter everything":
  - `prune_directories: Vec<String>` — directory names the walk neither lists nor enters, at any
    depth;
  - `prune_hidden_directories: bool` — the walk neither lists nor enters a directory whose name
    starts with `.`, while `include_hidden` still governs hidden *files*.
- The walk still applies no policy of its own: what to prune is always the caller's, named in
  the call.
- Saga, a Rust caller, calls the engine directly, with `include_hidden = true`,
  `prune_hidden_directories = true` and `prune_directories` = its skip list plus the caller's
  extra names — the old walk's behaviour, with every unreadable path reported instead of dropped.
- `read_directory_json`, the tool, is unchanged. An engine carries every choice its operation
  has; a tool fixes them. A shell or Python caller that needs pruning is given a named generated
  variant when it appears.

## `saga_runner` file hashing → `directory_read_io`

`hash_file` gives no hash for a file it cannot read. `directory_read_io::file_sha256` hashes one
file and names the file it could not read; saga records that failure in the report.

## `saga_runner` orphan sidecar sweep → `directory_read_io`

The sweep that removes a QA sidecar whose source is gone decides "gone" from a typed
presence read: a source that cannot be read is reported, and only a source the listing shows
absent removes its sidecar. A removal that fails is reported.

## yggdrasil `common_core::list_directory` → `directory_read_io`

`~/ai/smidja/yggdrasil/core/common_core` `list_directory` (used by Kvasir, Ratatoskr and
Svalinn) becomes a user of `capability/directory_read_io`, so the ecosystem walks directories
in one place.

**Waits on:** the author's ruling on where the yggdrasil build lives.
