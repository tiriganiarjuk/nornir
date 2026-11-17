//! The decision vocabulary of the Claude Code security hooks. Pure: no IO.
//!
//! A hook binary reads its input through `hook_io`, builds its configuration
//! from the modules here, and returns a `HookDecision`:
//!
//!   lib    — `Severity`, `HookDecision`, and the one mapping from a matched
//!            rule to a decision (`make_decision`)
//!   rules  — the rules file format, read strictly, and the one way a rule is
//!            matched against a subject
//!   flags  — the `--category severity` command line, read strictly
//!   fixtures — fixture files: concrete calls and the verdict each must get
//!   paths  — lexical path normalisation, applied before any path is matched
//!   shape  — whether a command is one command, one in a directory, or several
//!   shell  — a command line read as text: its structure, the paths it names
//!   speech — what is spoken for a decision
//!
//! Strict means: a rule, a flag or a severity that cannot be read is an error
//! the hook reports. Nothing is skipped.

pub mod fixtures;
pub mod flags;
pub mod paths;
pub mod rules;
pub mod shape;
pub mod shell;
pub mod speech;

/// How a matched rule is enforced, in ascending order.
///
///   Warn  — the call runs; the user and the session are told
///   Ask   — the call waits; the user allows or denies it
///   Block — the call is denied
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warn,
    Ask,
    Block,
}

/// Read a severity from its written form: `warn`, `ask` or `block`.
pub fn parse_severity(level: &str) -> Option<Severity> {
    match level {
        "warn" => Some(Severity::Warn),
        "ask" => Some(Severity::Ask),
        "block" => Some(Severity::Block),
        _ => None,
    }
}

/// What a hook decided about one tool call.
#[derive(Debug)]
pub enum HookDecision {
    /// The call runs, with nothing said.
    Allow,
    /// The call runs, and the session is given context. No warning is raised.
    AllowWithContext { context: String },
    /// The call runs; the user and the session are both told.
    Warn {
        category: String,
        event: String,
        user_reason: String,
        llm_context: String,
    },
    /// The call waits for the user to allow or deny it.
    Ask {
        category: String,
        event: String,
        reason: String,
        llm_context: String,
    },
    /// The call is denied; the reason is shown to the session.
    Deny {
        category: String,
        event: String,
        reason: String,
    },
}

/// The category every decision carries when the hook itself could not decide:
/// its input, its rules or its command line could not be read, or it failed.
pub const UNDECIDED: &str = "undecided";

/// The decision of an interactive hook that could not decide: the user does.
pub fn ask_because_undecided(hook: &str, cause: &str) -> HookDecision {
    HookDecision::Ask {
        category: UNDECIDED.into(),
        event: format!("{hook} could not examine this call"),
        reason: format!("{hook} could not examine this call: {cause}. Allow or deny?"),
        llm_context: format!(
            "The security hook {hook} could not examine this call ({cause}). \
             The user is being asked whether to allow it. Do NOT retry without permission."
        ),
    }
}

/// The decision of a sandbox hook that could not decide: the call is denied.
pub fn deny_because_undecided(hook: &str, cause: &str) -> HookDecision {
    HookDecision::Deny {
        category: UNDECIDED.into(),
        event: format!("{hook} could not examine this call"),
        reason: format!("{hook} could not examine this call: {cause}. The call is denied."),
    }
}

/// A matched rule and the subject it matched, ready to become a decision.
pub struct DecisionInput<'a> {
    pub severity: Severity,
    pub category: &'a str,
    pub description: &'a str,
    pub subject: &'a str,
    /// Past tense for the subject: "accessed" (paths) or "ran" (commands).
    pub verb_past: &'a str,
    /// Present tense: "access" (paths) or "run" (commands).
    pub verb_present: &'a str,
    /// The subject is shown shortened to this many characters when set.
    pub max_subject_chars: Option<usize>,
}

/// The subject as shown: whole, or its first characters followed by `...`.
/// Counted and cut in characters, so any text can be shortened.
pub fn shorten(subject: &str, max_chars: Option<usize>) -> String {
    match max_chars {
        Some(max) if subject.chars().count() > max => {
            let kept: String = subject.chars().take(max.saturating_sub(3)).collect();
            format!("{kept}...")
        }
        _ => subject.to_string(),
    }
}

