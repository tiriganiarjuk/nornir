//! The rules file format, and the one way a rule is matched.
//!
//! A rules file is TOML: one array of tables per category, each entry a
//! `pattern`, a `description`, and optionally a `severity` that replaces the
//! category's own for that rule.
//!
//! The file is read strictly. A category the reader was not told to expect, an
//! entry without its two required texts, a key that is not one of the three, a
//! severity that is not `warn`, `ask` or `block`, a pattern that does not
//! compile: each is an error naming the entry, and no rule set is returned.

use std::collections::BTreeMap;

use regex::Regex;

use crate::paths::{for_matching, with_pattern_as};
use crate::{make_decision, parse_severity, DecisionInput, HookDecision, Severity};

const ENTRY_KEYS: [&str; 3] = ["pattern", "description", "severity"];

/// How a rule's pattern is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternKind {
    /// A regular expression, matched anywhere in the subject.
    Regex,
    /// Literal text, matched anywhere in the subject.
    Literal,
}

/// One rule, ready to match.
#[derive(Debug)]
pub struct Rule {
    pub pattern: String,
    pub description: String,
    pub severity: Option<Severity>,
    matcher: Regex,
}

impl Rule {
    pub fn is_match(&self, subject: &str) -> bool {
        self.matcher.is_match(subject)
    }
}

/// Every category of a rules file, each with its rules in file order.
pub type RuleSet = BTreeMap<String, Vec<Rule>>;

/// Read a rules file. `categories` names every category the file may hold and
/// how its patterns are written; each is present in the result, empty when the
/// file has no entry for it.
pub fn parse_rule_set(toml_str: &str, categories: &[(&str, PatternKind)]) -> Result<RuleSet, String> {
    let table: toml::Table = toml_str
        .parse()
        .map_err(|toml_err| format!("the rules file is not TOML \u{2014} {toml_err}"))?;

    if let Some(unknown) = table.keys().find(|key| !categories.iter().any(|(name, _)| name == key)) {
        return Err(format!("the rules file has a category this hook does not read: [[{unknown}]]"));
    }

    let mut rule_set = RuleSet::new();
    for (category, kind) in categories {
        let rules = match table.get(*category) {
            Some(value) => parse_category(category, value, *kind)?,
            None => Vec::new(),
        };
        rule_set.insert((*category).to_string(), rules);
    }
    Ok(rule_set)
}

fn parse_category(category: &str, value: &toml::Value, kind: PatternKind) -> Result<Vec<Rule>, String> {
    let entries = value
        .as_array()
        .ok_or_else(|| format!("[[{category}]] is not an array of rules"))?;
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| parse_rule(&format!("[[{category}]] rule {}", index + 1), entry, kind))
        .collect()
}

fn parse_rule(place: &str, entry: &toml::Value, kind: PatternKind) -> Result<Rule, String> {
    let fields = entry.as_table().ok_or_else(|| format!("{place} is not a table"))?;
    if let Some(unknown) = fields.keys().find(|key| !ENTRY_KEYS.contains(&key.as_str())) {
        return Err(format!("{place} has a key that is not pattern, description or severity: {unknown}"));
    }
    let pattern = required_text(place, fields, "pattern")?;
    let description = required_text(place, fields, "description")?;
    let severity = match fields.get("severity") {
        None => None,
        Some(value) => Some(written_severity(place, value)?),
    };
    let matcher = compile(place, &pattern, kind)?;
    Ok(Rule { pattern, description, severity, matcher })
}

fn required_text(place: &str, fields: &toml::Table, key: &str) -> Result<String, String> {
    match fields.get(key).and_then(|value| value.as_str()) {
        Some(text) if !text.is_empty() => Ok(text.to_string()),
        _ => Err(format!("{place} has no {key}")),
    }
}

fn written_severity(place: &str, value: &toml::Value) -> Result<Severity, String> {
    value
        .as_str()
        .and_then(parse_severity)
        .ok_or_else(|| format!("{place} has a severity that is not warn, ask or block: {value}"))
}

fn compile(place: &str, pattern: &str, kind: PatternKind) -> Result<Regex, String> {
    let source = match kind {
        PatternKind::Regex => pattern.to_string(),
        PatternKind::Literal => regex::escape(pattern),
    };
    Regex::new(&source).map_err(|regex_err| format!("{place} has a pattern that does not compile \u{2014} {regex_err}"))
}

