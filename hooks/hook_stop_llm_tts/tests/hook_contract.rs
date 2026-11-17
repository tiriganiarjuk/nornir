//! The hook as Claude Code runs it: a Stop event on stdin, and what `announce` is
//! given. Each test has a home directory of its own, holding a stand-in `announce`
//! that records its arguments and its stdin.

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
    cat > \"$HOME/announce_stdin\"\n\
    mv \"$HOME/announce_args.writing\" \"$HOME/announce_args\"\n";

/// A fresh home directory with the stand-in `announce` installed, unique per test.
fn home_with_announce(test_name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let home = std::env::temp_dir().join(format!("hook_stop_llm_tts_{test_name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let tools = home.join("ai/tools/bin");
    fs::create_dir_all(&tools)?;
    fs::write(tools.join("announce"), RECORDING_ANNOUNCE)?;
    fs::set_permissions(tools.join("announce"), fs::Permissions::from_mode(0o755))?;
    Ok(home)
}

fn run(home: &Path, payload: &str) -> Result<Output, Box<dyn Error>> {
    let mut hook = Command::new(env!("CARGO_BIN_EXE_hook_stop_llm_tts"))
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    hook.stdin.take().ok_or("the hook has no stdin")?.write_all(payload.as_bytes())?;
    Ok(hook.wait_with_output()?)
}

fn stopped(last_assistant_message: &str) -> String {
    serde_json::json!({ "cwd": "/projects/alpha", "last_assistant_message": last_assistant_message }).to_string()
}

/// What `announce` was called with, once it has finished; `None` when it was not called.
/// The hook does not wait for `announce`, so the record is waited for.
fn announced(home: &Path, wait: Duration) -> Result<Option<(Vec<String>, String)>, Box<dyn Error>> {
    let deadline = Instant::now() + wait;
    while !home.join("announce_args").exists() {
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let args = fs::read_to_string(home.join("announce_args"))?.lines().map(str::to_string).collect();
    Ok(Some((args, fs::read_to_string(home.join("announce_stdin"))?)))
}

const SPOKEN_WITHIN: Duration = Duration::from_secs(10);
const SILENT_FOR: Duration = Duration::from_millis(400);

#[test]
fn the_last_message_is_handed_to_announce() -> TestResult {
    let home = home_with_announce("spoken")?;

    let output = run(&home, &stopped("The deploy finished"))?;

    assert_eq!(output.status.code(), Some(0));
    let (args, spoken) = announced(&home, SPOKEN_WITHIN)?.ok_or("announce was not called")?;
    assert_eq!(args, ["--profile", "cc", "--source", "/projects/alpha", "--stdin"]);
    assert!(spoken.contains("The deploy finished"), "{spoken}");
    Ok(())
}

#[test]
fn an_empty_message_is_not_spoken() -> TestResult {
    let home = home_with_announce("empty")?;

    let output = run(&home, &stopped("   "))?;

    assert_eq!(output.status.code(), Some(0));
    assert!(announced(&home, SILENT_FOR)?.is_none());
    Ok(())
}

#[test]
fn a_quiet_lock_keeps_the_message_unspoken() -> TestResult {
    let home = home_with_announce("quiet")?;
    fs::create_dir_all(home.join("ai/control/voice"))?;
    fs::write(home.join("ai/control/voice/QUIET.lock"), "")?;

    let output = run(&home, &stopped("The deploy finished"))?;

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
