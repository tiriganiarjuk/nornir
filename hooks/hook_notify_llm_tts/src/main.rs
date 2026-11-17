//! Notification hook: context-aware TTS for Claude Code notifications.
//!
//! Speaks workspace-specific messages based on notification type:
//!   permission_prompt → "{workspace} needs approval"
//!   idle_prompt       → "{workspace} idle"
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_notify_llm_tts --project-dir $CLAUDE_PROJECT_DIR

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use clap::Parser;
use session_event_io::Notification;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "hook_notify_llm_tts", about = "Notification hook: TTS alerts")]
struct Cli {
    /// Session project directory (from $CLAUDE_PROJECT_DIR)
    #[arg(long)]
    project_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let args = Cli::parse();
    session_event_io::run_event_hook(|event: Notification| {
        let project_dir = session_event_io::project_dir(args.project_dir.as_deref(), &event.cwd);
        speak(&event.notification_type, &project_dir);
    })
}

fn speak(notification_type: &str, project_dir: &Path) {
    // Extract workspace name from project dir path
    let workspace = project_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".to_string());
    let project_dir = project_dir.display().to_string();

    // Map notification_type to severity + message
    let (severity, message) = match notification_type {
        "permission_prompt" => ("notify", format!("{workspace} needs approval")),
        "idle_prompt" => ("info", format!("{workspace} idle")),
        _ => return,
    };

    // SILENT.lock: global first, then per-workspace
    let control_voice = write_engine::ai_home().join("control/voice");
    if control_voice.join("SILENT.lock").exists() {
        return;
    }
    if let Some(ref ws) = workspace_registry::resolve_workspace_from_path(&project_dir).ok().flatten() {
        if workspace_registry::workspace_control_dir(ws).join("SILENT.lock").exists() {
            return;
        }
    }

    // Spawn announce in background — do not wait
    let announce = write_engine::ai_home().join("tools/bin/announce");
    let mut cmd = Command::new(&announce);
    cmd.args(["--severity", severity]);
    cmd.args(["--source", &project_dir]);
    cmd.arg(&message);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = cmd.spawn();
}