/// The rules of one category. A category the set was not built with has none.
pub fn rules_of<'a>(rule_set: &'a RuleSet, category: &str) -> &'a [Rule] {
    rule_set.get(category).map(Vec::as_slice).unwrap_or(&[])
}

/// The first rule that matches the subject, in file order.
pub fn first_match<'a>(rules: &'a [Rule], subject: &str) -> Option<&'a Rule> {
    rules.iter().find(|rule| rule.is_match(subject))
}

/// The first protected-path rule a path reaches, in file order: a rule whose
/// literal pattern the path holds, or one a pattern segment of the path can
/// match. The path is a normalised path; it is matched as `for_matching`
/// gives it.
pub fn first_match_for_path<'a>(rules: &'a [Rule], path: &str) -> Option<&'a Rule> {
    let matched = for_matching(path);
    rules
        .iter()
        .find(|rule| rule.is_match(&matched) || with_pattern_as(&matched, &rule.pattern).is_some())
}

/// What is examined, and how a decision about it is worded.
pub struct Subject<'a> {
    pub text: &'a str,
    pub verb_past: &'a str,
    pub verb_present: &'a str,
    pub max_shown_chars: Option<usize>,
}

/// A command line, as the bash hooks word it.
pub fn command_subject(command: &str) -> Subject<'_> {
    Subject { text: command, verb_past: "ran", verb_present: "run", max_shown_chars: Some(60) }
}

/// A path, as the file-tool hooks word it.
pub fn path_subject(path: &str) -> Subject<'_> {
    Subject { text: path, verb_past: "accessed", verb_present: "access", max_shown_chars: None }
}

