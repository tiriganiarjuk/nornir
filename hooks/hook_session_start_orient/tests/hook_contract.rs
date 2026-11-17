//! The hook as Claude Code runs it: a SessionStart event on stdin, and what is on
//! disk afterwards. Each test has a home directory of its own.

use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

type TestResult = Result<(), Box<dyn Error>>;

/// A fresh home directory holding the project `alpha`, unique per test.
fn home_with_project(test_name: &str) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let home = std::env::temp_dir().join(format!("hook_session_start_orient_{test_name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let project = home.join("projects/alpha");
    fs::create_dir_all(&project)?;
    Ok((home, project))
}

fn run(home: &Path, args: &[&str], payload: &str) -> Result<Output, Box<dyn Error>> {
    let mut hook = Command::new(env!("CARGO_BIN_EXE_hook_session_start_orient"))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    hook.stdin.take().ok_or("the hook has no stdin")?.write_all(payload.as_bytes())?;
    Ok(hook.wait_with_output()?)
}

fn started(session_id: &str, cwd: &Path, source: &str) -> String {
    serde_json::json!({ "session_id": session_id, "cwd": cwd, "source": source }).to_string()
}

fn session_prompt(home: &Path, session_id: &str) -> PathBuf {
    home.join("ai/control/workspaces/alpha").join(session_id).join("SYSTEM_PROMPT.xml")
}

#[test]
fn the_launch_prompt_moves_under_the_session() -> TestResult {
    let (home, project) = home_with_project("launch_prompt")?;
    fs::write(project.join(".SYSTEM_PROMPT.xml"), "<prompt/>")?;

    let output = run(&home, &[], &started("s-1", &project, "startup"))?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(session_prompt(&home, "s-1"))?, "<prompt/>");
    assert!(!project.join(".SYSTEM_PROMPT.xml").exists());
    Ok(())
}

#[test]
fn the_project_dir_named_on_the_command_line_is_the_one_oriented() -> TestResult {
    let (home, project) = home_with_project("named_project")?;
    fs::write(project.join(".SYSTEM_PROMPT.xml"), "<prompt/>")?;
    let elsewhere = home.join("projects/alpha/deep/inside");
    fs::create_dir_all(&elsewhere)?;

    let project_arg = project.to_str().ok_or("project path is not UTF-8")?;
    let output = run(&home, &["--project-dir", project_arg], &started("s-1", &elsewhere, "startup"))?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(session_prompt(&home, "s-1"))?, "<prompt/>");
    Ok(())
}

#[test]
fn a_session_without_a_launch_prompt_still_gets_its_workspace() -> TestResult {
    let (home, project) = home_with_project("no_prompt")?;

    let output = run(&home, &[], &started("s-1", &project, "startup"))?;

    assert_eq!(output.status.code(), Some(0));
    assert!(home.join("ai/control/workspaces/alpha").is_dir());
    assert!(home.join("ai/control/workspaces/registry.db").is_file());
    assert!(!session_prompt(&home, "s-1").exists());
    Ok(())
}

#[test]
fn a_clear_takes_the_prompt_handed_over_by_the_ended_session() -> TestResult {
    let (home, project) = home_with_project("clear_handoff")?;
    let workspace = home.join("ai/control/workspaces/alpha");
    fs::create_dir_all(&workspace)?;
    fs::write(workspace.join("CLEAR_HANDOFF.xml"), "<handed-over/>")?;

    let output = run(&home, &[], &started("s-2", &project, "clear"))?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(session_prompt(&home, "s-2"))?, "<handed-over/>");
    assert!(!workspace.join("CLEAR_HANDOFF.xml").exists());
    Ok(())
}

#[test]
fn a_startup_leaves_a_handed_over_prompt_where_it_is() -> TestResult {
    let (home, project) = home_with_project("startup_no_handoff")?;
    let workspace = home.join("ai/control/workspaces/alpha");
    fs::create_dir_all(&workspace)?;
    fs::write(workspace.join("CLEAR_HANDOFF.xml"), "<handed-over/>")?;

    let output = run(&home, &[], &started("s-2", &project, "startup"))?;

    assert_eq!(output.status.code(), Some(0));
    assert!(workspace.join("CLEAR_HANDOFF.xml").exists());
    assert!(!session_prompt(&home, "s-2").exists());
    Ok(())
}

#[test]
fn without_an_event_the_hook_refuses_and_touches_nothing() -> TestResult {
    let (home, _project) = home_with_project("no_event")?;

    let output = run(&home, &[], "")?;

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8(output.stderr)?.lines().next(), Some("parse_error"));
    assert!(!home.join("ai").exists());
    Ok(())
}
