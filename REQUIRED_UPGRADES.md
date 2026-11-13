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

## `saga_runner` file hashing → `directory_read_io`

`hash_file` gives no hash for a file it cannot read, and the caller cannot tell why. A
listing with `with_hashes` hashes every file it lists and names each one it could not read.

## `saga_runner` orphan sidecar sweep → `directory_read_io`

The sweep that removes a QA sidecar whose source is gone decides "gone" from a typed
presence read: a source that cannot be read is reported, and only a source the listing shows
absent removes its sidecar. A removal that fails is reported.

## yggdrasil `common_core::list_directory` → `directory_read_io`

`~/ai/smidja/yggdrasil/core/common_core` `list_directory` (used by Kvasir, Ratatoskr and
Svalinn) becomes a user of `capability/directory_read_io`, so the ecosystem walks directories
in one place.

**Waits on:** the author's ruling on where the yggdrasil build lives.
