//! Check matrix — data-driven check dispatch.
//!
//! The matrix is defined in gleipnir_matrix.toml (embedded at compile time).
//! Each classification maps to a flat list of checks with severities.
//! No inheritance, no layers, no dedup — what's in the TOML is what runs.
//!
//! To change which checks apply to a file type, edit gleipnir_matrix.toml.
//! To add a new check, add it to the registry below AND to gleipnir_matrix.toml.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::checks_py::{architecture, imports, prohibited, style, suppression, type_safety};
use crate::structures::{CheckEntry, CheckFn, FileKind, Level, Severity, V2Classification, Zone};

// =========================================================================
// Check registry: name → function pointer
// =========================================================================
// This is the ONLY place where check names are tied to function pointers.
// The TOML matrix references checks by these names.

static CHECK_REGISTRY: &[(&str, CheckFn)] = &[
    // TYPE SAFETY
    ("no_any_types", type_safety::check_no_any_types),
    ("no_any_type_aliases", type_safety::check_no_any_type_aliases),
    ("no_object", type_safety::check_no_object),
    ("no_json_value", type_safety::check_no_json_value),
    ("no_bare_collections", type_safety::check_no_bare_collections),
    ("no_implicit_type_aliases", type_safety::check_no_implicit_type_aliases),
    ("union_member_count", type_safety::check_union_member_count),
    ("no_string_annotations", type_safety::check_no_string_annotations),
    ("no_callable_params", type_safety::check_no_callable_params),
    ("no_callable_type_aliases", type_safety::check_no_callable_type_aliases),
    // IMPORTS
    ("no_unsafe_imports", imports::check_no_unsafe_imports),
    ("no_relative_imports", imports::check_no_relative_imports),
    ("impure_module_quarantine", imports::check_impure_module_quarantine),
    ("no_type_checking_imports", imports::check_no_type_checking_imports),
    ("no_parent_imports", imports::check_no_parent_imports),
    ("no_disallowed_stdlib", imports::check_no_disallowed_stdlib),
    ("no_direct_file_io", imports::check_no_direct_file_io),
    ("no_sys_path_mutation", imports::check_no_sys_path_mutation),
    ("no_deferred_imports", imports::check_no_deferred_imports),
    ("v2_import_boundaries", imports::check_v2_import_boundaries),
    ("no_before_validators", imports::check_no_before_validators),
    // PROHIBITED
    ("no_cast", prohibited::check_no_cast),
    ("no_overload", prohibited::check_no_overload),
    ("no_bare_except", prohibited::check_no_bare_except),
    ("no_broad_exceptions", prohibited::check_no_broad_exceptions),
    ("no_print", prohibited::check_no_print),
    ("no_model_dump", prohibited::check_no_model_dump),
    ("no_future_annotations", prohibited::check_no_future_annotations),
    ("init_files_empty", prohibited::check_init_files_empty),
    ("no_dunder_all", prohibited::check_no_dunder_all),
    ("no_nested_functions", prohibited::check_no_nested_functions),
    ("no_general_lambda", prohibited::check_no_general_lambda),
    ("no_default_factory", prohibited::check_no_default_factory),
    ("no_partial", prohibited::check_no_partial),
    ("no_re_sub", prohibited::check_no_re_sub),
    ("no_recursion", prohibited::check_no_recursion),
    // SUPPRESSION
    ("no_suppression_comments", suppression::check_no_suppression_comments),
    // ARCHITECTURE
    ("no_methods_in_classes", architecture::check_no_methods_in_classes),
    ("pydantic_only", architecture::check_pydantic_only),
    ("god_classes", architecture::check_god_classes),
    ("no_callable_protocol", architecture::check_no_callable_protocol),
    ("no_inline_dispatch", architecture::check_no_inline_dispatch),
    ("no_reexport_shims", architecture::check_no_reexport_shims),
    ("hardcoded_config", architecture::check_hardcoded_config),
    ("classes_only_in_structures", architecture::check_classes_only_in_structures),
    ("structures_no_functions", architecture::check_structures_no_functions),
    ("structures_import_boundary", architecture::check_structures_import_boundary),
    ("import_count", architecture::check_import_count),
    ("max_functions_outside_zones", architecture::check_max_functions_outside_zones),
    ("v2_structure_no_logic", architecture::check_v2_structure_no_logic),
    ("v2_structure_bases", architecture::check_v2_structure_bases),
    ("v2_structure_import_boundary", architecture::check_v2_structure_import_boundary),
    ("v2_logic_no_constants", architecture::check_v2_logic_no_constants),
    ("v2_dispatch_only_tables", architecture::check_v2_dispatch_only_tables),
    ("v2_classes_only_in_structure", architecture::check_v2_classes_only_in_structure),
    ("unknown_file_in_zone", architecture::check_unknown_file_in_zone),
    // STYLE
    ("function_length", style::check_function_length),
    ("param_count", style::check_param_count),
    ("nesting_depth", style::check_nesting_depth),
    ("no_underscore_prefix", style::check_no_underscore_prefix),
    ("no_none_returns", style::check_no_none_returns),
    ("no_throwaway_assignment", style::check_no_throwaway_assignment),
    ("no_single_letter_names", style::check_no_single_letter_names),
    ("no_numbered_suffixes", style::check_no_numbered_suffixes),
    ("short_param_names", style::check_short_param_names),
    ("short_local_names", style::check_short_local_names),
    ("v2_cc_level", style::check_v2_cc_level),
];

// =========================================================================
// Matrix parsing (from embedded TOML)
// =========================================================================

static MATRIX_TOML: &str = include_str!("../gleipnir_matrix.toml");

struct ParsedMatrix {
    entries: HashMap<String, Vec<CheckEntry>>,
}

static MATRIX: LazyLock<ParsedMatrix> = LazyLock::new(|| {
    let registry: HashMap<&str, (&'static str, CheckFn)> = CHECK_REGISTRY
        .iter()
        .map(|&(name, func)| (name, (name, func)))
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
                        let &(static_name, check_fn) =
                            registry.get(name).unwrap_or_else(|| {
                                panic!(
                                    "gleipnir_matrix.toml: {key} references unknown check '{name}'"
                                )
                            });
                        check_list.push(CheckEntry {
                            name: static_name,
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
    }
}
