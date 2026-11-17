//! Fixture sets: concrete calls with the verdict a hook must give each.
//!
//! A fixture file pins a hook's behaviour. It states the command line the
//! hook is judged under, and then one entry per call: a Bash command or a
//! tool call, and the verdict — and for anything but `allow`, the category —
//! the hook must give it.
//!
//! ```toml
//! [judged_under]
//! arguments = "--chaining block --probing warn"
//! working_directory = "/work/project"
//!
//! [[fixture]]
//! name = "two commands joined by a semicolon"
//! command = "echo a; echo b"
//! verdict = "deny"
//! category = "chaining"
//! found = "2026-11-17, in a site-tools session"
//! ```
//!
//! The file is read strictly: an entry that cannot be read, a key this reader
//! does not know, or a name used twice is an error, and no set is returned.
//! A hook's test judges every fixture and reports every mismatch at once.

use std::collections::BTreeSet;

use crate::HookDecision;

/// What a hook does with a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Warn,
    Ask,
    Deny,
}

impl Verdict {
    fn written(self) -> &'static str {
        match self {
            Verdict::Allow => "allow",
            Verdict::Warn => "warn",
            Verdict::Ask => "ask",
            Verdict::Deny => "deny",
        }
    }
}

fn parse_verdict(written: &str) -> Option<Verdict> {
    [Verdict::Allow, Verdict::Warn, Verdict::Ask, Verdict::Deny]
        .into_iter()
        .find(|verdict| verdict.written() == written)
}

/// The call a fixture makes.
#[derive(Debug, Clone, PartialEq)]
pub enum FixtureCall {
    /// A Bash command.
    Command(String),
    /// A call to a file tool, with its input.
    Tool { tool: String, tool_input: toml::Table },
}

/// One pinned call.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub name: String,
    pub call: FixtureCall,
    /// The directory the call is made from; the set's own when it states none.
    pub working_directory: String,
    pub verdict: Verdict,
    /// The category of the decision; `None` exactly when the verdict is allow.
    pub category: Option<String>,
}

/// A fixture file: what the hook is judged under, and its fixtures.
#[derive(Debug, Clone)]
pub struct FixtureSet {
    /// The hook's command line, as its words.
    pub arguments: Vec<String>,
    /// The value of HOOK_LLM_ALLOW_PATHS the hook is judged under.
    pub allow_paths: String,
    /// The home directory `~` stands for.
    pub home: String,
    pub fixtures: Vec<Fixture>,
}

const SET_KEYS: [&str; 2] = ["judged_under", "fixture"];
const JUDGED_UNDER_KEYS: [&str; 4] = ["arguments", "allow_paths", "home", "working_directory"];
const FIXTURE_KEYS: [&str; 8] =
    ["name", "command", "tool", "tool_input", "working_directory", "verdict", "category", "found"];

/// Read a fixture file.
pub fn parse_fixture_set(toml_str: &str) -> Result<FixtureSet, String> {
    let table: toml::Table =
        toml_str.parse().map_err(|toml_err| format!("the fixture file is not TOML \u{2014} {toml_err}"))?;
    refuse_unknown_keys("the fixture file", &table, &SET_KEYS)?;

    let judged_under = table
        .get("judged_under")
        .and_then(|value| value.as_table())
        .ok_or("the fixture file has no [judged_under] table")?;
    refuse_unknown_keys("[judged_under]", judged_under, &JUDGED_UNDER_KEYS)?;
    let working_directory = text_or(judged_under, "working_directory", "/");

    let entries = table
        .get("fixture")
        .and_then(|value| value.as_array())
        .ok_or("the fixture file has no [[fixture]] entries")?;
    let fixtures = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| parse_fixture(index + 1, entry, &working_directory))
        .collect::<Result<Vec<Fixture>, String>>()?;
    refuse_repeated_names(&fixtures)?;

    Ok(FixtureSet {
        arguments: text_or(judged_under, "arguments", "").split_whitespace().map(str::to_string).collect(),
        allow_paths: text_or(judged_under, "allow_paths", ""),
        home: text_or(judged_under, "home", "/home/user"),
        fixtures,
    })
}

fn refuse_unknown_keys(place: &str, table: &toml::Table, known: &[&str]) -> Result<(), String> {
    match table.keys().find(|key| !known.contains(&key.as_str())) {
        Some(unknown) => Err(format!("{place} has a key this reader does not know: {unknown}")),
        None => Ok(()),
    }
}

fn text_or(table: &toml::Table, key: &str, otherwise: &str) -> String {
    table.get(key).and_then(|value| value.as_str()).unwrap_or(otherwise).to_string()
}

