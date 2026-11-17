//! The IO of a Claude Code hook binary: read the call from stdin, hand it to
//! the hook's pure `decide` function, write the reply, and tell the user.
//!
//!   run_pre_hook / run_post_hook — the entry points
//!   response                     — one reply type per hook event
//!
//! The decision vocabulary (`HookDecision`, severities, rules) is
//! `hook_decision_core`. This crate holds no decision logic.
//!
//! A hook always exits 0: Claude Code reads the decision from stdout. A pre
//! hook that cannot decide — its input unreadable or not the payload its
//! schema describes, or `decide` itself failing — still replies, with what its
//! caller chose for that case (`WhenUndecided`).
//!
//! Warn, ask and deny decisions are emitted to Hlidskjalf as alert datagrams.

pub mod response;

use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::ExitCode;

use hook_decision_core::paths::{absolute_from, normalize};
use hook_decision_core::rules::{parse_rule_set, rules_of, PatternKind, RuleSet};
use hook_decision_core::speech::{sentence, Spoken};
use hook_decision_core::{ask_because_undecided, deny_because_undecided, HookDecision, UNDECIDED};
use serde::Deserialize;

/// A PreToolUse call, as far as the hooks read it. Its shape on the wire is
/// `schemas/tools/hook-pre-tool-use.schema.json`.
#[derive(Debug, Deserialize)]
pub struct HookInput {
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    /// The directory the session is working in.
    #[serde(default)]
    pub cwd: Option<String>,
}

impl HookInput {
    /// A call to `tool_name` with `tool_input`, from no stated directory.
    pub fn for_tool(tool_name: &str, tool_input: serde_json::Value) -> Self {
        Self { tool_name: tool_name.to_string(), tool_input, cwd: None }
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.tool_input.get(key).and_then(|value| value.as_str())
    }

    /// The file a file tool was called on: its `file_path`.
    pub fn file_path(&self) -> Option<&str> {
        self.text("file_path")
    }

    /// The command of a Bash call; empty when the call carries none.
    pub fn command(&self) -> &str {
        self.text("command").unwrap_or("")
    }

    /// Every path this call reaches, absolute and normalised.
    ///
    /// The paths a tool names (`file_path`, `path`, `notebook_path`), and for
    /// the two search tools where the search runs: Glob's `pattern` and Grep's
    /// `glob` joined to the directory searched, and that directory itself when
    /// the call does not name one. A relative path is joined to `cwd`.
    pub fn targets(&self) -> Vec<String> {
        let base = self.cwd.as_deref().unwrap_or("/");
        let mut reached: Vec<String> = ["file_path", "path", "notebook_path"]
            .iter()
            .filter_map(|key| self.text(key))
            .map(|named| absolute_from(base, named))
            .collect();

        let search_filter = match self.tool_name.as_str() {
            "Glob" => Some(self.text("pattern")),
            "Grep" => Some(self.text("glob")),
            _ => None,
        };
        if let Some(filter) = search_filter {
            let searched = match self.text("path") {
                Some(named) => absolute_from(base, named),
                None => {
                    let searched = normalize(base);
                    reached.push(searched.clone());
                    searched
                }
            };
            if let Some(filter) = filter {
                reached.push(absolute_from(&searched, filter));
            }
        }
        reached
    }
}

static PROTECTED_PATHS_TOML: &str = include_str!("../protected_paths.toml");

/// The category of paths that are always blocked.
pub const FLOOR: &str = "floor";
/// The category of paths whose access is noticed.
pub const PROBING: &str = "probing";

/// The protected path list, `protected_paths.toml`: its floor and probing rules.
/// A floor rule is always a block, so one that states a severity is refused.
pub fn protected_paths() -> Result<RuleSet, String> {
    protected_paths_from(PROTECTED_PATHS_TOML)
}

fn protected_paths_from(toml_str: &str) -> Result<RuleSet, String> {
    let rule_set = parse_rule_set(toml_str, &[(FLOOR, PatternKind::Literal), (PROBING, PatternKind::Literal)])?;
    match rules_of(&rule_set, FLOOR).iter().find(|rule| rule.severity.is_some()) {
        Some(rule) => Err(format!("the floor rule for {} states a severity; the floor always blocks", rule.pattern)),
        None => Ok(rule_set),
    }
}

