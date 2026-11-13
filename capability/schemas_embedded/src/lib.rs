//! All schemas embedded via `include_str!()` through local symlinks.
//!
//! Schemas live in `nornir/schemas/` as symlinks to their source locations.
//! This decouples the Rust source from the physical location of schema files.
//! When schema sources move, update the symlinks — not this file.
//!
//! agents/  — agent building checkpoint and definition validation (20 schemas)
//! tools/   — read/write tools with schema based filter gates (8 schemas)

use schema_core::EmbeddedValidator;

// =============================================================================
// Agent pipeline schemas (13)
// =============================================================================

static RAW_DEF_JSON: &str =
    include_str!("../../../schemas/agents/agent-raw-definition.schema.json");
pub static RAW_DEFINITION: EmbeddedValidator =
    EmbeddedValidator::new(RAW_DEF_JSON, "raw-definition");

static PATHS_RES_JSON: &str =
    include_str!("../../../schemas/agents/agent-paths-resolved.schema.json");
pub static PATHS_RESOLVED: EmbeddedValidator =
    EmbeddedValidator::new(PATHS_RES_JSON, "paths-resolved");

static GUARD_RED_JSON: &str =
    include_str!("../../../schemas/agents/agent-guardrails-reduced.schema.json");
pub static GUARDRAILS_REDUCED: EmbeddedValidator =
    EmbeddedValidator::new(GUARD_RED_JSON, "guardrails-reduced");

static SUCCESS_RED_JSON: &str =
    include_str!("../../../schemas/agents/agent-success-reduced.schema.json");
pub static SUCCESS_REDUCED: EmbeddedValidator =
    EmbeddedValidator::new(SUCCESS_RED_JSON, "success-reduced");

static CRIT_MERG_JSON: &str =
    include_str!("../../../schemas/agents/agent-criteria-merged.schema.json");
pub static CRITERIA_MERGED: EmbeddedValidator =
    EmbeddedValidator::new(CRIT_MERG_JSON, "criteria-merged");

static INST_RED_JSON: &str =
    include_str!("../../../schemas/agents/agent-instructions-reduced.schema.json");
pub static INSTRUCTIONS_REDUCED: EmbeddedValidator =
    EmbeddedValidator::new(INST_RED_JSON, "instructions-reduced");

static EXAM_RED_JSON: &str =
    include_str!("../../../schemas/agents/agent-examples-reduced.schema.json");
pub static EXAMPLES_REDUCED: EmbeddedValidator =
    EmbeddedValidator::new(EXAM_RED_JSON, "examples-reduced");

static EXEC_MERG_JSON: &str =
    include_str!("../../../schemas/agents/agent-execution-merged.schema.json");
pub static EXECUTION_MERGED: EmbeddedValidator =
    EmbeddedValidator::new(EXEC_MERG_JSON, "execution-merged");

static INCL_MERG_JSON: &str =
    include_str!("../../../schemas/agents/agent-includes-merged.schema.json");
pub static INCLUDES_MERGED: EmbeddedValidator =
    EmbeddedValidator::new(INCL_MERG_JSON, "includes-merged");

static PERM_RES_JSON: &str =
    include_str!("../../../schemas/agents/agent-permissions-resolved.schema.json");
pub static PERMISSIONS_RESOLVED: EmbeddedValidator =
    EmbeddedValidator::new(PERM_RES_JSON, "permissions-resolved");

static UNIV_FMT_JSON: &str =
    include_str!("../../../schemas/agents/agent-universal-format.schema.json");
pub static UNIVERSAL_FORMAT: EmbeddedValidator =
    EmbeddedValidator::new(UNIV_FMT_JSON, "universal-format");

static UNIV_REND_JSON: &str =
    include_str!("../../../schemas/agents/agent-universal-render.schema.json");
pub static UNIVERSAL_RENDER: EmbeddedValidator =
    EmbeddedValidator::new(UNIV_REND_JSON, "universal-render");

static ANTH_REND_JSON: &str =
    include_str!("../../../schemas/agents/agent-anthropic-render.schema.json");
pub static ANTHROPIC_RENDER: EmbeddedValidator =
    EmbeddedValidator::new(ANTH_REND_JSON, "anthropic-render");

// =============================================================================
// Agent output control surface schemas (3)
// =============================================================================

