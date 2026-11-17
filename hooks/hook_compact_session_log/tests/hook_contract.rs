//! The hook as Claude Code runs it, when no event arrives. What it does with a
//! PostCompact event reaches Hlidskjalf, so that is tested on its parts, in the crate.

use std::error::Error;
use std::process::{Command, Stdio};

#[test]
fn without_an_event_the_hook_refuses_and_exits_1() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_hook_compact_session_log")).stdin(Stdio::null()).output()?;

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(String::from_utf8(output.stderr)?.lines().next(), Some("parse_error"));
    Ok(())
}