/// What a pre hook replies when it cannot decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhenUndecided {
    /// The user is asked. For the hooks of an interactive session.
    AskUser,
    /// The call is denied. For sandbox hooks, where no user is asking.
    Deny,
    /// The call runs. For a hook that only adds context and guards nothing.
    Allow,
}

/// Run a PreToolUse hook: read the call, decide, reply.
///
/// `decide_fn` is the hook's pure decision over the parsed call.
pub fn run_pre_hook<F>(when_undecided: WhenUndecided, decide_fn: F) -> ExitCode
where
    F: FnOnce(&HookInput) -> HookDecision,
{
    let hook = hook_name();
    let payload = read_payload();
    let call = match &payload {
        Ok(received) => parse_call(received),
        Err(cause) => Err(cause.clone()),
    };
    let (decision, tool) = match call {
        Ok(call) => (decided_or_undecided(when_undecided, &hook, &call, decide_fn), call.tool_name),
        Err(cause) => (undecided(when_undecided, &hook, &cause), "unknown".to_string()),
    };
    write_decision_log(&hook, payload.as_deref().unwrap_or(""), &decision);
    emit_decision(decision, &tool);
    ExitCode::SUCCESS
}

// ── The decision log ───────────────────────────────────────────────

/// The decision word of a decision that is logged; `None` for a plain allow.
fn logged_decision(decision: &HookDecision) -> Option<(&'static str, &str, &str, &str)> {
    match decision {
        HookDecision::Allow | HookDecision::AllowWithContext { .. } => None,
        HookDecision::Warn { category, event, user_reason, .. } => Some(("warn", category, event, user_reason)),
        HookDecision::Ask { category, event, reason, .. } => Some(("ask", category, event, reason)),
        HookDecision::Deny { category, event, reason } => Some(("deny", category, event, reason)),
    }
}

/// When and where a decision was made.
struct DecisionSetting<'a> {
    timestamp: f64,
    hook: &'a str,
    workspace: &'a str,
}

/// One line of the decision log for a call the hook did not simply allow; its
/// shape is `schemas/tools/hook-decision-log.schema.json`. The call is taken
/// from the payload as received, so a call the hook could not read is logged too.
fn decision_log_record(setting: &DecisionSetting, payload: &str, decision: &HookDecision) -> Option<serde_json::Value> {
    let (decision_word, category, event, detail) = logged_decision(decision)?;
    let received: serde_json::Value = serde_json::from_str(payload).unwrap_or(serde_json::Value::Null);
    let tool_input = received.get("tool_input").filter(|input| input.is_object()).cloned();
    let mut record = serde_json::json!({
        "timestamp": setting.timestamp,
        "hook": setting.hook,
        "workspace": setting.workspace,
        "session": received.get("session_id").and_then(|session| session.as_str()),
        "tool": received.get("tool_name").and_then(|tool| tool.as_str()),
        "decision": decision_word,
        "category": category,
        "event": event,
        "detail": detail,
        "tool_input": tool_input,
    });
    if !received.is_object() {
        record["unread_payload"] = serde_json::Value::String(payload.to_string());
    }
    Some(record)
}

/// Append the decision to `~/ai/logs/<hook>.log.jsonl`. A line that cannot be
/// written changes nothing about the decision.
fn write_decision_log(hook: &str, payload: &str, decision: &HookDecision) {
    let setting = DecisionSetting { timestamp: datagram_io::now(), hook, workspace: &datagram_io::workspace_name() };
    let Some(record) = decision_log_record(&setting, payload, decision) else {
        return;
    };
    let line = record.to_string();
    let matches_schema =
        schemas_embedded::HOOK_DECISION_LOG.validate(&line).map(|checked| checked.valid).unwrap_or(false);
    if !matches_schema {
        return;
    }
    let logs = write_engine::ai_home().join("logs");
    if std::fs::create_dir_all(&logs).is_ok() {
        let _ = write_engine::append_line_fsync(&logs.join(format!("{hook}.log.jsonl")), &line);
    }
}

