//! The hook as Claude Code runs it: a SessionEnd event on stdin, and what is in the
//! workspace's handoff slot afterwards. Each test has a home directory of its own.

use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

type TestResult = Result<(), Box<dyn Error>>;

/// A fresh home directory holding the control directory of the workspace `alpha`.
fn home_with_workspace(test_name: &str) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let home = std::env::temp_dir().join(format!("hook_session_end_handoff_{test_name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let workspace = home.join("ai/control/workspaces/alpha");
    fs::create_dir_all(workspace.join("s-1"))?;
    Ok((home, workspace))
}

fn run(home: &Path, payload: &str) -> Result<Output, Box<dyn Error>> {
    let mut hook = Command::new(env!("CARGO_BIN_EXE_hook_session_end_handoff"))
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    hook.stdin.take().ok_or("the hook has no stdin")?.write_all(payload.as_bytes())?;
    Ok(hook.wait_with_output()?)
}

fn ended(reason: &str) -> String {
    serde_json::json!({ "session_id": "s-1", "cwd": "/projects/alpha", "reason": reason }).to_string()
}

#[test]
fn a_clear_copies_the_ended_session_s_prompt_into_the_slot() -> TestResult {
    let (home, workspace) = home_with_workspace("clear")?;
    fs::write(workspace.join("s-1/SYSTEM_PROMPT.xml"), "<prompt/>")?;

    let output = run(&home, &ended("clear"))?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(workspace.join("CLEAR_HANDOFF.xml"))?, "<prompt/>");
    assert_eq!(fs::read_to_string(workspace.join("s-1/SYSTEM_PROMPT.xml"))?, "<prompt/>");
    Ok(())
}

#[test]
fn a_session_that_ends_another_way_hands_nothing_over() -> TestResult {
    let (home, workspace) = home_with_workspace("logout")?;
    fs::write(workspace.join("s-1/SYSTEM_PROMPT.xml"), "<prompt/>")?;

    let output = run(&home, &ended("logout"))?;

    assert_eq!(output.status.code(), Some(0));
    assert!(!workspace.join("CLEAR_HANDOFF.xml").exists());
    Ok(())
}

#[test]
fn a_cleared_session_with_no_prompt_hands_nothing_over() -> TestResult {
    let (home, workspace) = home_with_workspace("no_prompt")?;

    let output = run(&home, &ended("clear"))?;

    assert_eq!(output.status.code(), Some(0));
    assert!(!workspace.join("CLEAR_HANDOFF.xml").exists());
    Ok(())
}

#[test]
fn without_an_event_the_hook_refuses_and_exits_1() -> TestResult {
    let (home, workspace) = home_with_workspace("no_event")?;

    let output = run(&home, "")?;

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8(output.stderr)?.lines().next(), Some("parse_error"));
    assert!(!workspace.join("CLEAR_HANDOFF.xml").exists());
    Ok(())
}