static OUT_STRUCT_JSON: &str =
    include_str!("../../../schemas/agents/agent-output-structure.schema.json");
pub static OUTPUT_STRUCTURE: EmbeddedValidator =
    EmbeddedValidator::new(OUT_STRUCT_JSON, "output-structure");

static OUT_CONTENT_JSON: &str =
    include_str!("../../../schemas/agents/agent-output-content.schema.json");
pub static OUTPUT_CONTENT: EmbeddedValidator =
    EmbeddedValidator::new(OUT_CONTENT_JSON, "output-content");

static OUT_DISPLAY_JSON: &str =
    include_str!("../../../schemas/agents/agent-output-display.schema.json");
pub static OUTPUT_DISPLAY: EmbeddedValidator =
    EmbeddedValidator::new(OUT_DISPLAY_JSON, "output-display");

// =============================================================================
// Agent include fragment schemas (7)
// =============================================================================

static INCL_SUCCESS_JSON: &str =
    include_str!("../../../schemas/agents/include-success-criteria.schema.json");
pub static INCLUDE_SUCCESS_CRITERIA: EmbeddedValidator =
    EmbeddedValidator::new(INCL_SUCCESS_JSON, "include-success-criteria");

static INCL_FAILURE_JSON: &str =
    include_str!("../../../schemas/agents/include-failure-criteria.schema.json");
pub static INCLUDE_FAILURE_CRITERIA: EmbeddedValidator =
    EmbeddedValidator::new(INCL_FAILURE_JSON, "include-failure-criteria");

static INCL_EXEC_INST_JSON: &str =
    include_str!("../../../schemas/agents/include-execution-instructions.schema.json");
pub static INCLUDE_EXECUTION_INSTRUCTIONS: EmbeddedValidator =
    EmbeddedValidator::new(INCL_EXEC_INST_JSON, "include-execution-instructions");

static INCL_EXAMPLE_ENTRIES_JSON: &str =
    include_str!("../../../schemas/agents/include-example-entries.schema.json");
pub static INCLUDE_EXAMPLE_ENTRIES: EmbeddedValidator =
    EmbeddedValidator::new(INCL_EXAMPLE_ENTRIES_JSON, "include-example-entries");

static INCL_EXAMPLE_GROUP_JSON: &str =
    include_str!("../../../schemas/agents/include-example-group.schema.json");
pub static INCLUDE_EXAMPLE_GROUP: EmbeddedValidator =
    EmbeddedValidator::new(INCL_EXAMPLE_GROUP_JSON, "include-example-group");

static INCL_GUARD_CONSTR_JSON: &str =
    include_str!("../../../schemas/agents/include-guardrails-constraints.schema.json");
pub static INCLUDE_GUARDRAILS_CONSTRAINTS: EmbeddedValidator =
    EmbeddedValidator::new(INCL_GUARD_CONSTR_JSON, "include-guardrails-constraints");

static INCL_GUARD_ANTI_JSON: &str =
    include_str!("../../../schemas/agents/include-guardrails-anti-patterns.schema.json");
pub static INCLUDE_GUARDRAILS_ANTI_PATTERNS: EmbeddedValidator =
    EmbeddedValidator::new(INCL_GUARD_ANTI_JSON, "include-guardrails-anti-patterns");

// =============================================================================
// Tool schemas (8)
// =============================================================================

static DATAGRAM_JSON: &str =
    include_str!("../../../schemas/tools/validate.datagram.schema.json");
pub static DATAGRAM: EmbeddedValidator =
    EmbeddedValidator::new(DATAGRAM_JSON, "datagram");

static QC_REPORT_JSON: &str =
    include_str!("../../../schemas/tools/qc-report.schema.json");
pub static QC_REPORT: EmbeddedValidator =
    EmbeddedValidator::new(QC_REPORT_JSON, "qc-report");

static GLOSSARY_JSON: &str =
    include_str!("../../../schemas/tools/glossary.schema.json");
pub static GLOSSARY: EmbeddedValidator =
    EmbeddedValidator::new(GLOSSARY_JSON, "glossary");

static EMBEDDING_TARGET_JSON: &str =
    include_str!("../../../schemas/tools/embedding-target.schema.json");
pub static EMBEDDING_TARGET: EmbeddedValidator =
    EmbeddedValidator::new(EMBEDDING_TARGET_JSON, "embedding-target");

