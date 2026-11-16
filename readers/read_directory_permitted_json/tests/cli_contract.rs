//! The CLI face against the CLI contract (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces):
//! the three outcomes, each with its stdout, stderr and exit code — and a directory the
//! caller may not look into, which is part of a successful listing.

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

/// A fresh tree, unique per test: one readable folder with a file, one folder to lock.
fn fixture_dir(test_name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("read_directory_permitted_json_{test_name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("people"))?;
    fs::write(dir.join("people/ada.md"), "abc")?;
    fs::create_dir_all(dir.join("locked"))?;
    fs::write(dir.join("locked/secret.md"), "x")?;
    Ok(dir)
}

fn run(args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_read_directory_permitted_json")).args(args).output()?)
}

fn path_arg(path: &PathBuf) -> Result<&str, Box<dyn Error>> {
    path.to_str().ok_or_else(|| "fixture path is not UTF-8".into())
}

#[test]
fn success_lists_a_locked_directory_as_not_permitted_and_exits_0() -> TestResult {
    let dir = fixture_dir("success")?;
    let locked = dir.join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))?;
    let output = run(&[path_arg(&dir)?]);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755))?;
    let output = output?;

    assert_eq!(output.status.code(), Some(0));
    let listing: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let paths: Vec<&str> = listing["entries"].as_array().ok_or("no entries")?.iter()
        .filter_map(|entry| entry["path"].as_str()).collect();
    assert_eq!(paths, ["locked", "people", "people/ada.md"]);
    assert_eq!(listing["entries"][0]["permitted"], serde_json::json!(false));
    assert_eq!(listing["vanished"], serde_json::json!([]));
    assert!(output.stderr.is_empty());
    fs::remove_dir_all(&dir).ok();
    Ok(())
}

#[test]
fn failure_names_the_class_first_on_stderr_and_exits_1() -> TestResult {
    let dir = fixture_dir("failure")?;
    let output = run(&[path_arg(&dir.join("absent"))?])?;

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(stderr.lines().next(), Some("read_error"));
    fs::remove_dir_all(&dir).ok();
    Ok(())
}

#[test]
fn wrong_arguments_print_the_usage_and_exit_2() -> TestResult {
    let output = run(&[])?;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.starts_with("usage: read_directory_permitted_json"));
    Ok(())
}
