//! The CLI face against the CLI contract (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces):
//! the three outcomes, each with its stdout, stderr, exit code and effect on disk.

use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

type TestResult = Result<(), Box<dyn Error>>;

const REQUEST: &str = r#"{"files": [{"path": "service.json", "format": "json", "schema_name": "any", "data": {"name": "bifrost"}}]}"#;

/// A fresh directory holding a schema that accepts any object, unique per test.
fn fixture_dir(test_name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("write_json_validate_directory_{test_name}_{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("any.schema.json"), r#"{ "type": "object" }"#)?;
    Ok(dir)
}

fn run(args: &[&str], stdin: &str) -> Result<Output, Box<dyn Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_write_json_validate_directory"))
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
fn success_writes_the_directory_prints_its_report_and_exits_0() -> TestResult {
    let dir = fixture_dir("success")?;
    let generated = dir.join("generated");
    let output = run(&[path_arg(&generated)?, path_arg(&dir)?], REQUEST)?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report, serde_json::json!({ "written": 1, "unchanged": 0, "removed": 0 }));
    assert!(generated.join("service.json").exists());
    assert!(generated.join(".generated_directory.json").exists());
    Ok(())
}

#[test]
fn a_folder_it_did_not_write_is_refused_untouched_and_exits_1() -> TestResult {
    let dir = fixture_dir("refused")?;
    let authored = dir.join("authored");
    fs::create_dir_all(&authored)?;
    fs::write(authored.join("notes.md"), "Written by hand.\n")?;
    let output = run(&[path_arg(&authored)?, path_arg(&dir)?], REQUEST)?;

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(stderr.lines().next(), Some("write_error"));
    assert_eq!(fs::read_to_string(authored.join("notes.md"))?, "Written by hand.\n");
    assert!(!authored.join("service.json").exists());
    Ok(())
}

#[test]
fn wrong_arguments_print_the_usage_and_exit_2() -> TestResult {
    let output = run(&[], "")?;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.starts_with("usage: write_json_validate_directory"));
    Ok(())
}