fn hook_name() -> String {
    std::env::args()
        .next()
        .and_then(|invoked| {
            std::path::Path::new(&invoked).file_name().map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "hook".to_string())
}

/// What is on stdin, or why it could not be read.
fn read_payload() -> Result<String, String> {
    let mut payload = String::new();
    std::io::stdin()
        .read_to_string(&mut payload)
        .map_err(|io_err| format!("its input could not be read ({io_err})"))?;
    Ok(payload)
}

/// A payload as a call: valid against the PreToolUse schema, then typed.
pub fn parse_call(payload: &str) -> Result<HookInput, String> {
    let checked = schemas_embedded::HOOK_PRE_TOOL_USE
        .validate(payload)
        .map_err(|schema_err| format!("its input could not be checked ({schema_err})"))?;
    if !checked.valid {
        return Err(format!("its input is not a PreToolUse call ({})", checked.message));
    }
    serde_json::from_str(payload).map_err(|json_err| format!("its input could not be typed ({json_err})"))
}

fn decided_or_undecided<F>(when_undecided: WhenUndecided, hook: &str, call: &HookInput, decide_fn: F) -> HookDecision
where
    F: FnOnce(&HookInput) -> HookDecision,
{
    match catch_unwind(AssertUnwindSafe(|| decide_fn(call))) {
        Ok(decision) => decision,
        Err(_) => undecided(when_undecided, hook, "it failed while deciding"),
    }
}

fn undecided(when_undecided: WhenUndecided, hook: &str, cause: &str) -> HookDecision {
    match when_undecided {
        WhenUndecided::AskUser => ask_because_undecided(hook, cause),
        WhenUndecided::Deny => deny_because_undecided(hook, cause),
        WhenUndecided::Allow => HookDecision::Allow,
    }
}

/// Dispatch a decision: reply on stdout, notify the user, emit to the watchtower.
fn emit_decision(decision: HookDecision, tool: &str) {
    match decision {
        HookDecision::Allow => print_allow(),
        HookDecision::AllowWithContext { context } => print_allow_with_context(&context),
        HookDecision::Warn { category, event, user_reason, llm_context } => {
            notify('\u{26A0}', &category, &user_reason);
            print_warn(&user_reason, &llm_context);
            emit_to_watchtower(&WatchtowerEvent {
                spoken: Spoken::Warn, category: &category, event: &event,
                tool, detail: &user_reason, context: &llm_context,
            });
        }
        HookDecision::Ask { category, event, reason, llm_context } => {
            notify('\u{2753}', &category, &reason);
            print_ask(&reason, &llm_context);
            emit_to_watchtower(&WatchtowerEvent {
                spoken: Spoken::Ask, category: &category, event: &event,
                tool, detail: &reason, context: &llm_context,
            });
        }
        HookDecision::Deny { category, event, reason } => {
            notify('\u{2716}', &category, &reason);
            print_deny(&reason);
            emit_to_watchtower(&WatchtowerEvent {
                spoken: Spoken::Deny, category: &category, event: &event,
                tool, detail: &reason, context: "",
            });
        }
    }
}

// ── Reply on stdout ────────────────────────────────────────────────

fn print_allow() {
    use response::{HookOutput, PreToolUseResponse};
    print!("{}", PreToolUseResponse::allow().to_json());
}

fn print_allow_with_context(context: &str) {
    use response::{HookOutput, PreToolUseResponse};
    print!("{}", PreToolUseResponse::allow().with_context(context).to_json());
}

fn print_warn(user_reason: &str, llm_context: &str) {
    use response::{HookOutput, PreToolUseResponse};
    let resp = PreToolUseResponse::allow().with_reason(user_reason).with_context(llm_context);
    print!("{}", resp.to_json());
}

fn print_ask(reason: &str, llm_context: &str) {
    use response::{HookOutput, PreToolUseResponse};
    let resp = PreToolUseResponse::ask(reason).with_context(llm_context);
    print!("{}", resp.to_json());
}

fn print_deny(reason: &str) {
    use response::{HookOutput, PreToolUseResponse};
    print!("{}", PreToolUseResponse::deny(reason).to_json());
}

// ── User notification ───────────────────────────────────────────────
//
// Claude Code captures stdout (the reply) and stderr through pipes. A macOS
// notification is the channel that reaches the user at once; the decision log
// is the record.

fn notify(icon: char, category: &str, explanation: &str) {
    let short = explanation.lines().next().unwrap_or(explanation);
    let title = format!("{} INTERCEPT [{}]", icon, category);
    let _ = std::process::Command::new("terminal-notifier")
        .args(["-title", &title, "-message", short])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

// ── Watchtower emission ───────────────────────────────────────────

struct WatchtowerEvent<'a> {
    spoken: Spoken,
    category: &'a str,
    event: &'a str,
    tool: &'a str,
    detail: &'a str,
    context: &'a str,
}

fn decision_word(spoken: Spoken) -> &'static str {
    match spoken {
        Spoken::Deny => "deny",
        Spoken::Ask => "ask",
        Spoken::Warn => "warn",
    }
}

