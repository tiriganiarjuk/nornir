//! The CLI face against the CLI contract (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces):
//! the three outcomes, each with its stdout, stderr, exit code and effect on disk.

use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

type TestResult = Result<(), Box<dyn Error>>;

const VALID: &str = r#"{"frontmatter": {"name": "bifrost"}, "body": "The bridge.\\n"}"#;

/// A fresh directory holding a schema that accepts any object, unique per test.
fn fixture_dir(test_name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("write_json_validate_markdown_{test_name}_{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("any.schema.json"), r#"{ "type": "object" }"#)?;
    Ok(dir)
}

fn run(args: &[&str], stdin: &str) -> Result<Output, Box<dyn Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_write_json_validate_markdown"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().ok_or("no stdin")?.write_all(stdin.as_bytes())?;
    Ok(child.wait_with_output()?)
}

fn path_arg(path: &Path) -> Result<&str, Box<dyn Error>> {
    path.to_str().ok_or_else(|| "fixture path is not UTF-8".into())
}

#[test]
fn success_writes_the_file_prints_nothing_and_exits_0() -> TestResult {
    let dir = fixture_dir("success")?;
    let file = dir.join("out.md");
    let output = run(&[path_arg(&file)?, "any", path_arg(&dir)?], VALID)?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(file.exists());
    Ok(())
}

#[test]
fn failure_names_the_class_first_on_stderr_writes_nothing_and_exits_1() -> TestResult {
    let dir = fixture_dir("failure")?;
    let file = dir.join("out.md");
    let output = run(&[path_arg(&file)?, "any", path_arg(&dir)?], "[1]")?;

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(stderr.lines().next(), Some("validation_error"));
    assert!(!file.exists());
    Ok(())
}

#[test]
fn wrong_arguments_print_the_usage_and_exit_2() -> TestResult {
    let output = run(&[], "")?;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.starts_with("usage: write_json_validate_markdown"));
    Ok(())
}
