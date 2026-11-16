//! The command line of a rule-driven hook: `--category severity` pairs.
//!
//! Read strictly. An argument that is not a category the hook reads, a category
//! given twice, a category with no severity after it, or a severity that is not
//! `warn`, `ask` or `block` is an error; no severities are returned.

use std::collections::BTreeMap;

use crate::{parse_severity, Severity};

/// The severity each category was given. A category not on the command line is
/// absent, and the hook does not examine it.
pub type CategorySeverities = BTreeMap<String, Severity>;

pub fn parse_category_flags(args: &[String], categories: &[&str]) -> Result<CategorySeverities, String> {
    let mut severities = CategorySeverities::new();
    let mut remaining = args.iter();
    while let Some(flag) = remaining.next() {
        let category = flag
            .strip_prefix("--")
            .filter(|name| categories.contains(name))
            .ok_or_else(|| format!("{flag} is not a category this hook reads"))?;
        let written = remaining.next().ok_or_else(|| format!("{flag} has no severity after it"))?;
        let severity = parse_severity(written)
            .ok_or_else(|| format!("{flag} has a severity that is not warn, ask or block: {written}"))?;
        if severities.insert(category.to_string(), severity).is_some() {
            return Err(format!("{flag} is given twice"));
        }
    }
    Ok(severities)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATEGORIES: [&str; 2] = ["probing", "chaining"];

    fn parsed(args: &[&str]) -> Result<CategorySeverities, String> {
        let owned: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        parse_category_flags(&owned, &CATEGORIES)
    }

    #[test]
    fn pairs_are_read() {
        let severities = parsed(&["--probing", "warn", "--chaining", "block"]).unwrap();
        assert_eq!(severities.get("probing"), Some(&Severity::Warn));
        assert_eq!(severities.get("chaining"), Some(&Severity::Block));
    }

    #[test]
    fn no_arguments_gives_no_severities() {
        assert!(parsed(&[]).unwrap().is_empty());
    }

    #[test]
    fn a_category_left_out_is_absent() {
        assert!(!parsed(&["--probing", "ask"]).unwrap().contains_key("chaining"));
    }

    #[test]
    fn an_unknown_category_is_refused() {
        assert!(parsed(&["--gaming", "warn"]).unwrap_err().contains("--gaming"));
    }

    #[test]
    fn a_misspelled_severity_is_refused() {
        assert!(parsed(&["--probing", "blok"]).unwrap_err().contains("blok"));
    }

    #[test]
    fn a_category_without_a_severity_is_refused() {
        assert!(parsed(&["--probing"]).unwrap_err().contains("no severity"));
    }

    #[test]
    fn a_bare_word_is_refused() {
        assert!(parsed(&["probing", "warn"]).is_err());
    }

    #[test]
    fn a_category_given_twice_is_refused() {
        assert!(parsed(&["--probing", "warn", "--probing", "block"]).unwrap_err().contains("twice"));
    }
}
