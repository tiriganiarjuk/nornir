//! Check matrix — data-driven check dispatch.
//!
//! The matrix is authored in GLEIPNIR_MATRIX.md. generate_matrix_toml.py
//! compiles it into gleipnir_matrix.toml, which is locked and embedded here at
//! compile time. Each classification maps to a flat list of checks with
//! severities. No inheritance, no layers, no dedup — what's in the TOML is
//! what runs.
//!
//! To change which checks apply to a file type, edit GLEIPNIR_MATRIX.md and
//! regenerate the TOML; that file's header gives the steps.
//! To add a new check, add it to the registry below and give it a row there.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::checks_py::{architecture, imports, prohibited, style, suppression, type_safety};
use crate::messages;
use crate::structures::{CheckEntry, CheckFn, CheckMessages, FileKind, Level, Severity, V2Classification, Zone};

// =========================================================================
// Check registry: messages entry → function pointer
// =========================================================================
// This is the ONLY place where a check's name is tied to its function pointer.
// A check is named through its constant in `crate::messages`; the TOML matrix
// references checks by the names those constants carry.

const CHECK_REGISTRY: &[(&CheckMessages, CheckFn)] = &[
    // TYPE SAFETY
    (&messages::NO_ANY_TYPES, type_safety::check_no_any_types),
    (&messages::NO_ANY_TYPE_ALIASES, type_safety::check_no_any_type_aliases),
    (&messages::NO_OBJECT, type_safety::check_no_object),
    (&messages::NO_JSON_VALUE, type_safety::check_no_json_value),
    (&messages::NO_BARE_COLLECTIONS, type_safety::check_no_bare_collections),
    (&messages::NO_IMPLICIT_TYPE_ALIASES, type_safety::check_no_implicit_type_aliases),
    (&messages::UNION_MEMBER_COUNT, type_safety::check_union_member_count),
    (&messages::NO_STRING_ANNOTATIONS, type_safety::check_no_string_annotations),
    (&messages::NO_CALLABLE_PARAMS, type_safety::check_no_callable_params),
    (&messages::NO_CALLABLE_TYPE_ALIASES, type_safety::check_no_callable_type_aliases),
    (&messages::NO_OPEN_MODELS, type_safety::check_no_open_models),
    (&messages::NO_BASE_TYPE_ANNOTATIONS, type_safety::check_no_base_type_annotations),
    (&messages::NO_BUILTIN_RECORDS, type_safety::check_no_builtin_records),
    (&messages::NO_STRING_VALUE_SETS, type_safety::check_no_string_value_sets),
    // IMPORTS
    (&messages::NO_UNSAFE_IMPORTS, imports::check_no_unsafe_imports),
    (&messages::NO_RELATIVE_IMPORTS, imports::check_no_relative_imports),
    (&messages::IMPURE_MODULE_QUARANTINE, imports::check_impure_module_quarantine),
    (&messages::NO_TYPE_CHECKING_IMPORTS, imports::check_no_type_checking_imports),
    (&messages::NO_PARENT_IMPORTS, imports::check_no_parent_imports),
    (&messages::NO_DISALLOWED_STDLIB, imports::check_no_disallowed_stdlib),
    (&messages::NO_DIRECT_FILE_IO, imports::check_no_direct_file_io),
    (&messages::NO_SYS_PATH_MUTATION, imports::check_no_sys_path_mutation),
    (&messages::NO_DEFERRED_IMPORTS, imports::check_no_deferred_imports),
    (&messages::V2_IMPORT_BOUNDARIES, imports::check_v2_import_boundaries),
    (&messages::NO_BEFORE_VALIDATORS, imports::check_no_before_validators),
    // PROHIBITED
    (&messages::NO_CAST, prohibited::check_no_cast),
    (&messages::NO_OVERLOAD, prohibited::check_no_overload),
    (&messages::NO_BARE_EXCEPT, prohibited::check_no_bare_except),
    (&messages::NO_BROAD_EXCEPTIONS, prohibited::check_no_broad_exceptions),
    (&messages::NO_PRINT, prohibited::check_no_print),
    (&messages::NO_MODEL_DUMP, prohibited::check_no_model_dump),
    (&messages::NO_MODEL_BYPASS, prohibited::check_no_model_bypass),
    (&messages::NO_FUTURE_ANNOTATIONS, prohibited::check_no_future_annotations),
    (&messages::INIT_FILES_EMPTY, prohibited::check_init_files_empty),
    (&messages::NO_DUNDER_ALL, prohibited::check_no_dunder_all),
    (&messages::NO_NESTED_FUNCTIONS, prohibited::check_no_nested_functions),
    (&messages::NO_CLOSURES, prohibited::check_no_closures),
    (&messages::NO_GENERAL_LAMBDA, prohibited::check_no_general_lambda),
    (&messages::NO_DEFAULT_FACTORY, prohibited::check_no_default_factory),
    (&messages::NO_PARTIAL, prohibited::check_no_partial),
    (&messages::NO_RE_SUB, prohibited::check_no_re_sub),
    (&messages::NO_RECURSION, prohibited::check_no_recursion),
    // SUPPRESSION
    (&messages::NO_SUPPRESSION_COMMENTS, suppression::check_no_suppression_comments),
    // ARCHITECTURE
    (&messages::NO_METHODS_IN_CLASSES, architecture::check_no_methods_in_classes),
    (&messages::PYDANTIC_ONLY, architecture::check_pydantic_only),
    (&messages::GOD_CLASSES, architecture::check_god_classes),
    (&messages::NO_CALLABLE_PROTOCOL, architecture::check_no_callable_protocol),
    (&messages::NO_INLINE_DISPATCH, architecture::check_no_inline_dispatch),
    (&messages::NO_REEXPORT_SHIMS, architecture::check_no_reexport_shims),
    (&messages::HARDCODED_CONFIG, architecture::check_hardcoded_config),
    (&messages::CLASSES_ONLY_IN_STRUCTURES, architecture::check_classes_only_in_structures),
    (&messages::STRUCTURES_NO_FUNCTIONS, architecture::check_structures_no_functions),
    (&messages::STRUCTURES_IMPORT_BOUNDARY, architecture::check_structures_import_boundary),
    (&messages::IMPORT_COUNT, architecture::check_import_count),
    (&messages::MAX_FUNCTIONS_OUTSIDE_ZONES, architecture::check_max_functions_outside_zones),
    (&messages::V2_STRUCTURE_NO_LOGIC, architecture::check_v2_structure_no_logic),
    (&messages::V2_STRUCTURE_BASES, architecture::check_v2_structure_bases),
    (&messages::V2_STRUCTURE_IMPORT_BOUNDARY, architecture::check_v2_structure_import_boundary),
    (&messages::V2_LOGIC_NO_CONSTANTS, architecture::check_v2_logic_no_constants),
    (&messages::V2_DISPATCH_ONLY_TABLES, architecture::check_v2_dispatch_only_tables),
    (&messages::V2_CLASSES_ONLY_IN_STRUCTURE, architecture::check_v2_classes_only_in_structure),
    (&messages::UNKNOWN_FILE_IN_ZONE, architecture::check_unknown_file_in_zone),
    (&messages::FILE_IN_NO_ZONE, architecture::check_file_in_no_zone),
    // STYLE
    (&messages::FUNCTION_LENGTH, style::check_function_length),
    (&messages::PARAM_COUNT, style::check_param_count),
    (&messages::NESTING_DEPTH, style::check_nesting_depth),
    (&messages::NO_UNDERSCORE_PREFIX, style::check_no_underscore_prefix),
    (&messages::NO_NONE_RETURNS, style::check_no_none_returns),
    (&messages::NO_THROWAWAY_ASSIGNMENT, style::check_no_throwaway_assignment),
    (&messages::NO_SINGLE_LETTER_NAMES, style::check_no_single_letter_names),
    (&messages::NO_NUMBERED_SUFFIXES, style::check_no_numbered_suffixes),
    (&messages::SHORT_PARAM_NAMES, style::check_short_param_names),
    (&messages::SHORT_LOCAL_NAMES, style::check_short_local_names),
    (&messages::V2_CC_LEVEL, style::check_v2_cc_level),
];

