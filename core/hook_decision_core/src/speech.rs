//! What is spoken aloud for a decision: one phrase per category.

use crate::UNDECIDED;

/// The decision kinds that are spoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spoken {
    Deny,
    Ask,
    Warn,
}

fn category_phrase(category: &str) -> &'static str {
    match category {
        "floor" => "access sensitive credentials",
        "probing" => "probing the guardrail settings",
        "subversion" => "subvert the safety controls",
        "truncation" => "reading constraint files selectively",
        "evasion" => "evade the safety controls",
        "destruction" => "destroy uncommitted work",
        "revert" => "revert file changes",
        "workflow" => "build outside the deploy tool",
        "chaining" => "chain shell commands",
        "path" => "access a restricted path",
        "bash" => "run a restricted command",
        _ => "do something unexpected",
    }
}

/// The sentence spoken for a decision in a category.
pub fn sentence(spoken: Spoken, category: &str) -> String {
    if category == UNDECIDED {
        return undecided_sentence(spoken);
    }
    let phrase = category_phrase(category);
    match spoken {
        Spoken::Deny => {
            let prefix = match category {
                "floor" | "subversion" | "chaining" | "destruction" => "DANGER",
                _ => "WARNING",
            };
            format!("{prefix}: Claude is trying to {phrase} -- BLOCKED.")
        }
        Spoken::Ask => format!("ATTENTION: Claude is trying to {phrase} -- Awaiting your decision."),
        Spoken::Warn => format!("WARNING: Claude is {phrase} -- Correction provided."),
    }
}

fn undecided_sentence(spoken: Spoken) -> String {
    match spoken {
        Spoken::Deny => "DANGER: a security hook could not examine a call -- BLOCKED.".to_string(),
        _ => "DANGER: a security hook could not examine a call -- Awaiting your decision.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_denied_floor_access_is_danger() {
        assert_eq!(
            sentence(Spoken::Deny, "floor"),
            "DANGER: Claude is trying to access sensitive credentials -- BLOCKED."
        );
    }

    #[test]
    fn a_denied_probe_is_a_warning() {
        assert!(sentence(Spoken::Deny, "probing").starts_with("WARNING:"));
    }

    #[test]
    fn every_category_a_hook_decides_in_has_its_own_phrase() {
        let categories = [
            "floor", "probing", "subversion", "truncation", "evasion", "destruction", "revert", "workflow",
            "chaining", "path", "bash",
        ];
        for category in categories {
            assert!(
                !sentence(Spoken::Ask, category).contains("something unexpected"),
                "{category} has no phrase"
            );
        }
    }

    #[test]
    fn an_undecided_hook_says_so() {
        assert!(sentence(Spoken::Ask, UNDECIDED).contains("could not examine"));
        assert!(sentence(Spoken::Deny, UNDECIDED).ends_with("BLOCKED."));
    }
}
