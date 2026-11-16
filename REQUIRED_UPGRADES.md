# REQUIRED_UPGRADES — tools to move onto newer tools

Forward-only: each entry names a tool that is to be rebuilt onto a newer, better tool, what it
moves to, and what it waits on when it cannot start yet. An entry leaves this file when the
upgrade is done; the record of it is git.

---

## yggdrasil `common_core::list_directory` → `directory_read_io`

`~/ai/smidja/yggdrasil/core/common_core` `list_directory` (used by Kvasir, Ratatoskr and
Svalinn) becomes a user of `capability/directory_read_io`, so the ecosystem walks directories
in one place.

**Waits on:** the author's ruling on where the yggdrasil build lives.