// Every check the matrix names is in the registry, proven when the crate compiles.

const fn same_text(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

const fn is_registered(check_name: &str) -> bool {
    let mut index = 0;
    while index < CHECK_REGISTRY.len() {
        if same_text(CHECK_REGISTRY[index].0.name, check_name) {
            return true;
        }
        index += 1;
    }
    false
}

const _: () = {
    let mut index = 0;
    while index < messages::MATRIX_CHECK_NAMES.len() {
        assert!(
            is_registered(messages::MATRIX_CHECK_NAMES[index]),
            "gleipnir_matrix.toml names a check that CHECK_REGISTRY does not hold"
        );
        index += 1;
    }
};

// =========================================================================
// Matrix parsing (from embedded TOML)
// =========================================================================

static MATRIX_TOML: &str = include_str!("../gleipnir_matrix.toml");

struct ParsedMatrix {
    entries: HashMap<String, Vec<CheckEntry>>,
}

static MATRIX: LazyLock<ParsedMatrix> = LazyLock::new(|| {
    let registry: HashMap<&str, (&'static CheckMessages, CheckFn)> = CHECK_REGISTRY
        .iter()
        .map(|&(check, func)| (check.name, (check, func)))
        .collect();

    let raw: toml::Value =
        toml::from_str(MATRIX_TOML).expect("gleipnir_matrix.toml parse error");

    let mut entries = HashMap::new();

    for version in ["v1", "v2"] {
        let version_table = match raw.get(version).and_then(|v| v.as_table()) {
            Some(t) => t,
            None => continue,
        };

        for (classification, section) in version_table {
            let key = format!("{version}.{classification}");
            let section = section
                .as_table()
                .unwrap_or_else(|| panic!("gleipnir_matrix.toml: {key} is not a table"));

            let mut check_list = Vec::new();

            for (severity_name, severity) in [
                ("blocked", Severity::Blocked),
                ("error", Severity::Error),
                ("warning", Severity::Warning),
            ] {
                if let Some(names) = section.get(severity_name).and_then(|v| v.as_array()) {
                    for name_val in names {
                        let name = name_val.as_str().unwrap_or_else(|| {
                            panic!(
                                "gleipnir_matrix.toml: {key}.{severity_name} contains non-string"
                            )
                        });
                        let &(check, check_fn) =
                            registry.get(name).unwrap_or_else(|| {
                                panic!(
                                    "gleipnir_matrix.toml: {key} references unknown check '{name}'"
                                )
                            });
                        check_list.push(CheckEntry {
                            messages: check,
                            severity,
                            check_fn,
                        });
                    }
                }
            }

            entries.insert(key, check_list);
        }
    }

    ParsedMatrix { entries }
});

// =========================================================================
// Public API: classification → check list
// =========================================================================

/// Get checks for a v1 file classification.
pub fn checks_for_kind(kind: FileKind) -> Vec<CheckEntry> {
    let key = match kind {
        FileKind::Script => "v1.script",
        FileKind::Test => "v1.test",
        FileKind::DataStructure => "v1.data_structure",
        FileKind::UnsafeImpure => "v1.unsafe_impure",
        FileKind::UnsafePure => "v1.unsafe_pure",
        FileKind::ImpureFunction => "v1.impure_function",
        FileKind::PureFunction => "v1.pure_function",
        FileKind::Outside => "v1.outside",
    };
    MATRIX.entries.get(key).cloned().unwrap_or_default()
}

/// Get checks for a v2 zone architecture classification.
pub fn checks_for_v2(classification: &V2Classification) -> Vec<CheckEntry> {
    let key = v2_key(classification);
    MATRIX.entries.get(key).cloned().unwrap_or_default()
}

fn v2_key(classification: &V2Classification) -> &'static str {
    if classification.level == Level::L8 {
        return "v2.entry_point";
    }

    let is_dispatch = matches!(classification.level, Level::L3 | Level::L6);

    match classification.zone {
        Zone::Structure => "v2.structure",
        Zone::StructureGen => "v2.structure_gen",
        Zone::StructureModel => "v2.structure_model",
        Zone::StructureConfig => "v2.structure_config",
        Zone::StructureExample => "v2.structure_example",
        Zone::Pure if is_dispatch => "v2.pure_dispatch",
        Zone::Pure => "v2.pure",
        Zone::Impure if is_dispatch => "v2.impure_dispatch",
        Zone::Impure => "v2.impure",
        Zone::Transform if is_dispatch => "v2.transform_dispatch",
        Zone::Transform => "v2.transform",
        Zone::Orchestrate if is_dispatch => "v2.orchestrate_dispatch",
        Zone::Orchestrate => "v2.orchestrate",
        Zone::Outside => "v2.outside",
    }
}
