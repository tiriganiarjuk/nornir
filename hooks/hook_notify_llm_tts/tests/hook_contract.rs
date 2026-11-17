//! The hook as Claude Code runs it: a Notification event on stdin, and what
//! `announce` is given. Each test has a home directory of its own, holding a
//! stand-in `announce` that records its arguments.

use std::error::Error;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn Error>>;

const RECORDING_ANNOUNCE: &str = "#!/bin/sh\n\
    printf '%s\\n' \"$@\" > \"$HOME/announce_args.writing\"\n\
    mv \"$HOME/announce_args.writing\" \"$HOME/announce_args\"\n";

/// A fresh home directory with the stand-in `announce` installed, unique per test.
fn home_with_announce(test_name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let home = std::env::temp_dir().join(format!("hook_notify_llm_tts_{test_name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let tools = home.join("ai/tools/bin");
    fs::create_dir_all(&tools)?;
    fs::write(tools.join("announce"), RECORDING_ANNOUNCE)?;
    fs::set_permissions(tools.join("announce"), fs::Permissions::from_mode(0o755))?;
    Ok(home)
}

fn run(home: &Path, payload: &str) -> Result<Output, Box<dyn Error>> {
    let mut hook = Command::new(env!("CARGO_BIN_EXE_hook_notify_llm_tts"))
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    hook.stdin.take().ok_or("the hook has no stdin")?.write_all(payload.as_bytes())?;
    Ok(hook.wait_with_output()?)
}

fn notified(notification_type: &str) -> String {
    serde_json::json!({ "cwd": "/projects/alpha", "notification_type": notification_type }).to_string()
}

/// The arguments `announce` was called with; `None` when it was not called.
/// The hook does not wait for `announce`, so the record is waited for.
fn announced(home: &Path, wait: Duration) -> Result<Option<Vec<String>>, Box<dyn Error>> {
    let deadline = Instant::now() + wait;
    while !home.join("announce_args").exists() {
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(Some(fs::read_to_string(home.join("announce_args"))?.lines().map(str::to_string).collect()))
}

const SPOKEN_WITHIN: Duration = Duration::from_secs(10);
const SILENT_FOR: Duration = Duration::from_millis(400);

#[test]
fn a_permission_prompt_says_the_workspace_needs_approval() -> TestResult {
    let home = home_with_announce("permission")?;

    let output = run(&home, &notified("permission_prompt"))?;

    assert_eq!(output.status.code(), Some(0));
    let args = announced(&home, SPOKEN_WITHIN)?.ok_or("announce was not called")?;
    assert_eq!(args, ["--severity", "notify", "--source", "/projects/alpha", "alpha needs approval"]);
    Ok(())
}

#[test]
fn an_idle_prompt_says_the_workspace_is_idle() -> TestResult {
    let home = home_with_announce("idle")?;

    run(&home, &notified("idle_prompt"))?;

    let args = announced(&home, SPOKEN_WITHIN)?.ok_or("announce was not called")?;
    assert_eq!(args, ["--severity", "info", "--source", "/projects/alpha", "alpha idle"]);
    Ok(())
}

#[test]
fn another_kind_of_notification_is_not_spoken() -> TestResult {
    let home = home_with_announce("other")?;

    let output = run(&home, &notified("auth_success"))?;

    assert_eq!(output.status.code(), Some(0));
    assert!(announced(&home, SILENT_FOR)?.is_none());
    Ok(())
}

#[test]
fn a_silent_lock_keeps_the_notification_unspoken() -> TestResult {
    let home = home_with_announce("silent")?;
    fs::create_dir_all(home.join("ai/control/voice"))?;
    fs::write(home.join("ai/control/voice/SILENT.lock"), "")?;

    let output = run(&home, &notified("permission_prompt"))?;

    assert_eq!(output.status.code(), Some(0));
    assert!(announced(&home, SILENT_FOR)?.is_none());
    Ok(())
}

#[test]
fn without_an_event_the_hook_refuses_and_speaks_nothing() -> TestResult {
    let home = home_with_announce("no_event")?;

    let output = run(&home, "")?;

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8(output.stderr)?.lines().next(), Some("parse_error"));
    assert!(announced(&home, SILENT_FOR)?.is_none());
    Ok(())
}