/// The one mapping from a matched rule's severity to a decision.
pub fn make_decision(input: &DecisionInput) -> HookDecision {
    let short = shorten(input.subject, input.max_subject_chars);
    let event = format!("{} \u{2014} {}", input.description.to_lowercase(), short);

    match input.severity {
        Severity::Block => HookDecision::Deny {
            category: input.category.into(),
            event,
            reason: format!(
                "{} blocked ({}: {}).\n{}",
                blocked_label(input.verb_present, &short),
                input.category,
                input.description,
                short
            ),
        },
        Severity::Ask => HookDecision::Ask {
            category: input.category.into(),
            event,
            reason: format!(
                "LLM wants to {} '{}' ({}: {}). Allow or deny?",
                input.verb_present, short, input.category, input.description
            ),
            llm_context: format!(
                "Your command requires user approval ({}: {}). \
                 The user is being asked whether to allow this. \
                 Do NOT retry without permission.",
                input.category, input.description
            ),
        },
        Severity::Warn => HookDecision::Warn {
            category: input.category.into(),
            event,
            user_reason: format!(
                "LLM {} '{}' ({}: {}). Behavior flagged.",
                input.verb_past, short, input.category, input.description
            ),
            llm_context: format!(
                "Your {} to '{}' was flagged as {} behavior ({}). \
                 The user has been notified. If this {} is necessary \
                 for your task, explain why to the user.",
                input.verb_present, short, input.category, input.description, input.verb_present
            ),
        },
    }
}

fn blocked_label(verb_present: &str, short: &str) -> String {
    if verb_present == "run" {
        "Command".to_string()
    } else {
        format!("Access to '{short}'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(severity: Severity, subject: &str, max: Option<usize>) -> HookDecision {
        make_decision(&DecisionInput {
            severity,
            category: "subversion",
            description: "Deleting lock files",
            subject,
            verb_past: "ran",
            verb_present: "run",
            max_subject_chars: max,
        })
    }

    #[test]
    fn severity_reads_its_three_written_forms() {
        assert_eq!(parse_severity("warn"), Some(Severity::Warn));
        assert_eq!(parse_severity("ask"), Some(Severity::Ask));
        assert_eq!(parse_severity("block"), Some(Severity::Block));
    }

    #[test]
    fn severity_refuses_other_spellings() {
        for other in ["", "WARN", "Warn", "deny", "blok", " block"] {
            assert_eq!(parse_severity(other), None, "{other:?}");
        }
    }

    #[test]
    fn shorten_keeps_a_subject_at_the_limit_whole() {
        let subject = "a".repeat(60);
        assert_eq!(shorten(&subject, Some(60)), subject);
    }

    #[test]
    fn shorten_cuts_a_subject_over_the_limit() {
        let shown = shorten(&"a".repeat(61), Some(60));
        assert_eq!(shown.chars().count(), 60);
        assert!(shown.ends_with("..."));
    }

    #[test]
    fn shorten_cuts_between_characters_of_any_width() {
        // 56 one-byte characters, then four-byte characters across the cut.
        let subject = format!("{}{}", "a".repeat(56), "\u{1F512}".repeat(10));
        let shown = shorten(&subject, Some(60));
        assert_eq!(shown.chars().count(), 60);
        assert!(shown.starts_with(&"a".repeat(56)));
        assert!(shown.ends_with("\u{1F512}..."));
    }

    #[test]
    fn shorten_without_a_limit_changes_nothing() {
        let subject = "\u{00E9}".repeat(500);
        assert_eq!(shorten(&subject, None), subject);
    }

    #[test]
    fn block_denies_a_multibyte_command() {
        let command = format!("rm {}.lock", "\u{00E9}".repeat(80));
        match decision(Severity::Block, &command, Some(60)) {
            HookDecision::Deny { category, reason, .. } => {
                assert_eq!(category, "subversion");
                assert!(reason.contains("blocked"));
            }
            other => panic!("block must deny, got {other:?}"),
        }
    }

    #[test]
    fn ask_asks_and_warn_warns() {
        assert!(matches!(decision(Severity::Ask, "rm x.lock", None), HookDecision::Ask { .. }));
        assert!(matches!(decision(Severity::Warn, "rm x.lock", None), HookDecision::Warn { .. }));
    }

    #[test]
    fn a_blocked_path_names_the_path() {
        let d = make_decision(&DecisionInput {
            severity: Severity::Block,
            category: "probing",
            description: "Hook scripts",
            subject: "/home/.claude/hooks/x",
            verb_past: "accessed",
            verb_present: "access",
            max_subject_chars: None,
        });
        match d {
            HookDecision::Deny { reason, .. } => assert!(reason.contains("Access to '/home/.claude/hooks/x'")),
            other => panic!("block must deny, got {other:?}"),
        }
    }

    #[test]
    fn an_undecided_interactive_hook_asks() {
        match ask_because_undecided("hook_pre_llm_bash", "its input is not JSON") {
            HookDecision::Ask { category, reason, .. } => {
                assert_eq!(category, UNDECIDED);
                assert!(reason.contains("its input is not JSON"));
            }
            other => panic!("must ask, got {other:?}"),
        }
    }

    #[test]
    fn an_undecided_sandbox_hook_denies() {
        assert!(matches!(
            deny_because_undecided("hook_pre_subagent_bash", "its input is not JSON"),
            HookDecision::Deny { .. }
        ));
    }
}
