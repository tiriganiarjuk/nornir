//! PreToolUse hook: the file-tool sandbox of a subagent.
//!
//! Usage (in agent frontmatter):
//!     hook_pre_subagent_tool Read=/schemas/,/docs/ Grep=/schemas/ Write=/output/
//!
//! Each argument is Tool=directory1,directory2: a tool the subagent may use,
//! and the directories it may use it in. A call is allowed only when its tool
//! is named and every path it reaches lies under one of that tool's
//! directories. A tool not named, a call that reaches no path, and a path
//! outside are all denied.

use std::collections::BTreeMap;
use std::process::ExitCode;

use hook_decision_core::paths::{is_under, normalize};
use hook_decision_core::{deny_because_undecided, HookDecision};
use hook_io::{HookInput, WhenUndecided};

const HOOK: &str = "hook_pre_subagent_tool";

fn main() -> ExitCode {
    hook_io::run_pre_hook(WhenUndecided::Deny, decide)
}

/// Tool → the directories it may be used in, normalised.
type ToolGrants = BTreeMap<String, Vec<String>>;

/// The command line, read strictly: every argument is `Tool=/dir[,/dir]`, and
/// at least one is given.
fn read_grants(args: &[String]) -> Result<ToolGrants, String> {
    if args.is_empty() {
        return Err("no tool is granted any directory".to_string());
    }
    let mut grants = ToolGrants::new();
    for arg in args {
        let (tool, directories) = tool_grant(arg)?;
        grants.entry(tool).or_default().extend(directories);
    }
    Ok(grants)
}

fn tool_grant(argument: &str) -> Result<(String, Vec<String>), String> {
    let malformed = || format!("{argument} is not Tool=/directory[,/directory]");
    let (tool, written) = argument.split_once('=').ok_or_else(malformed)?;
    let directories: Vec<String> = written.split(',').map(normalize).collect();
    if tool.is_empty() || directories.iter().any(|directory| !directory.starts_with('/')) {
        return Err(malformed());
    }
    Ok((tool.to_string(), directories))
}

fn decide(input: &HookInput) -> HookDecision {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match read_grants(&args) {
        Ok(grants) => decide_with_grants(input, &grants),
        Err(cause) => deny_because_undecided(HOOK, &cause),
    }
}

fn decide_with_grants(input: &HookInput, grants: &ToolGrants) -> HookDecision {
    let tool = input.tool_name.as_str();
    let Some(allowed) = grants.get(tool) else {
        return HookDecision::Deny {
            category: "path".into(),
            event: format!("tool '{tool}' has no path grants"),
            reason: format!("Tool '{tool}' is not in the allowed tool list."),
        };
    };

    let targets = input.targets();
    if targets.is_empty() {
        return HookDecision::Deny {
            category: "path".into(),
            event: format!("{tool} call names no path"),
            reason: format!("This {tool} call names no path. Name the path explicitly, under:\n{}", listed(allowed)),
        };
    }

    match targets.iter().find(|target| !allowed.iter().any(|directory| is_under(target, directory))) {
        None => HookDecision::Allow,
        Some(outside) => HookDecision::Deny {
            category: "path".into(),
            event: format!("blocked {tool} access to '{outside}'"),
            reason: format!(
                "'{outside}' is outside {tool}'s allowed paths.\nYou may only access:\n{}\n\
                 Adjust your path to use one of these locations.",
                listed(allowed)
            ),
        },
    }
}