fn required_text(place: &str, table: &toml::Table, key: &str) -> Result<String, String> {
    match table.get(key).and_then(|value| value.as_str()) {
        Some(text) if !text.is_empty() => Ok(text.to_string()),
        _ => Err(format!("{place} has no {key}")),
    }
}

fn parse_fixture(number: usize, entry: &toml::Value, set_directory: &str) -> Result<Fixture, String> {
    let numbered = format!("fixture {number}");
    let fields = entry.as_table().ok_or_else(|| format!("{numbered} is not a table"))?;
    refuse_unknown_keys(&numbered, fields, &FIXTURE_KEYS)?;
    let name = required_text(&numbered, fields, "name")?;
    let place = format!("fixture {number} ({name})");

    let written_verdict = required_text(&place, fields, "verdict")?;
    let verdict = parse_verdict(&written_verdict)
        .ok_or_else(|| format!("{place} has a verdict that is not allow, warn, ask or deny: {written_verdict}"))?;
    let category = fields.get("category").and_then(|value| value.as_str()).map(str::to_string);
    match (verdict, &category) {
        (Verdict::Allow, Some(_)) => return Err(format!("{place} is allowed and states a category")),
        (Verdict::Allow, None) | (_, Some(_)) => {}
        (_, None) => return Err(format!("{place} is not allowed and states no category")),
    }

    Ok(Fixture {
        call: parse_call(&place, fields)?,
        working_directory: text_or(fields, "working_directory", set_directory),
        name,
        verdict,
        category,
    })
}

fn parse_call(place: &str, fields: &toml::Table) -> Result<FixtureCall, String> {
    let command = fields.get("command").and_then(|value| value.as_str());
    let tool = fields.get("tool").and_then(|value| value.as_str());
    let tool_input = fields.get("tool_input").and_then(|value| value.as_table());
    match (command, tool, tool_input) {
        (Some(command), None, None) => Ok(FixtureCall::Command(command.to_string())),
        (None, Some(tool), Some(tool_input)) => {
            Ok(FixtureCall::Tool { tool: tool.to_string(), tool_input: tool_input.clone() })
        }
        _ => Err(format!("{place} states neither one command nor one tool with its tool_input")),
    }
}

fn refuse_repeated_names(fixtures: &[Fixture]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    match fixtures.iter().find(|fixture| !seen.insert(fixture.name.as_str())) {
        Some(repeated) => Err(format!("two fixtures are named: {}", repeated.name)),
        None => Ok(()),
    }
}

/// The verdict and category a decision amounts to.
pub fn verdict_of(decision: &HookDecision) -> (Verdict, Option<&str>) {
    match decision {
        HookDecision::Allow | HookDecision::AllowWithContext { .. } => (Verdict::Allow, None),
        HookDecision::Warn { category, .. } => (Verdict::Warn, Some(category)),
        HookDecision::Ask { category, .. } => (Verdict::Ask, Some(category)),
        HookDecision::Deny { category, .. } => (Verdict::Deny, Some(category)),
    }
}

fn shown(verdict: Verdict, category: Option<&str>) -> String {
    match category {
        Some(category) => format!("{} ({category})", verdict.written()),
        None => verdict.written().to_string(),
    }
}

/// How a hook's decision differs from what a fixture expects; `None` when it
/// is the expected verdict in the expected category.
pub fn mismatch(fixture: &Fixture, decision: &HookDecision) -> Option<String> {
    let (verdict, category) = verdict_of(decision);
    let as_expected = verdict == fixture.verdict && category == fixture.category.as_deref();
    (!as_expected).then(|| {
        format!(
            "{}\n    expected {}\n    actual   {}",
            fixture.name,
            shown(fixture.verdict, fixture.category.as_deref()),
            shown(verdict, category)
        )
    })
}

