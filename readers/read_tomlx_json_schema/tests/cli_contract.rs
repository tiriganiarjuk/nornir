//! The CLI face against the CLI contract (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces):
//! the three outcomes, each with its stdout, stderr and exit code.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

/// A fresh directory holding a valid input and a schema that accepts it, unique per test.
fn fixture_dir(test_name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("read_tomlx_json_schema_{test_name}_{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("any.schema.json"), r#"{ "type": "object" }"#)?;
    fs::write(dir.join("valid.tomlx"), "[features]  # type=bool\ndark_mode = true\n")?;
    Ok(dir)
}

fn run(args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_read_tomlx_json_schema")).args(args).output()?)
}

fn path_arg(path: &Path) -> Result<&str, Box<dyn Error>> {
    path.to_str().ok_or_else(|| "fixture path is not UTF-8".into())
}

#[test]
fn success_prints_the_data_as_json_and_exits_0() -> TestResult {
    let dir = fixture_dir("success")?;
    let file = dir.join("valid.tomlx");
    let output = run(&[path_arg(&file)?])?;

    assert_eq!(output.status.code(), Some(0));
    let data: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert!(data.is_object());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn failure_names_the_class_first_on_stderr_and_exits_1() -> TestResult {
    let dir = fixture_dir("failure")?;
    let file = dir.join("absent.tomlx");
    let output = run(&[path_arg(&file)?])?;

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(stderr.lines().next(), Some("read_error"));
    Ok(())
}

#[test]
fn wrong_arguments_print_the_usage_and_exit_2() -> TestResult {
    let output = run(&[])?;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.starts_with("usage: read_tomlx_json_schema"));
    Ok(())
}
