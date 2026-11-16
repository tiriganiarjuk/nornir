//! The CLI face against the CLI contract (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces):
//! the three outcomes, each with its stdout, stderr and exit code.

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

const SCHEMA: &str = r#"{
  "type": "object",
  "properties": { "name": { "type": "string" }, "port": { "type": "integer" } },
  "required": ["name", "port"],
  "additionalProperties": false
}"#;

/// A fresh directory holding the schema and the two inputs, unique per test.
fn fixture_dir(test_name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("read_toml_validate_json_{test_name}_{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("service.schema.json"), SCHEMA)?;
    fs::write(dir.join("valid.toml"), "name = \"bifrost\"\nport = 4000\n")?;
    fs::write(dir.join("invalid.toml"), "name = \"bifrost\"\nport = \"not a number\"\n")?;
    Ok(dir)
}

fn run(args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_read_toml_validate_json")).args(args).output()?)
}

fn path_arg(path: &PathBuf) -> Result<&str, Box<dyn Error>> {
    path.to_str().ok_or_else(|| "fixture path is not UTF-8".into())
}

#[test]
fn success_prints_the_validated_data_and_exits_0() -> TestResult {
    let dir = fixture_dir("success")?;
    let file = dir.join("valid.toml");
    let output = run(&[path_arg(&file)?, "service", path_arg(&dir)?])?;

    assert_eq!(output.status.code(), Some(0));
    let data: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(data, serde_json::json!({ "name": "bifrost", "port": 4000 }));
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn failure_names_the_class_first_on_stderr_and_exits_1() -> TestResult {
    let dir = fixture_dir("failure")?;
    let file = dir.join("invalid.toml");
    let output = run(&[path_arg(&file)?, "service", path_arg(&dir)?])?;

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(stderr.lines().next(), Some("validation_error"));
    Ok(())
}

#[test]
fn wrong_arguments_print_the_usage_and_exit_2() -> TestResult {
    let output = run(&["only-one-argument"])?;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.starts_with("usage: read_toml_validate_json"));
    Ok(())
}