/// A hook that could not decide is the highest priority there is: the guard
/// itself is not working.
fn alert_priority(spoken: Spoken, category: &str) -> datagram_io::Priority {
    match spoken {
        _ if category == UNDECIDED => datagram_io::Priority::High,
        Spoken::Deny => datagram_io::Priority::High,
        Spoken::Warn => datagram_io::Priority::Normal,
        Spoken::Ask => datagram_io::Priority::Low,
    }
}

fn emit_to_watchtower(alert: &WatchtowerEvent) {
    let datagram = datagram_io::Datagram {
        timestamp: datagram_io::now(),
        source: hook_name(),
        kind: datagram_io::DatagramKind::Alert,
        classifier: None,
        priority: alert_priority(alert.spoken, alert.category),
        workspace: datagram_io::workspace_name(),
        detail: Some(alert.detail.to_string()),
        speech: None, // spoken locally through announce
        payload: Some(serde_json::json!({
            "category": alert.category,
            "decision": decision_word(alert.spoken),
            "tool": alert.tool,
            "event": alert.event,
            "context_injected": alert.context,
        })),
    };
    datagram_io::emit(&datagram);
    speak_alert(&sentence(alert.spoken, alert.category), alert.spoken, alert.category);
}

fn announce_severity(spoken: Spoken, category: &str) -> &'static str {
    match spoken {
        _ if category == UNDECIDED => "critical",
        Spoken::Deny => match category {
            "floor" | "subversion" | "destruction" | "chaining" => "critical",
            _ => "alert",
        },
        Spoken::Ask => "notify",
        Spoken::Warn => "warn",
    }
}

fn speak_alert(speech: &str, spoken: Spoken, category: &str) {
    // SILENT.lock: global first, then per-workspace
    let control_voice = write_engine::ai_home().join("control/voice");
    if control_voice.join("SILENT.lock").exists() {
        return;
    }
    let source = std::env::var("CLAUDE_PROJECT_DIR").unwrap_or_default();
    let workspace = if source.is_empty() {
        None
    } else {
        workspace_registry::resolve_workspace_from_path(&source).ok().flatten()
    };
    if let Some(workspace) = workspace {
        if workspace_registry::workspace_control_dir(&workspace).join("SILENT.lock").exists() {
            return;
        }
    }

    let announce = write_engine::ai_home().join("tools/bin/announce");
    let mut cmd = std::process::Command::new(&announce);
    cmd.args(["--severity", announce_severity(spoken, category)]);
    if !source.is_empty() {
        cmd.args(["--source", &source]);
    }
    cmd.arg(speech);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let _ = cmd.spawn();
}

// ── PostToolUse contract ─────────────────────────────────────────

/// A PostToolUse call, as far as the hooks read it. Its shape on the wire is
/// `schemas/tools/hook-post-tool-use.schema.json`.
#[derive(Debug, Deserialize)]
pub struct PostHookInput {
    pub tool_name: String,
    pub tool_input: serde_json::Value,
}

