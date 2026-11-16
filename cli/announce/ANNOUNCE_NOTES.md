# announce — Operational Notes

`announce` (`cli/announce`, core: `announce_core`; deployed to `~/ai/tools/bin/announce` via
`nornir_deploy`) is nornir's TTS tool.

## Backends
- **kokoro** (default) — local TTS at `http://127.0.0.1:8880/v1/audio/speech` (OpenAI-compatible),
  zero credits, always caches (local = free).
- **elevenlabs** — cloud; needs `ELEVENLABS_API_KEY`; uses hit-count cache gating.

Voices are named in `announce.toml`'s `[voices.*]` tables; a name with no table is passed through
as a backend voice id.

## Config split
- Voice config: `~/ai/control/voice/announce.toml`
- Secrets: `~/ai/voice/.env` (kept OUT of `control/`)
- Audio cache: `~/ai/audio/{voice_id}/{lang_code}/{slug}_{hash4}.mp3`

## Control directory `~/ai/control/`
- `voice/` — `announce.toml`, `hits.toml`, `lookups/`, global `SILENT.lock` / `QUIET.lock` /
  `RECORDING.lock`
- `workspaces/` — `registry.db` plus per-workspace dirs holding `VOICE.lock` / `SILENT.lock` /
  `QUIET.lock`

## Workspace registry (`workspace_registry` crate)
SQLite + WAL at `~/ai/control/workspaces/registry.db`. Functions: `register_workspace(name,path)`,
`register_session(session_id,workspace)`, `resolve_workspace_from_path` (longest-prefix),
`workspace_from_session`, `workspace_control_dir(name)`. Populated by `hook_session_start_orient`
on every SessionStart.

## Locks — who checks which
Each lock is checked global first (`voice/`), then for the workspace that `--source` resolves to.

- **`SILENT.lock`** — all speech off. Checked by `announce` itself (`cli/announce/src/main.rs:104-112`)
  and by `hook_io` before its spoken alerts.
- **`QUIET.lock`** — Claude Code's response readout off; everything else still speaks. Checked by
  `hook_stop_llm_tts`, not by `announce` (`hooks/hook_stop_llm_tts/src/main.rs:73-80`).
- **`RECORDING.lock`** — global only, created by `relay_mic_to_voice` while the mic is open. While it
  exists, `announce` queues every message except `--severity critical` to `~/ai/voice/queue.jsonl`;
  `relay_mic_to_voice` replays the queue when the mic closes.

## Settings priority
`resolve_settings` (`core/announce_core/src/lib.rs`) merges CLI > `VOICE.lock` > `--profile` >
default for voice and language; speed adds the voice's own speed before the default. The model
comes from the profile or the default only. Voice = workspace identity (`VOICE.lock`), not severity.
`--severity` affects only the `RECORDING.lock` bypass.

## macOS CoreAudio + fork
Never `fork()` with rodio; self-exec via `Command::new(exe).arg("--play-file").spawn()`. `hush`
stops playback by matching that self-exec's arguments with `pkill`, so renaming them breaks `hush`.

## Speed and ElevenLabs specifics
Speed is clamped 0.7–1.2 for every backend. ElevenLabs: auth header `xi-api-key`; PCM stream
`…/stream?output_format=pcm_24000`, MP3 cache `…?output_format=mp3_22050_32`.
