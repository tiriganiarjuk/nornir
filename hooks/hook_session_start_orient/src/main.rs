//! SessionStart hook: register workspace and session in the workspace registry.
//!
//! Runs on every session start (startup, resume, clear, compact).
//! Registers the workspace name + path and session_id → workspace mapping.
//! Creates the control directory for the workspace if it doesn't exist.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_session_start_orient --project-dir $CLAUDE_PROJECT_DIR

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use session_event_io::SessionStart;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "hook_session_start_orient", about = "SessionStart hook: register workspace + session")]
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
    session_event_io::run_event_hook(|event: SessionStart| {
        let project_dir = session_event_io::project_dir(args.project_dir.as_deref(), &event.cwd);
        orient(&event, &project_dir);
    })
}

fn orient(event: &SessionStart, project_dir: &std::path::Path) {
    // Extract workspace name from last path component
    let workspace = project_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Register workspace (upsert — safe to call on every session start)
    let _ = workspace_registry::register_workspace(&workspace, &project_dir.display().to_string());

    // Register session → workspace mapping
    let _ = workspace_registry::register_session(event.session_id.as_str(), &workspace);

    // Ensure control directory exists
    let control_dir = workspace_registry::workspace_control_dir(&workspace);
    let _ = std::fs::create_dir_all(&control_dir);

    // Move .SYSTEM_PROMPT.xml into session-specific dir (if present).
    // cc_launch writes this ephemeral file at launch; we relocate it so
    // hook_session_start_inject can re-inject from a stable, session-keyed path.
    let session_dir = control_dir.join(&event.session_id);
    let dst = session_dir.join("SYSTEM_PROMPT.xml");

    let src = project_dir.join(".SYSTEM_PROMPT.xml");
    if src.exists() {
        let _ = std::fs::create_dir_all(&session_dir);
        let _ = std::fs::rename(&src, &dst);
    }

    // /clear mints a new session_id, so no launch-time file exists for it.
    // hook_session_end_handoff (SessionEnd, reason "clear") left the old
    // session's prompt in the workspace-level slot; move it into place so
    // this session's later compactions re-inject from the session-keyed
    // path like any other session.
    if event.source == "clear" && !dst.exists() {
        let slot = workspace_registry::clear_handoff_path(&workspace);
        if workspace_registry::clear_handoff_is_fresh(&slot) {
            let _ = std::fs::create_dir_all(&session_dir);
            let _ = std::fs::rename(&slot, &dst);
        }
    }
}