static SUMMARIES_JSON: &str =
    include_str!("../../../schemas/tools/summaries.schema.json");
pub static SUMMARIES: EmbeddedValidator =
    EmbeddedValidator::new(SUMMARIES_JSON, "summaries");

static RAW_JSONL_JSON: &str =
    include_str!("../../../schemas/tools/raw-jsonl.schema.json");
pub static RAW_JSONL_RECORD: EmbeddedValidator =
    EmbeddedValidator::new(RAW_JSONL_JSON, "raw-jsonl");

static INTERVIEW_EXCHANGE_JSON: &str =
    include_str!("../../../schemas/tools/interview-exchange.schema.json");
pub static INTERVIEW_EXCHANGE: EmbeddedValidator =
    EmbeddedValidator::new(INTERVIEW_EXCHANGE_JSON, "interview-exchange");

static INTERVIEW_SESSION_JSON: &str =
    include_str!("../../../schemas/tools/interview-session.schema.json");
pub static INTERVIEW_SESSION: EmbeddedValidator =
    EmbeddedValidator::new(INTERVIEW_SESSION_JSON, "interview-session");

static DIRECTORY_LISTING_JSON: &str =
    include_str!("../../../schemas/tools/directory-listing.schema.json");
pub static DIRECTORY_LISTING: EmbeddedValidator =
    EmbeddedValidator::new(DIRECTORY_LISTING_JSON, "directory-listing");

static ZONE_REQUEST_JSON: &str =
    include_str!("../../../schemas/tools/zone-request.schema.json");
pub static ZONE_REQUEST: EmbeddedValidator =
    EmbeddedValidator::new(ZONE_REQUEST_JSON, "zone-request");

static ZONE_REPORT_JSON: &str =
    include_str!("../../../schemas/tools/zone-report.schema.json");
pub static ZONE_REPORT: EmbeddedValidator =
    EmbeddedValidator::new(ZONE_REPORT_JSON, "zone-report");

#[cfg(test)]
mod tests {
    use super::*;

    // Agent pipeline schemas

    #[test]
    fn test_raw_definition_compiles() {
        assert_eq!(RAW_DEFINITION.schema_name(), "raw-definition");
        assert!(!RAW_DEFINITION.schema_json().is_empty());
    }

    #[test]
    fn test_paths_resolved_compiles() {
        assert_eq!(PATHS_RESOLVED.schema_name(), "paths-resolved");
        assert!(!PATHS_RESOLVED.schema_json().is_empty());
    }

    #[test]
    fn test_guardrails_reduced_compiles() {
        assert_eq!(GUARDRAILS_REDUCED.schema_name(), "guardrails-reduced");
    }

    #[test]
    fn test_success_reduced_compiles() {
        assert_eq!(SUCCESS_REDUCED.schema_name(), "success-reduced");
    }

    #[test]
    fn test_criteria_merged_compiles() {
        assert_eq!(CRITERIA_MERGED.schema_name(), "criteria-merged");
    }

    #[test]
    fn test_instructions_reduced_compiles() {
        assert_eq!(INSTRUCTIONS_REDUCED.schema_name(), "instructions-reduced");
    }

    #[test]
    fn test_examples_reduced_compiles() {
        assert_eq!(EXAMPLES_REDUCED.schema_name(), "examples-reduced");
    }

    #[test]
    fn test_execution_merged_compiles() {
        assert_eq!(EXECUTION_MERGED.schema_name(), "execution-merged");
    }

    #[test]
    fn test_includes_merged_compiles() {
        assert_eq!(INCLUDES_MERGED.schema_name(), "includes-merged");
    }

    #[test]
    fn test_permissions_resolved_compiles() {
        assert_eq!(PERMISSIONS_RESOLVED.schema_name(), "permissions-resolved");
        assert!(!PERMISSIONS_RESOLVED.schema_json().is_empty());
    }

    #[test]
    fn test_universal_format_compiles() {
        assert_eq!(UNIVERSAL_FORMAT.schema_name(), "universal-format");
        assert!(!UNIVERSAL_FORMAT.schema_json().is_empty());
    }