/// Every mismatch of a fixture run, as one text.
pub fn mismatch_report(mismatches: &[String], judged: usize) -> String {
    format!("{} of {} fixtures do not get their verdict:\n\n{}", mismatches.len(), judged, mismatches.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
[judged_under]
arguments = "--chaining block --probing warn"
allow_paths = "/n/"
home = "/home/tester"
working_directory = "/work/project"

[[fixture]]
name = "a semicolon chain"
command = "echo a; echo b"
verdict = "deny"
category = "chaining"
found = "2026-11-17, in a site-tools session"

[[fixture]]
name = "one command"
command = "ls -la"
verdict = "allow"

[[fixture]]
name = "a glob into the floor"
tool = "Glob"
tool_input = { pattern = "/home/tester/.ssh/*" }
working_directory = "/elsewhere"
verdict = "deny"
category = "floor"
"#;

    fn deny(category: &str) -> HookDecision {
        HookDecision::Deny { category: category.into(), event: String::new(), reason: String::new() }
    }

    #[test]
    fn a_good_file_is_read() {
        let set = parse_fixture_set(GOOD).unwrap();
        assert_eq!(set.arguments, vec!["--chaining", "block", "--probing", "warn"]);
        assert_eq!(set.allow_paths, "/n/");
        assert_eq!(set.home, "/home/tester");
        assert_eq!(set.fixtures.len(), 3);
        assert_eq!(set.fixtures[0].call, FixtureCall::Command("echo a; echo b".to_string()));
        assert_eq!(set.fixtures[0].working_directory, "/work/project");
        assert_eq!(set.fixtures[2].working_directory, "/elsewhere");
        assert!(matches!(&set.fixtures[2].call, FixtureCall::Tool { tool, .. } if tool == "Glob"));
    }

    fn with_fixture(fixture: &str) -> Result<FixtureSet, String> {
        parse_fixture_set(&format!("[judged_under]\narguments = \"\"\n\n[[fixture]]\n{fixture}\n"))
    }

    #[test]
    fn a_fixture_without_a_name_or_a_verdict_is_refused() {
        assert!(with_fixture("command = \"ls\"\nverdict = \"allow\"").unwrap_err().contains("no name"));
        assert!(with_fixture("name = \"x\"\ncommand = \"ls\"").unwrap_err().contains("no verdict"));
    }

    #[test]
    fn a_misspelled_verdict_or_key_is_refused() {
        assert!(with_fixture("name = \"x\"\ncommand = \"ls\"\nverdict = \"alow\"").is_err());
        assert!(with_fixture("name = \"x\"\ncomand = \"ls\"\nverdict = \"allow\"").unwrap_err().contains("comand"));
    }

    #[test]
    fn a_refusal_without_a_category_and_an_allow_with_one_are_refused() {
        assert!(with_fixture("name = \"x\"\ncommand = \"ls\"\nverdict = \"deny\"").unwrap_err().contains("no category"));
        let allowed_with_category = "name = \"x\"\ncommand = \"ls\"\nverdict = \"allow\"\ncategory = \"floor\"";
        assert!(with_fixture(allowed_with_category).is_err());
    }

    #[test]
    fn a_fixture_must_state_exactly_one_call() {
        assert!(with_fixture("name = \"x\"\nverdict = \"allow\"").is_err());
        assert!(with_fixture("name = \"x\"\ncommand = \"ls\"\ntool = \"Read\"\nverdict = \"allow\"").is_err());
        assert!(with_fixture("name = \"x\"\ntool = \"Read\"\nverdict = \"allow\"").is_err());
    }

    #[test]
    fn a_name_used_twice_is_refused() {
        let twice = "name = \"x\"\ncommand = \"ls\"\nverdict = \"allow\"\n\n[[fixture]]\nname = \"x\"\ncommand = \"pwd\"\nverdict = \"allow\"";
        assert!(with_fixture(twice).unwrap_err().contains("two fixtures are named"));
    }

    #[test]
    fn a_file_without_fixtures_or_what_it_is_judged_under_is_refused() {
        assert!(parse_fixture_set("[judged_under]\narguments = \"\"\n").is_err());
        assert!(parse_fixture_set("[[fixture]]\nname = \"x\"\ncommand = \"ls\"\nverdict = \"allow\"\n").is_err());
        assert!(parse_fixture_set("[[[broken").is_err());
    }

    #[test]
    fn the_expected_decision_is_no_mismatch() {
        let set = parse_fixture_set(GOOD).unwrap();
        assert!(mismatch(&set.fixtures[0], &deny("chaining")).is_none());
        assert!(mismatch(&set.fixtures[1], &HookDecision::Allow).is_none());
    }

    #[test]
    fn another_verdict_or_another_category_is_a_mismatch() {
        let set = parse_fixture_set(GOOD).unwrap();
        let allowed = mismatch(&set.fixtures[0], &HookDecision::Allow).unwrap();
        assert!(allowed.contains("a semicolon chain"));
        assert!(allowed.contains("expected deny (chaining)"));
        assert!(allowed.contains("actual   allow"));
        assert!(mismatch(&set.fixtures[0], &deny("subversion")).unwrap().contains("actual   deny (subversion)"));
        assert!(mismatch(&set.fixtures[1], &deny("floor")).is_some());
    }

    #[test]
    fn the_report_counts_and_lists_every_mismatch() {
        let report = mismatch_report(&["first".to_string(), "second".to_string()], 40);
        assert!(report.starts_with("2 of 40 fixtures"));
        assert!(report.contains("first") && report.contains("second"));
    }
}
