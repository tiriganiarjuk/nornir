//! SessionEnd hook: hand the assembled system prompt across a /clear.
//!
//! /clear ends the current session and starts a new conversation under a
//! fresh session_id, and the new session's SessionStart event carries no
//! reference to the old one. This hook is the only place the old session_id
//! is still known: it fires on SessionEnd with reason "clear" and copies the
//! dying session's SYSTEM_PROMPT.xml into the workspace-level handoff slot
//! (`workspace_registry::clear_handoff_path`), where the new session's
//! SessionStart hooks (orient, inject) pick it up.
//!
//! Copies rather than moves: the ended session stays resumable via /resume,
//! so its own session-keyed prompt must remain in place.
//!
//! Usage (in ~/.claude/settings.json, SessionEnd, matcher "clear"):
//!     hook_session_end_handoff

use std::process::ExitCode;

use session_event_io::SessionEnd;

fn main() -> ExitCode {
    session_event_io::run_event_hook(hand_off)
}

fn hand_off(event: SessionEnd) {
    // Belt and braces: settings.json matches on "clear", but the handoff must
    // never fire for logout/exit — those end the process, nothing consumes it.
    if event.reason != "clear" {
        return;
    }

    let workspace = std::path::Path::new(&event.cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let prompt = workspace_registry::workspace_control_dir(&workspace)
        .join(&event.session_id)
        .join("SYSTEM_PROMPT.xml");

    if prompt.exists() {
        let _ = std::fs::copy(&prompt, workspace_registry::clear_handoff_path(&workspace));
    }
}