    #[test]
    fn test_universal_render_compiles() {
        assert_eq!(UNIVERSAL_RENDER.schema_name(), "universal-render");
        assert!(!UNIVERSAL_RENDER.schema_json().is_empty());
    }

    #[test]
    fn test_anthropic_render_compiles() {
        assert_eq!(ANTHROPIC_RENDER.schema_name(), "anthropic-render");
        assert!(!ANTHROPIC_RENDER.schema_json().is_empty());
    }

    // Agent output control surface schemas

    #[test]
    fn test_output_structure_compiles() {
        assert_eq!(OUTPUT_STRUCTURE.schema_name(), "output-structure");
        assert!(!OUTPUT_STRUCTURE.schema_json().is_empty());
    }

    #[test]
    fn test_output_content_compiles() {
        assert_eq!(OUTPUT_CONTENT.schema_name(), "output-content");
        assert!(!OUTPUT_CONTENT.schema_json().is_empty());
    }

    #[test]
    fn test_output_display_compiles() {
        assert_eq!(OUTPUT_DISPLAY.schema_name(), "output-display");
        assert!(!OUTPUT_DISPLAY.schema_json().is_empty());
    }

    // Agent include fragment schemas

    #[test]
    fn test_include_success_criteria_compiles() {
        assert_eq!(INCLUDE_SUCCESS_CRITERIA.schema_name(), "include-success-criteria");
        assert!(!INCLUDE_SUCCESS_CRITERIA.schema_json().is_empty());
    }

    #[test]
    fn test_include_failure_criteria_compiles() {
        assert_eq!(INCLUDE_FAILURE_CRITERIA.schema_name(), "include-failure-criteria");
        assert!(!INCLUDE_FAILURE_CRITERIA.schema_json().is_empty());
    }

    #[test]
    fn test_include_execution_instructions_compiles() {
        assert_eq!(INCLUDE_EXECUTION_INSTRUCTIONS.schema_name(), "include-execution-instructions");
        assert!(!INCLUDE_EXECUTION_INSTRUCTIONS.schema_json().is_empty());
    }

    #[test]
    fn test_include_example_entries_compiles() {
        assert_eq!(INCLUDE_EXAMPLE_ENTRIES.schema_name(), "include-example-entries");
        assert!(!INCLUDE_EXAMPLE_ENTRIES.schema_json().is_empty());
    }

    #[test]
    fn test_include_example_group_compiles() {
        assert_eq!(INCLUDE_EXAMPLE_GROUP.schema_name(), "include-example-group");
        assert!(!INCLUDE_EXAMPLE_GROUP.schema_json().is_empty());
    }

    #[test]
    fn test_include_guardrails_constraints_compiles() {
        assert_eq!(INCLUDE_GUARDRAILS_CONSTRAINTS.schema_name(), "include-guardrails-constraints");
        assert!(!INCLUDE_GUARDRAILS_CONSTRAINTS.schema_json().is_empty());
    }

    #[test]
    fn test_include_guardrails_anti_patterns_compiles() {
        assert_eq!(INCLUDE_GUARDRAILS_ANTI_PATTERNS.schema_name(), "include-guardrails-anti-patterns");
        assert!(!INCLUDE_GUARDRAILS_ANTI_PATTERNS.schema_json().is_empty());
    }

    // Tool schemas

    #[test]
    fn test_datagram_compiles() {
        assert_eq!(DATAGRAM.schema_name(), "datagram");
        assert!(!DATAGRAM.schema_json().is_empty());
    }

    #[test]
    fn test_qc_report_compiles() {
        assert_eq!(QC_REPORT.schema_name(), "qc-report");
        assert!(!QC_REPORT.schema_json().is_empty());
    }

    #[test]
    fn test_glossary_compiles() {
        assert_eq!(GLOSSARY.schema_name(), "glossary");
        assert!(!GLOSSARY.schema_json().is_empty());
    }

    #[test]
    fn test_embedding_target_compiles() {
        assert_eq!(EMBEDDING_TARGET.schema_name(), "embedding-target");
        assert!(!EMBEDDING_TARGET.schema_json().is_empty());
    }

    #[test]
    fn test_summaries_compiles() {
        assert_eq!(SUMMARIES.schema_name(), "summaries");
        assert!(!SUMMARIES.schema_json().is_empty());
    }

