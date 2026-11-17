//! The hook as Claude Code runs it: a SessionStart event on stdin, and the context
//! it replies with. Each test has a home directory of its own.

use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

type TestResult = Result<(), Box<dyn Error>>;

/// A fresh home directory holding the control directory of the workspace `alpha`.
fn home_with_workspace(test_name: &str) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let home = std::env::temp_dir().join(format!("hook_session_start_inject_{test_name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let workspace = home.join("ai/control/workspaces/alpha");
    fs::create_dir_all(&workspace)?;
    Ok((home, workspace))
}

fn store_prompt(workspace: &Path, session_id: &str, prompt: &str) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(workspace.join(session_id))?;
    Ok(fs::write(workspace.join(session_id).join("SYSTEM_PROMPT.xml"), prompt)?)
}

fn run(home: &Path, session_id: &str, source: &str) -> Result<Output, Box<dyn Error>> {
    let payload = serde_json::json!({ "session_id": session_id, "cwd": "/projects/alpha", "source": source });
    run_with(home, &payload.to_string())
}

fn run_with(home: &Path, payload: &str) -> Result<Output, Box<dyn Error>> {
    let mut hook = Command::new(env!("CARGO_BIN_EXE_hook_session_start_inject"))
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    hook.stdin.take().ok_or("the hook has no stdin")?.write_all(payload.as_bytes())?;
    Ok(hook.wait_with_output()?)
}

/// The context the hook replied with.
fn injected(output: &Output) -> Result<String, Box<dyn Error>> {
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(reply["hookSpecificOutput"]["hookEventName"], "SessionStart");
    Ok(reply["hookSpecificOutput"]["additionalContext"].as_str().ok_or("no additionalContext")?.to_string())
}

#[test]
fn a_compacted_session_gets_its_prompt_back() -> TestResult {
    let (home, workspace) = home_with_workspace("compact")?;
    store_prompt(&workspace, "s-1", "<prompt/>")?;

    let output = run(&home, "s-1", "compact")?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(injected(&output)?, "<prompt/>");
    Ok(())
}

#[test]
fn a_resumed_session_gets_its_prompt_back() -> TestResult {
    let (home, workspace) = home_with_workspace("resume")?;
    store_prompt(&workspace, "s-1", "<prompt/>")?;

    assert_eq!(injected(&run(&home, "s-1", "resume")?)?, "<prompt/>");
    Ok(())
}

#[test]
fn a_started_session_is_given_nothing() -> TestResult {
    let (home, workspace) = home_with_workspace("startup")?;
    store_prompt(&workspace, "s-1", "<prompt/>")?;

    let output = run(&home, "s-1", "startup")?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn a_session_with_no_stored_prompt_is_given_nothing() -> TestResult {
    let (home, _workspace) = home_with_workspace("no_prompt")?;

    let output = run(&home, "s-1", "compact")?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn a_prompt_large_enough_to_be_cut_leads_with_the_integrity_banner() -> TestResult {
    let (home, workspace) = home_with_workspace("large")?;
    let prompt = "<rule/>".repeat(2000);
    store_prompt(&workspace, "s-1", &prompt)?;

    let context = injected(&run(&home, "s-1", "compact")?)?;

    assert!(context.starts_with(&format!("<injection-integrity bytes=\"{}\">", prompt.len())), "{}", &context[..80]);
    assert!(context.ends_with(&prompt));
    Ok(())
}

#[test]
fn a_cleared_session_gets_the_prompt_handed_over() -> TestResult {
    let (home, workspace) = home_with_workspace("clear")?;
    fs::write(workspace.join("CLEAR_HANDOFF.xml"), "<handed-over/>")?;

    assert_eq!(injected(&run(&home, "s-2", "clear")?)?, "<handed-over/>");
    Ok(())
}

#[test]
fn a_compacted_session_is_not_given_a_handed_over_prompt() -> TestResult {
    let (home, workspace) = home_with_workspace("compact_no_handoff")?;
    fs::write(workspace.join("CLEAR_HANDOFF.xml"), "<handed-over/>")?;

    assert!(run(&home, "s-2", "compact")?.stdout.is_empty());
    Ok(())
}

#[test]
fn without_an_event_the_hook_refuses_and_exits_1() -> TestResult {
    let (home, _workspace) = home_with_workspace("no_event")?;

    let output = run_with(&home, "")?;

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(String::from_utf8(output.stderr)?.lines().next(), Some("parse_error"));
    Ok(())
}