/// The decision of one category about one subject: the first matching rule's,
/// at the rule's own severity where it states one, else the category's.
pub fn decide_category(
    rules: &[Rule],
    subject: &Subject,
    category: &str,
    category_severity: Severity,
) -> Option<HookDecision> {
    first_match(rules, subject.text).map(|rule| {
        make_decision(&DecisionInput {
            severity: rule.severity.unwrap_or(category_severity),
            category,
            description: &rule.description,
            subject: subject.text,
            verb_past: subject.verb_past,
            verb_present: subject.verb_present,
            max_subject_chars: subject.max_shown_chars,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: [(&str, PatternKind); 2] = [("floor", PatternKind::Literal), ("subversion", PatternKind::Regex)];

    fn parsed(toml_str: &str) -> Result<RuleSet, String> {
        parse_rule_set(toml_str, &BOTH)
    }

    const GOOD: &str = r#"
[[floor]]
pattern = "/.ssh/"
description = "SSH keys"

[[subversion]]
pattern = "chflags.*noschg"
description = "Removing system immutable flag"
severity = "ask"

[[subversion]]
pattern = "flock.*--unlock"
description = "Unlocking file locks"
"#;

    #[test]
    fn a_good_file_gives_every_category_in_order() {
        let rule_set = parsed(GOOD).unwrap();
        assert_eq!(rules_of(&rule_set, "floor").len(), 1);
        let subversion = rules_of(&rule_set, "subversion");
        assert_eq!(subversion[0].pattern, "chflags.*noschg");
        assert_eq!(subversion[0].severity, Some(Severity::Ask));
        assert_eq!(subversion[1].severity, None);
    }

    #[test]
    fn a_category_with_no_entry_is_present_and_empty() {
        let rule_set = parsed("[[floor]]\npattern = \"/x/\"\ndescription = \"x\"\n").unwrap();
        assert!(rules_of(&rule_set, "subversion").is_empty());
    }

    #[test]
    fn a_category_never_asked_for_has_no_rules() {
        assert!(rules_of(&parsed(GOOD).unwrap(), "gaming").is_empty());
    }

    #[test]
    fn text_that_is_not_toml_is_refused() {
        assert!(parsed("[[[broken").unwrap_err().contains("not TOML"));
    }

    #[test]
    fn a_misspelled_category_is_refused() {
        let err = parsed("[[subverison]]\npattern = \"x\"\ndescription = \"x\"\n").unwrap_err();
        assert!(err.contains("[[subverison]]"), "{err}");
    }

    #[test]
    fn a_rule_without_a_pattern_is_refused() {
        let err = parsed("[[floor]]\ndescription = \"x\"\n").unwrap_err();
        assert!(err.contains("[[floor]] rule 1 has no pattern"), "{err}");
    }

    #[test]
    fn a_rule_without_a_description_is_refused() {
        let err = parsed("[[floor]]\npattern = \"x\"\n").unwrap_err();
        assert!(err.contains("has no description"), "{err}");
    }

    #[test]
    fn a_rule_with_an_empty_pattern_is_refused() {
        assert!(parsed("[[floor]]\npattern = \"\"\ndescription = \"x\"\n").is_err());
    }

    #[test]
    fn a_misspelled_severity_is_refused() {
        let err = parsed("[[floor]]\npattern = \"x\"\ndescription = \"x\"\nseverity = \"blok\"\n").unwrap_err();
        assert!(err.contains("not warn, ask or block"), "{err}");
    }

    #[test]
    fn a_misspelled_key_is_refused() {
        let err = parsed("[[floor]]\npattern = \"x\"\ndescription = \"x\"\nseverty = \"block\"\n").unwrap_err();
        assert!(err.contains("severty"), "{err}");
    }

    #[test]
    fn a_pattern_that_does_not_compile_is_refused() {
        let err = parsed("[[subversion]]\npattern = \"(unclosed\"\ndescription = \"x\"\n").unwrap_err();
        assert!(err.contains("[[subversion]] rule 1") && err.contains("does not compile"), "{err}");
    }

    #[test]
    fn a_category_that_is_not_an_array_is_refused() {
        assert!(parsed("[floor]\npattern = \"x\"\n").unwrap_err().contains("not an array"));
    }

    #[test]
    fn a_literal_pattern_matches_its_text_and_nothing_else() {
        let rule_set = parsed("[[floor]]\npattern = \"/.docker/config.json\"\ndescription = \"x\"\n").unwrap();
        let rule = &rules_of(&rule_set, "floor")[0];
        assert!(rule.is_match("/home/u/.docker/config.json"));
        assert!(!rule.is_match("/home/u/.docker/configxjson"));
    }

    #[test]
    fn a_regex_pattern_matches_as_a_regex() {
        let rule_set = parsed(GOOD).unwrap();
        let rule = &rules_of(&rule_set, "subversion")[0];
        assert!(rule.is_match("sudo chflags -R noschg x"));
        assert!(!rule.is_match("chflags schg x"));
    }

    #[test]
    fn a_path_reaches_a_rule_it_spells_or_one_its_pattern_can_match() {
        let rule_set = parsed(GOOD).unwrap();
        let floor = rules_of(&rule_set, "floor");
        for reaching in ["/home/u/.ssh/id_rsa", "/home/u/.SSH", "/home/u/.s*/id_*", "/home/u/.ss?"] {
            assert!(first_match_for_path(floor, reaching).is_some(), "{reaching}");
        }
        for elsewhere in ["/home/u/.sshrc", "/home/u/src/*.rs", "/home/u/*", "/home/u/.a*/x"] {
            assert!(first_match_for_path(floor, elsewhere).is_none(), "{elsewhere}");
        }
    }

    #[test]
    fn the_first_matching_rule_decides() {
        let rule_set = parsed(GOOD).unwrap();
        let rules = rules_of(&rule_set, "subversion");
        let decision = decide_category(rules, &command_subject("chflags noschg x"), "subversion", Severity::Block);
        assert!(matches!(decision, Some(HookDecision::Ask { .. })), "the rule's own severity applies");
        let decision = decide_category(rules, &command_subject("flock --unlock x"), "subversion", Severity::Block);
        assert!(matches!(decision, Some(HookDecision::Deny { .. })), "the category's severity applies");
    }

    #[test]
    fn a_subject_no_rule_matches_gets_no_decision() {
        let rule_set = parsed(GOOD).unwrap();
        let rules = rules_of(&rule_set, "subversion");
        assert!(decide_category(rules, &command_subject("ls -la"), "subversion", Severity::Block).is_none());
    }

    #[test]
    fn a_path_subject_is_shown_whole() {
        let rule_set = parsed(GOOD).unwrap();
        let path = format!("/home/u/.ssh/{}", "k".repeat(200));
        let decision = decide_category(rules_of(&rule_set, "floor"), &path_subject(&path), "floor", Severity::Block);
        match decision {
            Some(HookDecision::Deny { reason, .. }) => assert!(reason.contains(&path)),
            other => panic!("must deny, got {other:?}"),
        }
    }
}