impl PostHookInput {
    /// The file the tool was called on: its `file_path`.
    pub fn file_path(&self) -> Option<&str> {
        self.tool_input.get("file_path").and_then(|value| value.as_str())
    }
}

/// Run a PostToolUse hook: read the call, assess, reply.
///
/// `assess_fn` returns the context to give the session, or `None` for silence.
/// A call the hook cannot read is not silence: the session is told the hook
/// did not run, and an alert is raised.
pub fn run_post_hook<F>(assess_fn: F) -> ExitCode
where
    F: FnOnce(&PostHookInput) -> Option<String>,
{
    let said = match read_post_call() {
        Ok(call) => assess_fn(&call),
        Err(cause) => {
            let message = post_hook_did_not_run(&hook_name(), &cause);
            alert_post_hook_did_not_run(&message);
            Some(message)
        }
    };
    if let Some(context) = said {
        print_post_context(context);
    }
    ExitCode::SUCCESS
}

/// The PostToolUse reply on stdout: context for the session.
fn print_post_context(context: String) {
    use response::{HookOutput, PostToolUseResponse};
    let json = PostToolUseResponse::allow().with_context(context).to_json();
    if !json.is_empty() {
        print!("{}", json);
    }
}

fn read_post_call() -> Result<PostHookInput, String> {
    let mut payload = String::new();
    std::io::stdin()
        .read_to_string(&mut payload)
        .map_err(|io_err| format!("its input could not be read ({io_err})"))?;
    parse_post_call(&payload)
}

/// A payload as a PostToolUse call: valid against its schema, then typed.
pub fn parse_post_call(payload: &str) -> Result<PostHookInput, String> {
    let checked = schemas_embedded::HOOK_POST_TOOL_USE
        .validate(payload)
        .map_err(|schema_err| format!("its input could not be checked ({schema_err})"))?;
    if !checked.valid {
        return Err(format!("its input is not a PostToolUse call ({})", checked.message));
    }
    serde_json::from_str(payload).map_err(|json_err| format!("its input could not be typed ({json_err})"))
}

/// What the session is told when a post hook could not read its call.
fn post_hook_did_not_run(hook: &str, cause: &str) -> String {
    format!(
        "{hook} did not run: {cause}. Whatever this tool call wrote has not been examined \
         and is not known to be clean."
    )
}

