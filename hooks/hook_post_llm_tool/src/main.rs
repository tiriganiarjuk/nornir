//! PostToolUse hook: quality assessment injection for file writes.
//!
//! When a tool writes a Python, Rust, or Svelte file, runs the quality pipeline:
//!   saga <file> --sidecar | syn --stdin
//!
//! If syn reports violations or faults, injects the TOON assessment as additionalContext.
//! If syn reports nothing, returns silence (no injection).
//! If saga or syn could not run, or failed, injects that the file was not examined
//! and raises an alert on Hlidskjalf. The session is never blocked.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_post_llm_tool

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output, Stdio};

use failure_mode_core::Failure;
use hook_io::PostHookInput;

fn main() -> ExitCode {
    hook_io::run_post_hook(assess)
}

// ── Dispatcher ────────────────────────────────────────────────────

fn assess(input: &PostHookInput) -> Option<String> {
    let file_path = extract_file_path(input)?;
    match classify_file(&file_path) {
        FileKind::Python | FileKind::Rust | FileKind::Svelte => assess_source(&file_path),
        FileKind::Other => None,
    }
}

// ── File classification ───────────────────────────────────────────

enum FileKind {
    Python,
    Rust,
    Svelte,
    Other,
}

fn classify_file(path: &Path) -> FileKind {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => FileKind::Python,
        Some("rs") => FileKind::Rust,
        Some("svelte") => FileKind::Svelte,
        _ => FileKind::Other,
    }
}

fn extract_file_path(input: &PostHookInput) -> Option<PathBuf> {
    let path = Path::new(input.file_path()?);
    if path.exists() {
        Some(path.to_path_buf())
    } else {
        None
    }
}

// ── Source assessment (Python + Rust + Svelte) ───────────────────

/// What the session is told about the file it wrote: syn's assessment, or — when
/// the quality check did not run — that it did not, which is also raised as an
/// alert. Silence means the check ran and found nothing.
fn assess_source(file_path: &Path) -> Option<String> {
    match quality_report(file_path, &saga_bin(), &syn_bin()) {
        Ok(report) => report,
        Err(not_run) => {
            alert_not_run(&not_run);
            Some(not_run.standard_message())
        }
    }
}