    #[test]
    fn test_raw_jsonl_record_compiles() {
        assert_eq!(RAW_JSONL_RECORD.schema_name(), "raw-jsonl");
        assert!(!RAW_JSONL_RECORD.schema_json().is_empty());
    }

    #[test]
    fn test_interview_exchange_compiles() {
        assert_eq!(INTERVIEW_EXCHANGE.schema_name(), "interview-exchange");
        assert!(!INTERVIEW_EXCHANGE.schema_json().is_empty());
    }

    #[test]
    fn test_interview_exchange_requires_stamps_and_refuses_extras() {
        let complete = r#"{"exchange":1,"session":"20261110T045515Z","ts":"2026-11-10T04:55:15Z","agent":"q","stt":"a","user":"a","llm_log":{"learned":["x"],"threads":[]}}"#;
        assert!(INTERVIEW_EXCHANGE.is_valid(complete).unwrap());

        let unstamped = r#"{"agent":"q","stt":"a","user":"a"}"#;
        assert!(!INTERVIEW_EXCHANGE.is_valid(unstamped).unwrap());

        let flat_learned = r#"{"exchange":1,"session":"20261110T045515Z","ts":"2026-11-10T04:55:15Z","agent":"q","stt":"a","user":"a","learned":["x"]}"#;
        assert!(!INTERVIEW_EXCHANGE.is_valid(flat_learned).unwrap());
    }

    #[test]
    fn test_interview_session_compiles() {
        assert_eq!(INTERVIEW_SESSION.schema_name(), "interview-session");
        assert!(!INTERVIEW_SESSION.schema_json().is_empty());
    }

    #[test]
    fn test_interview_session_shape() {
        let complete = r#"{"id":"20261110T045515Z","opened":"2026-11-10T04:55:15Z","closed":"2026-11-10T11:25:09Z","about":"setup","tags":["forseti-setup"],"prior":[]}"#;
        assert!(INTERVIEW_SESSION.is_valid(complete).unwrap());

        let bad_id = r#"{"id":"2026-11-10","opened":"2026-11-10T04:55:15Z","closed":"2026-11-10T11:25:09Z","about":"setup"}"#;
        assert!(!INTERVIEW_SESSION.is_valid(bad_id).unwrap());

        let counted = r#"{"id":"20261110T045515Z","opened":"2026-11-10T04:55:15Z","closed":"2026-11-10T11:25:09Z","about":"setup","exchanges":50}"#;
        assert!(!INTERVIEW_SESSION.is_valid(counted).unwrap());
    }

    #[test]
    fn test_directory_listing_shape() {
        let listing = r#"{"root":"/r","entries":[{"path":"a","kind":"directory"},{"path":"a/b.md","kind":"file","size":3}]}"#;
        assert!(DIRECTORY_LISTING.is_valid(listing).unwrap());

        let unknown_kind = r#"{"root":"/r","entries":[{"path":"a","kind":"socket"}]}"#;
        assert!(!DIRECTORY_LISTING.is_valid(unknown_kind).unwrap());

        let short_hash = r#"{"root":"/r","entries":[{"path":"a","kind":"file","size":0,"sha256":"abc"}]}"#;
        assert!(!DIRECTORY_LISTING.is_valid(short_hash).unwrap());
    }

    #[test]
    fn test_zone_request_shape() {
        let request = r#"{"files":[{"path":"a.md","format":"markdown","schema_name":"page","data":{"frontmatter":{},"body":"x"}},{"path":"b.jpg","from":"/src/b.jpg"}]}"#;
        assert!(ZONE_REQUEST.is_valid(request).unwrap());

        let unknown_format = r#"{"files":[{"path":"a","format":"csv","schema_name":"s","data":{}}]}"#;
        assert!(!ZONE_REQUEST.is_valid(unknown_format).unwrap());

        let both_kinds = r#"{"files":[{"path":"a","from":"/x","format":"json","schema_name":"s","data":{}}]}"#;
        assert!(!ZONE_REQUEST.is_valid(both_kinds).unwrap());
    }

    #[test]
    fn test_zone_report_shape() {
        assert!(ZONE_REPORT.is_valid(r#"{"written":1,"unchanged":2,"removed":0}"#).unwrap());
        assert!(!ZONE_REPORT.is_valid(r#"{"written":-1,"unchanged":0,"removed":0}"#).unwrap());
    }

}