fn listed(directories: &[String]) -> String {
    directories.iter().map(|directory| format!("  {directory}")).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grants() -> ToolGrants {
        let args: Vec<String> = ["Read=/schemas/,/docs/", "Write=/output/", "Grep=/schemas/", "Glob=/schemas"]
            .iter()
            .map(|arg| arg.to_string())
            .collect();
        read_grants(&args).unwrap()
    }

    fn call(tool: &str, cwd: Option<&str>, tool_input: serde_json::Value) -> HookInput {
        HookInput { tool_name: tool.to_string(), tool_input, cwd: cwd.map(str::to_string) }
    }

    fn file_call(tool: &str, path: &str) -> HookInput {
        call(tool, Some("/work"), serde_json::json!({ "file_path": path }))
    }

    fn allowed(input: &HookInput) -> bool {
        matches!(decide_with_grants(input, &grants()), HookDecision::Allow)
    }

    // ── read_grants ──────────────────────────────────────────────

    #[test]
    fn grants_are_read_and_merged() {
        let args = vec!["Read=/a/".to_string(), "Read=/b/../c".to_string()];
        assert_eq!(read_grants(&args).unwrap().get("Read"), Some(&vec!["/a/".to_string(), "/c".to_string()]));
    }

    #[test]
    fn no_grants_is_refused() {
        assert!(read_grants(&[]).is_err());
    }

    #[test]
    fn a_malformed_grant_is_refused() {
        for arg in ["Read", "Read=", "=/a", "Read=relative", "Read=/a,,/b", "Read=/a,b"] {
            assert!(read_grants(&[arg.to_string()]).is_err(), "{arg}");
        }
    }

    // ── Allowed ──────────────────────────────────────────────────

    #[test]
    fn a_path_under_a_granted_directory_is_allowed() {
        assert!(allowed(&file_call("Read", "/schemas/a.json")));
        assert!(allowed(&file_call("Read", "/docs/guide/b.md")));
        assert!(allowed(&file_call("Write", "/output/x.json")));
        assert!(allowed(&file_call("Read", "/schemas/sub/../a.json")));
    }

    #[test]
    fn a_search_inside_a_granted_directory_is_allowed() {
        assert!(allowed(&call("Grep", Some("/work"), serde_json::json!({ "pattern": "x", "path": "/schemas/" }))));
        assert!(allowed(&call("Glob", Some("/work"), serde_json::json!({ "pattern": "**/*.json", "path": "/schemas" }))));
        assert!(allowed(&call("Grep", Some("/schemas/sub"), serde_json::json!({ "pattern": "x" }))));
    }

    // ── Denied ───────────────────────────────────────────────────

    #[test]
    fn a_tool_without_a_grant_is_denied() {
        assert!(!allowed(&file_call("Edit", "/schemas/a.json")));
    }

    #[test]
    fn a_tool_is_held_to_its_own_directories() {
        assert!(!allowed(&file_call("Write", "/schemas/a.json")));
        assert!(!allowed(&file_call("Read", "/output/x.json")));
    }

    #[test]
    fn a_path_outside_is_denied() {
        assert!(!allowed(&file_call("Read", "/etc/passwd")));
        assert!(!allowed(&file_call("Read", "/schemas_private/a.json")));
    }

    #[test]
    fn a_path_that_climbs_out_is_denied() {
        assert!(!allowed(&file_call("Read", "/schemas/../etc/passwd")));
        assert!(!allowed(&file_call("Read", "/schemas/a/../../etc/passwd")));
    }

    #[test]
    fn a_relative_path_is_judged_from_the_working_directory() {
        assert!(!allowed(&file_call("Read", "schemas/a.json")));
        assert!(allowed(&call("Read", Some("/schemas"), serde_json::json!({ "file_path": "a.json" }))));
    }

    #[test]
    fn a_call_that_names_no_path_is_denied() {
        assert!(!allowed(&call("Read", Some("/schemas"), serde_json::json!({}))));
        assert!(!allowed(&call("Read", Some("/schemas"), serde_json::json!({ "file_path": null }))));
    }

    #[test]
    fn a_search_without_a_path_is_judged_by_where_it_runs() {
        assert!(!allowed(&call("Grep", Some("/home/user"), serde_json::json!({ "pattern": "x" }))));
        assert!(!allowed(&call("Grep", None, serde_json::json!({ "pattern": "x" }))));
    }

    #[test]
    fn a_search_filter_that_reaches_outside_is_denied() {
        let glob = call("Glob", Some("/work"), serde_json::json!({ "pattern": "/etc/*" }));
        assert!(!allowed(&glob));
        let climbing = call("Glob", Some("/work"), serde_json::json!({ "path": "/schemas", "pattern": "../etc/*" }));
        assert!(!allowed(&climbing));
        let grep = call("Grep", Some("/work"), serde_json::json!({ "pattern": "x", "path": "/schemas/", "glob": "../../etc/*" }));
        assert!(!allowed(&grep));
    }
}
