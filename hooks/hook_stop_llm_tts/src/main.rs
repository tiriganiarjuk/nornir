//! Stop hook: TTS playback of assistant responses.
//!
//! Receives the Claude Code Stop event, filters the response text
//! to remove code blocks and markdown noise, checks QUIET.lock files,
//! then calls `announce` in the background.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_stop_llm_tts --project-dir $CLAUDE_PROJECT_DIR

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use clap::Parser;
use session_event_io::Stop;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "hook_stop_llm_tts", about = "Stop hook: TTS playback")]
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
    session_event_io::run_event_hook(|event: Stop| {
        let project_dir = session_event_io::project_dir(args.project_dir.as_deref(), &event.cwd);
        speak(&event.last_assistant_message, &project_dir);
    })
}

fn speak(last_assistant_message: &str, project_dir: &Path) {
    let message = last_assistant_message.trim();
    if message.is_empty() {
        return;
    }
    let project_dir = project_dir.display().to_string();

    // Resolve workspace from project dir
    let workspace = workspace_registry::resolve_workspace_from_path(&project_dir).ok().flatten();

    // QUIET.lock: global first, then per-workspace
    let control_voice = write_engine::ai_home().join("control/voice");
    if control_voice.join("QUIET.lock").exists() {
        return;
    }
    if let Some(ref ws) = workspace {
        if workspace_registry::workspace_control_dir(ws).join("QUIET.lock").exists() {
            return;
        }
    }

    // Filter markdown noise, then make paths speakable
    let filtered = text_core::strip_markdown(message);
    let filtered = text_core::tts_clean(&filtered);
    if filtered.trim().is_empty() {
        return;
    }

    // Spawn announce in background — do not wait
    let mut cmd = Command::new(announce_bin());
    cmd.args(["--profile", "cc"]);
    cmd.args(["--source", &project_dir]);
    cmd.arg("--stdin");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let Ok(mut child) = cmd.spawn() else {
        return;
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(filtered.as_bytes());
        // stdin drops here, closing the pipe
    }
    // Don't wait — announce manages its own background playback
}

fn announce_bin() -> PathBuf {
    write_engine::ai_home().join("tools/bin/announce")
}

