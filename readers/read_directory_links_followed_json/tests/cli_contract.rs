//! The CLI face against the CLI contract (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces):
//! the three outcomes, each with its stdout, stderr and exit code — and the two refusals
//! that following links adds.

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

/// A fresh project, unique per test, whose `atom` level is a link into a core tree beside
/// it and outside its root. Returns the directory holding both, and the project's root.
fn fixture_dir(test_name: &str) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("read_directory_links_followed_json_{test_name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("core/source/atom"))?;
    fs::write(dir.join("core/source/atom/path_absolute.yaml"), "kind: atom")?;
    let project = dir.join("project/source");
    fs::create_dir_all(project.join("section"))?;
    fs::write(project.join("section/identity.yaml"), "x")?;
    symlink("../../core/source/atom", project.join("atom"))?;
    Ok((dir, project))
}

fn run(args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_read_directory_links_followed_json")).args(args).output()?)
}

fn path_arg(path: &PathBuf) -> Result<&str, Box<dyn Error>> {
    path.to_str().ok_or_else(|| "fixture path is not UTF-8".into())
}

#[test]
fn success_lists_what_each_link_leads_to_and_exits_0() -> TestResult {
    let (dir, project) = fixture_dir("success")?;
    let output = run(&[path_arg(&project)?])?;

    assert_eq!(output.status.code(), Some(0));
    let listing: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let entries = listing["entries"].as_array().ok_or("no entries")?;
    let listed: Vec<(&str, &str)> = entries.iter()
        .filter_map(|entry| Some((entry["path"].as_str()?, entry["kind"].as_str()?)))
        .collect();
    assert_eq!(listed, [
        ("atom", "directory"),
        ("atom/path_absolute.yaml", "file"),
        ("section", "directory"),
        ("section/identity.yaml", "file"),
    ]);
    assert_eq!(entries[1]["size"], serde_json::json!(10));
    assert!(output.stderr.is_empty());
    fs::remove_dir_all(&dir).ok();
    Ok(())
}

#[test]
fn a_broken_link_is_a_read_error_naming_the_link_and_exits_1() -> TestResult {
    let (dir, project) = fixture_dir("broken")?;
    symlink("../../core/source/absent", project.join("enum"))?;
    let output = run(&[path_arg(&project)?])?;

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(stderr.lines().next(), Some("read_error"));
    assert!(stderr.contains("\n  enum\n"), "{stderr}");
    fs::remove_dir_all(&dir).ok();
    Ok(())
}

#[test]
fn a_link_that_leads_back_is_a_validation_error_naming_it_and_where_it_leads() -> TestResult {
    let (dir, project) = fixture_dir("loop")?;
    symlink("..", project.join("section/up"))?;
    let output = run(&[path_arg(&project)?])?;

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(stderr.lines().next(), Some("validation_error"));
    let leads_back_to = fs::canonicalize(&project)?;
    assert!(stderr.contains(&format!("section/up leads back to {}", leads_back_to.display())), "{stderr}");
    fs::remove_dir_all(&dir).ok();
    Ok(())
}

#[test]
fn failure_names_the_class_first_on_stderr_and_exits_1() -> TestResult {
    let (dir, project) = fixture_dir("failure")?;
    let output = run(&[path_arg(&project.join("absent"))?])?;

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
    assert!(stderr.starts_with("usage: read_directory_links_followed_json"));
    Ok(())
}