fn alert_post_hook_did_not_run(message: &str) {
    let datagram = datagram_io::Datagram {
        timestamp: datagram_io::now(),
        source: hook_name(),
        kind: datagram_io::DatagramKind::Alert,
        classifier: None,
        priority: datagram_io::Priority::High,
        workspace: datagram_io::workspace_name(),
        detail: Some(message.to_string()),
        speech: None,
        payload: Some(serde_json::json!({ "category": UNDECIDED, "decision": "not_run" })),
    };
    datagram_io::emit(&datagram);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(tool: &str, cwd: Option<&str>, tool_input: serde_json::Value) -> HookInput {
        HookInput { tool_name: tool.to_string(), tool_input, cwd: cwd.map(str::to_string) }
    }

    // ── protected_paths ──────────────────────────────────────────

    #[test]
    fn the_embedded_list_reads() {
        let protected = protected_paths().unwrap();
        assert!(!rules_of(&protected, FLOOR).is_empty());
        assert!(!rules_of(&protected, PROBING).is_empty());
    }

    #[test]
    fn the_floor_holds_every_credential_store() {
        let protected = protected_paths().unwrap();
        let floor: Vec<&str> = rules_of(&protected, FLOOR).iter().map(|rule| rule.pattern.as_str()).collect();
        for store in ["/.ssh/", "/.aws/", "/.gnupg/", "/.kube/config", "/.docker/config.json", "/.netrc"] {
            assert!(floor.contains(&store), "{store} is not on the floor");
        }
    }

    #[test]
    fn probing_covers_every_hook_and_the_grant() {
        let protected = protected_paths().unwrap();
        let probing = rules_of(&protected, PROBING);
        let watched = [
            "/n/hooks/hook_pre_llm_bash/src/main.rs",
            "/n/hooks/hook_pre_llm_tool/src/main.rs",
            "/n/hooks/hook_pre_subagent_bash/src/main.rs",
            "/n/hooks/hook_pre_subagent_tool/src/main.rs",
            "/n/hooks/hook_post_llm_tool/src/main.rs",
            "/n/capability/hook_io/protected_paths.toml",
            "/n/core/hook_decision_core/src/rules.rs",
            "/n/cli/cc_launch/src/permissions.rs",
            "/u/.claude/settings.json",
        ];
        for path in watched {
            assert!(probing.iter().any(|rule| rule.is_match(path)), "{path} is not watched");
        }
    }

    #[test]
    fn every_protected_pattern_is_written_in_lower_case() {
        // Paths are matched in lower case; a pattern with a capital would match nothing.
        let protected = protected_paths().unwrap();
        for rule in rules_of(&protected, FLOOR).iter().chain(rules_of(&protected, PROBING)) {
            assert_eq!(rule.pattern, rule.pattern.to_lowercase(), "{}", rule.pattern);
        }
    }

    #[test]
    fn a_floor_rule_that_states_a_severity_is_refused() {
        let weakened = "[[floor]]\npattern = \"/.ssh/\"\ndescription = \"x\"\nseverity = \"warn\"\n";
        assert!(protected_paths_from(weakened).unwrap_err().contains("always blocks"));
    }

    #[test]
    fn a_list_with_a_category_of_its_own_is_refused() {
        assert!(protected_paths_from("[[gaming]]\npattern = \"x\"\ndescription = \"x\"\n").is_err());
    }

    // ── parse_call ───────────────────────────────────────────────

    #[test]
    fn a_full_payload_is_a_call() {
        let payload = r#"{"session_id":"s","hook_event_name":"PreToolUse","cwd":"/w","tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let parsed = parse_call(payload).unwrap();
        assert_eq!(parsed.tool_name, "Bash");
        assert_eq!(parsed.command(), "ls");
        assert_eq!(parsed.cwd.as_deref(), Some("/w"));
    }

    #[test]
    fn the_deploy_verification_payload_is_a_call() {
        assert!(parse_call(r#"{"tool_name":"Test","tool_input":{}}"#).is_ok());
    }

    #[test]
    fn text_that_is_not_json_is_not_a_call() {
        assert!(parse_call("not json").is_err());
        assert!(parse_call("").is_err());
    }

    #[test]
    fn a_payload_without_a_tool_name_is_not_a_call() {
        let err = parse_call(r#"{"tool_input":{"command":"ls"}}"#).unwrap_err();
        assert!(err.contains("not a PreToolUse call"), "{err}");
    }

    #[test]
    fn a_payload_whose_tool_input_was_renamed_is_not_a_call() {
        assert!(parse_call(r#"{"tool_name":"Bash","input":{"command":"ls"}}"#).is_err());
    }

    #[test]
    fn a_payload_with_a_tool_input_that_is_not_an_object_is_not_a_call() {
        assert!(parse_call(r#"{"tool_name":"Bash","tool_input":"ls"}"#).is_err());
    }

    // ── undecided ────────────────────────────────────────────────

    #[test]
    fn an_interactive_hook_that_cannot_decide_asks() {
        assert!(matches!(undecided(WhenUndecided::AskUser, "h", "x"), HookDecision::Ask { .. }));
    }

    #[test]
    fn a_sandbox_hook_that_cannot_decide_denies() {
        assert!(matches!(undecided(WhenUndecided::Deny, "h", "x"), HookDecision::Deny { .. }));
    }

    #[test]
    fn a_decide_function_that_panics_is_undecided() {
        let failing = |_: &HookInput| -> HookDecision { panic!("boom") };
        let decision = decided_or_undecided(WhenUndecided::AskUser, "h", &call("Bash", None, serde_json::json!({})), failing);
        match decision {
            HookDecision::Ask { category, .. } => assert_eq!(category, UNDECIDED),
            other => panic!("must ask, got {other:?}"),
        }
    }

    #[test]
    fn a_decide_function_that_returns_is_passed_through() {
        let allowing = |_: &HookInput| HookDecision::Allow;
        let decision = decided_or_undecided(WhenUndecided::Deny, "h", &call("Bash", None, serde_json::json!({})), allowing);
        assert!(matches!(decision, HookDecision::Allow));
    }

    #[test]
    fn an_undecided_ask_is_high_priority() {
        assert!(matches!(alert_priority(Spoken::Ask, UNDECIDED), datagram_io::Priority::High));
        assert!(matches!(alert_priority(Spoken::Ask, "revert"), datagram_io::Priority::Low));
    }

    // ── The decision log ─────────────────────────────────────────

    const SETTING: DecisionSetting = DecisionSetting { timestamp: 1_800_000_000.5, hook: "hook_pre_llm_bash", workspace: "tyr" };

    fn deny(category: &str) -> HookDecision {
        HookDecision::Deny { category: category.into(), event: "an event".into(), reason: "a reason".into() }
    }

    fn valid_log_line(record: &serde_json::Value) -> bool {
        schemas_embedded::HOOK_DECISION_LOG.validate(&record.to_string()).unwrap().valid
    }

    #[test]
    fn a_denied_call_is_logged_whole() {
        let payload = r#"{"session_id":"s-1","tool_name":"Bash","tool_input":{"command":"echo a; echo b"}}"#;
        let record = decision_log_record(&SETTING, payload, &deny("chaining")).unwrap();
        assert_eq!(record["decision"], "deny");
        assert_eq!(record["category"], "chaining");
        assert_eq!(record["session"], "s-1");
        assert_eq!(record["tool"], "Bash");
        assert_eq!(record["tool_input"]["command"], "echo a; echo b");
        assert_eq!(record["hook"], "hook_pre_llm_bash");
        assert!(valid_log_line(&record));
    }

    #[test]
    fn a_warned_and_an_asked_call_are_logged() {
        let payload = r#"{"tool_name":"Read","tool_input":{"file_path":"/x"}}"#;
        let warned = HookDecision::Warn {
            category: "probing".into(), event: "e".into(), user_reason: "u".into(), llm_context: "c".into(),
        };
        let asked = ask_because_undecided("hook_pre_llm_tool", "its flags are misspelled");
        let warned_record = decision_log_record(&SETTING, payload, &warned).unwrap();
        let asked_record = decision_log_record(&SETTING, payload, &asked).unwrap();
        assert_eq!(warned_record["decision"], "warn");
        assert_eq!(asked_record["category"], UNDECIDED);
        assert!(warned_record["session"].is_null());
        assert!(valid_log_line(&warned_record) && valid_log_line(&asked_record));
    }

    #[test]
    fn an_allowed_call_is_not_logged() {
        let payload = r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        assert!(decision_log_record(&SETTING, payload, &HookDecision::Allow).is_none());
        let with_context = HookDecision::AllowWithContext { context: "c".into() };
        assert!(decision_log_record(&SETTING, payload, &with_context).is_none());
    }

    #[test]
    fn a_payload_the_hook_could_not_read_is_logged_as_received() {
        let record = decision_log_record(&SETTING, "not json", &deny(UNDECIDED)).unwrap();
        assert_eq!(record["unread_payload"], "not json");
        assert!(record["tool"].is_null() && record["tool_input"].is_null());
        assert!(valid_log_line(&record));
    }

    // ── targets ──────────────────────────────────────────────────

    #[test]
    fn a_file_tool_reaches_its_file() {
        let read = call("Read", Some("/w"), serde_json::json!({ "file_path": "/src/main.rs" }));
        assert_eq!(read.targets(), vec!["/src/main.rs"]);
    }

    #[test]
    fn a_path_is_normalised() {
        let read = call("Read", Some("/w"), serde_json::json!({ "file_path": "/a/tools/../.ssh/id_rsa" }));
        assert_eq!(read.targets(), vec!["/a/.ssh/id_rsa"]);
    }

    #[test]
    fn a_relative_path_is_joined_to_the_working_directory() {
        let read = call("Read", Some("/w/project"), serde_json::json!({ "file_path": "../other/x.py" }));
        assert_eq!(read.targets(), vec!["/w/other/x.py"]);
    }

    #[test]
    fn a_notebook_tool_reaches_its_notebook() {
        let edit = call("NotebookEdit", Some("/w"), serde_json::json!({ "notebook_path": "/n/book.ipynb" }));
        assert_eq!(edit.targets(), vec!["/n/book.ipynb"]);
    }

    #[test]
    fn a_glob_with_an_absolute_pattern_reaches_it() {
        let glob = call("Glob", Some("/w"), serde_json::json!({ "pattern": "/home/u/.ssh/*" }));
        assert!(glob.targets().contains(&"/home/u/.ssh/*".to_string()));
    }

    #[test]
    fn a_glob_without_a_path_reaches_the_working_directory() {
        let glob = call("Glob", Some("/w/project"), serde_json::json!({ "pattern": "**/*.rs" }));
        assert_eq!(glob.targets(), vec!["/w/project", "/w/project/**/*.rs"]);
    }

    #[test]
    fn a_glob_pattern_is_joined_to_the_path_searched() {
        let glob = call("Glob", Some("/w"), serde_json::json!({ "path": "/home/u", "pattern": ".aws/*" }));
        assert_eq!(glob.targets(), vec!["/home/u", "/home/u/.aws/*"]);
    }

    #[test]
    fn a_grep_reaches_its_path_and_its_glob() {
        let grep = call("Grep", Some("/w"), serde_json::json!({ "pattern": "secret", "path": "/home/u", "glob": ".ssh/*" }));
        assert_eq!(grep.targets(), vec!["/home/u", "/home/u/.ssh/*"]);
    }

    #[test]
    fn a_grep_search_text_is_not_a_path() {
        let grep = call("Grep", Some("/w"), serde_json::json!({ "pattern": "/.ssh/", "path": "/src" }));
        assert_eq!(grep.targets(), vec!["/src"]);
    }

    #[test]
    fn a_grep_without_a_path_reaches_the_working_directory() {
        let grep = call("Grep", Some("/w/project"), serde_json::json!({ "pattern": "x" }));
        assert_eq!(grep.targets(), vec!["/w/project"]);
    }

    #[test]
    fn a_call_that_names_no_path_reaches_none() {
        assert!(call("Read", Some("/w"), serde_json::json!({})).targets().is_empty());
        assert!(call("Read", Some("/w"), serde_json::json!({ "file_path": null })).targets().is_empty());
    }

    // ── command ──────────────────────────────────────────────────

    #[test]
    fn a_bash_call_carries_its_command() {
        assert_eq!(call("Bash", None, serde_json::json!({ "command": "ls -la" })).command(), "ls -la");
    }

    #[test]
    fn a_call_without_a_command_carries_an_empty_one() {
        assert_eq!(call("Bash", None, serde_json::json!({})).command(), "");
        assert_eq!(call("Bash", None, serde_json::json!({ "command": null })).command(), "");
    }

    // ── PostHookInput ────────────────────────────────────────────

    #[test]
    fn a_post_call_names_its_file() {
        let post = parse_post_call(r#"{"tool_name":"Write","tool_input":{"file_path":"/out/file.py"},"tool_response":{}}"#)
            .unwrap();
        assert_eq!(post.tool_name, "Write");
        assert_eq!(post.file_path(), Some("/out/file.py"));
    }

    #[test]
    fn a_post_call_without_a_file_names_none() {
        let post = parse_post_call(r#"{"tool_name":"Write","tool_input":{}}"#).unwrap();
        assert!(post.file_path().is_none());
    }

    #[test]
    fn a_payload_that_is_not_a_post_call_is_refused() {
        for payload in ["", "not json", r#"{"tool_input":{}}"#, r#"{"tool_name":"Write","input":{}}"#] {
            assert!(parse_post_call(payload).is_err(), "{payload:?}");
        }
    }

    #[test]
    fn a_post_hook_that_did_not_run_says_so_and_why() {
        let message = post_hook_did_not_run("hook_post_llm_tool", "its input is not a PostToolUse call");
        assert!(message.contains("did not run"));
        assert!(message.contains("its input is not a PostToolUse call"));
        assert!(message.contains("not known to be clean"));
    }
}
