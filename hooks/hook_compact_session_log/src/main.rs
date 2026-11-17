//! PostCompact hook: persist compact_summary, emit datagram, announce via TTS.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_compact_session_log --project-dir $CLAUDE_PROJECT_DIR

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use datagram_core::{Datagram, DatagramKind, Priority};
use session_event_io::{PostCompact, SessionId};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "hook_compact_session_log", about = "PostCompact hook: log summary + TTS")]
struct Cli {
    /// Session project directory (from $CLAUDE_PROJECT_DIR)
    #[arg(long)]
    project_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

fn persist_summary(session_id: &SessionId, compact_summary: &str) {
    if compact_summary.is_empty() {
        return;
    }

    let epoch_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let timestamp = time_core::iso_zulu(epoch_secs);

    let record = summary_record(&timestamp, compact_summary);
    let jsonl_path = summary_path(&write_engine::ai_home(), session_id);

    if let Some(sessions_dir) = jsonl_path.parent() {
        if std::fs::create_dir_all(sessions_dir).is_ok() {
            let _ = write_engine::append_line_fsync(&jsonl_path, &record.to_string());
        }
    }
}

/// One line of a session's compaction log: when, and the summary.
fn summary_record(timestamp: &str, compact_summary: &str) -> serde_json::Value {
    serde_json::json!({
        "timestamp": timestamp,
        "compact_summary": compact_summary,
    })
}

/// Where a session's compaction summaries are kept under the ai home.
fn summary_path(ai_home: &Path, session_id: &SessionId) -> PathBuf {
    ai_home.join("intercept/sessions").join(session_id).join("compact_summary.jsonl")
}

fn emit_datagram(workspace: &str) {
    let datagram = Datagram {
        timestamp: datagram_io::now(),
        source: "hook_compact_session_log".into(),
        kind: DatagramKind::Notify,
        classifier: None,
        priority: Priority::Low,
        workspace: workspace.into(),
        detail: Some(format!("{workspace} compacted")),
        speech: None,
        payload: None,
    };
    datagram_io::emit(&datagram);
}

fn announce_compaction(workspace: &str, project_dir: &str) {
    let control_voice = write_engine::ai_home().join("control/voice");
    if control_voice.join("SILENT.lock").exists() {
        return;
    }
    if workspace_registry::workspace_control_dir(workspace).join("SILENT.lock").exists() {
        return;
    }

    let announce = write_engine::ai_home().join("tools/bin/announce");
    let mut cmd = Command::new(&announce);
    cmd.args(["--severity", "info"]);
    cmd.args(["--source", project_dir]);
    cmd.arg(format!("{workspace} compacted"));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = cmd.spawn();
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let args = Cli::parse();

    session_event_io::run_event_hook(|event: PostCompact| {
        let project_dir = session_event_io::project_dir(args.project_dir.as_deref(), &event.cwd);

        // Extract workspace name from project dir path
        let workspace = project_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".to_string());

        persist_summary(&event.session_id, &event.compact_summary);
        emit_datagram(&workspace);
        announce_compaction(&workspace, &project_dir.display().to_string());
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_line_holds_when_and_the_summary() {
        let record = summary_record("2026-11-17T07:00:00Z", "what happened");
        assert_eq!(record, serde_json::json!({ "timestamp": "2026-11-17T07:00:00Z", "compact_summary": "what happened" }));
    }

    #[test]
    fn a_summary_is_kept_under_its_session() -> Result<(), String> {
        let session_id = SessionId::try_from("s-1".to_string())?;
        assert_eq!(
            summary_path(Path::new("/home/u/ai"), &session_id),
            Path::new("/home/u/ai/intercept/sessions/s-1/compact_summary.jsonl"),
        );
        Ok(())
    }
}