/// Syn's assessment of the file from a fresh saga report; `None` when it found
/// nothing. A saga or syn that cannot be run, or exits in failure, is the failure
/// `quality_check_not_run`.
fn quality_report(file_path: &Path, saga_binary: &Path, syn_binary: &Path) -> Result<Option<String>, Failure> {
    let shown = file_path.to_string_lossy();
    let not_run =
        |instrument: &str, reason: String| failure_mode_core::quality_check_not_run(&shown, instrument, reason);

    let mut saga_process = Command::new(saga_binary)
        .arg(file_path)
        .arg("--sidecar")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|io_err| not_run("saga", io_err.to_string()))?;
    let saga_stdout = saga_process
        .stdout
        .take()
        .ok_or_else(|| not_run("saga", "its output could not be captured".to_string()))?;

    let syn_output = Command::new(syn_binary)
        .arg("--stdin")
        .stdin(saga_stdout)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|io_err| not_run("syn", io_err.to_string()))?;
    let saga_output = saga_process.wait_with_output().map_err(|io_err| not_run("saga", io_err.to_string()))?;

    if !saga_output.status.success() {
        return Err(not_run("saga", exit_reason(&saga_output)));
    }
    if !syn_output.status.success() {
        return Err(not_run("syn", exit_reason(&syn_output)));
    }

    let toon = String::from_utf8_lossy(&syn_output.stdout);
    let trimmed = toon.trim();
    if trimmed.is_empty() || trimmed == "All checks passed." {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

/// How a process ended and what it said on stderr, on one line.
fn exit_reason(output: &Output) -> String {
    let said: Vec<&str> = std::str::from_utf8(&output.stderr)
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if said.is_empty() {
        output.status.to_string()
    } else {
        format!("{} ({})", said.join(" "), output.status)
    }
}

/// Tell the human on Hlidskjalf that a written file was not examined.
fn alert_not_run(not_run: &Failure) {
    let datagram = datagram_io::Datagram {
        timestamp: datagram_io::now(),
        source: "hook_post_llm_tool".to_string(),
        kind: datagram_io::DatagramKind::Alert,
        classifier: None,
        priority: not_run.alert.unwrap_or(datagram_io::Priority::High),
        workspace: datagram_io::workspace_name(),
        detail: Some(not_run.standard_message()),
        speech: Some("The quality check did not run on a written file. Check Hlidskjalf.".to_string()),
        payload: None,
    };
    datagram_io::emit_validated_or_alert(&datagram, "hook_post_llm_tool");
}

// ── Binary resolution ─────────────────────────────────────────────

fn tools_bin() -> PathBuf {
    write_engine::ai_home().join("tools/bin")
}

fn saga_bin() -> PathBuf {
    tools_bin().join("saga")
}

fn syn_bin() -> PathBuf {
    tools_bin().join("syn")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── quality_report ────────────────────────────────────────────

    #[test]
    fn a_saga_that_cannot_be_run_is_the_check_not_run_never_silence() {
        let not_run = quality_report(
            Path::new("/project/src/a.py"),
            Path::new("/nonexistent/hook_post_llm_tool/saga"),
            Path::new("/nonexistent/hook_post_llm_tool/syn"),
        )
        .unwrap_err();
        assert_eq!(not_run.mode, "quality_check_not_run");
        assert!(not_run.standard.contains("'/project/src/a.py'"), "{}", not_run.standard);
        assert!(not_run.standard.contains("saga failed"), "{}", not_run.standard);
    }

    #[test]
    fn a_syn_that_exits_in_failure_is_the_check_not_run_with_what_it_said() {
        // `true` stands in for a saga that ran; `ls` of a missing path for a syn that refused.
        let not_run = quality_report(Path::new("/nonexistent/hook_post_llm_tool/a.py"), Path::new("/usr/bin/true"), Path::new("/bin/ls"))
            .unwrap_err();
        assert_eq!(not_run.mode, "quality_check_not_run");
        assert!(not_run.standard.contains("syn failed"), "{}", not_run.standard);
        assert!(not_run.standard.contains("exit status"), "{}", not_run.standard);
    }

    #[test]
    fn instruments_that_find_nothing_give_silence() {
        let report = quality_report(Path::new("/project/src/a.py"), Path::new("/usr/bin/true"), Path::new("/usr/bin/true"));
        assert_eq!(report.ok(), Some(None));
    }

    // ── classify_file ─────────────────────────────────────────────

    #[test]
    fn classify_python_file() {
        match classify_file(Path::new("/tmp/test.py")) {
            FileKind::Python => {} // correct
            _ => panic!(".py must classify as Python"),
        }
    }

    #[test]
    fn classify_rust_file() {
        match classify_file(Path::new("/tmp/test.rs")) {
            FileKind::Rust => {} // correct
            _ => panic!(".rs must classify as Rust"),
        }
    }

    #[test]
    fn classify_svelte_file() {
        match classify_file(Path::new("/tmp/App.svelte")) {
            FileKind::Svelte => {} // correct
            _ => panic!(".svelte must classify as Svelte"),
        }
    }

    #[test]
    fn classify_no_extension_as_other() {
        match classify_file(Path::new("/tmp/Makefile")) {
            FileKind::Other => {} // correct
            _ => panic!("No extension must classify as Other"),
        }
    }

    #[test]
    fn classify_javascript_as_other() {
        match classify_file(Path::new("/tmp/test.js")) {
            FileKind::Other => {} // correct
            _ => panic!(".js must classify as Other"),
        }
    }

    #[test]
    fn classify_pyw_as_other() {
        // .pyw is NOT .py — strict extension match
        match classify_file(Path::new("/tmp/test.pyw")) {
            FileKind::Other => {} // correct
            _ => panic!(".pyw must classify as Other"),
        }
    }

    // ── extract_file_path ─────────────────────────────────────────

    #[test]
    fn extract_file_path_no_field() {
        let input = PostHookInput {
            tool_name: Some("Write".to_string()),
            tool_input: serde_json::json!({}),        };
        assert!(extract_file_path(&input).is_none());
    }

    #[test]
    fn extract_file_path_null_value() {
        let input = PostHookInput {
            tool_name: Some("Write".to_string()),
            tool_input: serde_json::json!({ "file_path": null }),        };
        assert!(extract_file_path(&input).is_none());
    }

    #[test]
    fn extract_file_path_numeric_value() {
        let input = PostHookInput {
            tool_name: Some("Write".to_string()),
            tool_input: serde_json::json!({ "file_path": 42 }),        };
        assert!(extract_file_path(&input).is_none());
    }

    // ── tools_bin / saga_bin / syn_bin resolution ──────────────────

    #[test]
    fn saga_bin_under_tools_bin() {
        let saga = saga_bin();
        let tools = tools_bin();
        assert!(saga.starts_with(&tools));
        assert_eq!(saga.file_name().unwrap().to_str().unwrap(), "saga");
    }

    #[test]
    fn syn_bin_under_tools_bin() {
        let syn = syn_bin();
        let tools = tools_bin();
        assert!(syn.starts_with(&tools));
        assert_eq!(syn.file_name().unwrap().to_str().unwrap(), "syn");
    }
}
