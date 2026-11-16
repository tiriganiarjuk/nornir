//! PostToolUse hook: quality assessment injection for file writes.
//!
//! When a tool writes a Python, Rust, or Svelte file, the file is assessed through
//! `syn_runner::assess_written_file`: saga examines it and writes its sidecar, and
//! syn applies the policy of the hook's working directory to the report.
//!
//! If the assessment has something to report, its TOON text is injected as additionalContext.
//! If the assessment is clean, returns silence (no injection).
//! If the assessment did not happen, injects that the file was not examined
//! and raises an alert on Hlidskjalf. The session is never blocked.
//!
//! Usage (in ~/.claude/settings.json):
//!     hook_post_llm_tool

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use failure_mode_core::Failure;
use hook_io::PostHookInput;
use syn_runner::{Assessment, SourceKind};

fn main() -> ExitCode {
    hook_io::run_post_hook(assess)
}

// ── Dispatcher ────────────────────────────────────────────────────

/// Other files are not examined: the hook says nothing about them.
fn assess(input: &PostHookInput) -> Option<String> {
    let file_path = extract_file_path(input)?;
    source_kind(&file_path)?;
    assess_source(&file_path)
}

// ── File classification ───────────────────────────────────────────

/// The kind of the written file, by its name; `None` for a file saga does not examine.
fn source_kind(path: &Path) -> Option<SourceKind> {
    SourceKind::of_file_name(path.file_name()?.to_str()?)
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
    let policy_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let outcome = syn_runner::assess_written_file(file_path, &policy_directory);
    if let Err(not_run) = &outcome {
        alert_not_run(not_run);
    }
    session_message(outcome)
}

/// What the session is told for an assessment's outcome: nothing when it is clean,
/// its report when it is not, and the failure's message when it did not happen.
fn session_message(outcome: Result<Assessment, Failure>) -> Option<String> {
    match outcome {
        Ok(assessment) if assessment.clean => None,
        Ok(assessment) => Some(assessment.report_text),
        Err(not_run) => Some(not_run.standard_message()),
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── session_message ───────────────────────────────────────────

    #[test]
    fn a_clean_assessment_gives_silence() {
        let clean = Assessment { clean: true, report_text: "All checks passed.".to_string() };
        assert_eq!(session_message(Ok(clean)), None);
    }

    #[test]
    fn an_assessment_with_something_to_report_gives_its_report() {
        let findings = Assessment { clean: false, report_text: "no_print: 1 issue".to_string() };
        assert_eq!(session_message(Ok(findings)).as_deref(), Some("no_print: 1 issue"));
    }

    #[test]
    fn an_assessment_that_did_not_happen_is_the_check_not_run_never_silence() {
        let not_run = failure_mode_core::quality_check_not_run("/project/src/a.py", "saga", "Permission denied");
        let message = session_message(Err(not_run)).unwrap();
        assert!(message.contains("did not run on '/project/src/a.py'"), "{message}");
        assert!(message.contains("saga failed"), "{message}");
        assert!(message.contains("not known to be clean"), "{message}");
    }

    // ── source_kind ───────────────────────────────────────────────

    #[test]
    fn a_python_rust_or_svelte_file_is_examined() {
        assert_eq!(source_kind(Path::new("/tmp/test.py")), Some(SourceKind::Python));
        assert_eq!(source_kind(Path::new("/tmp/test.rs")), Some(SourceKind::Rust));
        assert_eq!(source_kind(Path::new("/tmp/App.svelte")), Some(SourceKind::Svelte));
    }

    #[test]
    fn every_other_file_is_not_examined() {
        // .pyw is not .py: the extension is matched whole.
        for other in ["/tmp/Makefile", "/tmp/test.js", "/tmp/test.pyw", "/tmp/.py"] {
            assert_eq!(source_kind(Path::new(other)), None, "{other}");
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
}
