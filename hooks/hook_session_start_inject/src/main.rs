//! SessionStart hook: re-inject system prompt on compact, resume, or clear.
//!
//! On compaction the assembled system prompt drops out of context. This hook
//! reads it back from the session-specific path where hook_session_start_orient
//! placed it and emits it as additionalContext.
//!
//! Claude Code persists oversized hook output to a file and inlines only a ~2KB
//! preview — leaving the session running on a fraction of its instructions while
//! feeling oriented. For any payload big enough to risk that, the injection leads
//! with an integrity banner that survives inside the preview and orders the full
//! read; when the payload inlines whole, the banner's condition self-neutralizes.
//!
//! Path: ~/ai/control/workspaces/{workspace}/{session_id}/SYSTEM_PROMPT.xml

use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use session_event_io::SessionStart;

fn announce(workspace: &str, source: &str, project_dir: &str) {
    let control_voice = write_engine::ai_home().join("control/voice");
    if control_voice.join("SILENT.lock").exists() {
        return;
    }
    if workspace_registry::workspace_control_dir(workspace).join("SILENT.lock").exists() {
        return;
    }

    let announce_bin = write_engine::ai_home().join("tools/bin/announce");
    let mut cmd = Command::new(&announce_bin);
    cmd.args(["--severity", "info"]);
    cmd.args(["--source", project_dir]);
    cmd.arg(format!("{workspace}: {source} context injection"));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = cmd.spawn();
}

fn workspace_name(workspace_path: &str) -> String {
    Path::new(workspace_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Payloads above this risk Claude Code's persisted-output truncation (observed
/// inlining limit is well above 2KB but unpublished; conservative by design).
const PREVIEW_RISK_BYTES: usize = 8 * 1024;

fn integrity_banner(payload_bytes: usize, prompt_path: &Path) -> String {
    format!(
        "<injection-integrity bytes=\"{payload_bytes}\">\n\
         THIS SYSTEM-PROMPT INJECTION IS {kb} KB. If the visible text ends within \
         about 2 KB, Claude Code has persisted the full output to a file and you are \
         reading a PREVIEW — a fraction of your instructions. Operating on the preview \
         is the known failure mode: confident, oriented-feeling, and wrong. Before ANY \
         other action, Read the persisted additionalContext file (its path is printed \
         immediately above this preview) end to end. Fallback copy: {path}. Then honor \
         the bootloader in MEMORY.md: prove the read with one non-obvious constraint, \
         or state plainly that you could not read it.\n\
         </injection-integrity>\n\n",
        kb = payload_bytes / 1024,
        path = prompt_path.display(),
    )
}

/// The system prompt to put back into the session, when this start calls for it.
fn prompt_to_inject(event: &SessionStart) -> Option<String> {
    // Only re-inject on compact, resume, or clear — not startup
    match event.source.as_str() {
        "compact" | "resume" | "clear" => {}
        _ => return None,
    }

    let workspace = workspace_name(&event.cwd);
    let prompt_path = workspace_registry::workspace_control_dir(&workspace)
        .join(&event.session_id)
        .join("SYSTEM_PROMPT.xml");

    let read = |p: &Path| std::fs::read_to_string(p).ok().filter(|c| !c.is_empty());

    // /clear mints a new session_id with no session-keyed prompt yet; the old
    // session's prompt sits in the workspace-level handoff slot (written by
    // hook_session_end_handoff). orient moves the slot into the session dir
    // concurrently with us — same-event hooks are unordered — so try the
    // session path, then the slot, then the session path once more (the slot
    // vanishes atomically when orient's rename wins the race).
    let content = read(&prompt_path).or_else(|| {
        if event.source != "clear" {
            return None;
        }
        let slot = workspace_registry::clear_handoff_path(&workspace);
        if workspace_registry::clear_handoff_is_fresh(&slot) {
            read(&slot).or_else(|| read(&prompt_path))
        } else {
            read(&prompt_path)
        }
    })?;

    if content.len() > PREVIEW_RISK_BYTES {
        Some(format!("{}{content}", integrity_banner(content.len(), &prompt_path)))
    } else {
        Some(content)
    }
}

fn main() -> ExitCode {
    session_event_io::run_event_hook(|event: SessionStart| {
        if let Some(context) = prompt_to_inject(&event) {
            announce(&workspace_name(&event.cwd), &event.source, &event.cwd);

            let output = serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": context
                }
            });
            println!("{}", output);
        }
    })
}
