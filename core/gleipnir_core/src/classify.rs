//! File classification by content and path.
//!
//! Every file is classified exactly once before any checks run.
//! The classification determines which checks apply (via the matrix).

use crate::structures::{FileKind, Level, V2Classification, Zone};

/// Classify a Python source file into one of 8 kinds.
///
/// Decision tree (order matters):
/// 1. PEP 723 shebang → Script
/// 2. Test filename (`is_test_filename`) → Test
/// 3. Zone path → DataStructure / UnsafeImpure / UnsafePure / ImpureFunction / PureFunction
/// 4. Fallback → Outside
pub fn classify_file(file_path: &str, first_line: &str) -> FileKind {
    if first_line.starts_with("#!/usr/bin/env -S uv run") {
        return FileKind::Script;
    }

    let filename = file_path.rsplit('/').next().unwrap_or(file_path);
    if is_test_filename(filename) {
        return FileKind::Test;
    }

    if file_path.contains("/structures/") {
        return FileKind::DataStructure;
    }
    if file_path.contains("/unsafe/impure/") {
        return FileKind::UnsafeImpure;
    }
    if file_path.contains("/unsafe/pure/") {
        return FileKind::UnsafePure;
    }
    if file_path.contains("/functions/impure/") {
        return FileKind::ImpureFunction;
    }
    if file_path.contains("/functions/pure/") {
        return FileKind::PureFunction;
    }

    FileKind::Outside
}

/// True when a filename follows a pytest naming convention:
/// `conftest.py`, `test_<name>.py`, or `<name>_test.py`.
pub(crate) fn is_test_filename(filename: &str) -> bool {
    filename == "conftest.py"
        || (filename.starts_with("test_") && filename.ends_with(".py"))
        || filename.ends_with("_test.py")
}

// -------------------------------------------------------------------------
// V2 zone architecture classification
// -------------------------------------------------------------------------

/// Zone markers for v2 classification.
/// Maps a path component to its zone.
const V2_ZONE_MARKERS: &[(&str, Zone)] = &[
    ("/logic/pure/", Zone::Pure),
    ("/logic/impure/", Zone::Impure),
    ("/logic/transform/", Zone::Transform),
    ("/logic/orchestrate/", Zone::Orchestrate),
    // Structure sub-zones (specific before fallback)
    ("/structure/gen/", Zone::StructureGen),
    ("/structure/model/", Zone::StructureModel),
    ("/structure/config/", Zone::StructureConfig),
    ("/structure/example/", Zone::StructureExample),
    ("/structure/", Zone::Structure),
];

/// Level from filename and zone context.
/// Layout: logic/{zone}/{module_name}/{level}.py
///
/// dispatch.py maps to L3 in logic zones, L6 in orchestrate zone.
/// ffi.py and primitive.py both map to L1 (same level, different filenames).
fn level_from_filename(file_path: &str, zone: Zone) -> Level {
    let filename = file_path.rsplit('/').next().unwrap_or("");
    let stem = filename.strip_suffix(".py").unwrap_or(filename);
    match stem {
        "ffi" | "primitive" => Level::L1,
        "simple" => Level::L2,
        "dispatch" => match zone {
            Zone::Orchestrate => Level::L6,
            _ => Level::L3,
        },
        "composed" => Level::L4,
        "assembled" => Level::L5,
        "orchestrate" => Level::L7,
        _ => Level::Outside,
    }
}

/// Path markers kept for import path classification.
/// Import paths use the directory-as-level convention:
///   mypackage.logic.pure.composed.module
const V2_IMPORT_PATH_MARKERS: &[(&str, Level, Zone)] = &[
    // Pure track
    ("/logic/pure/ffi/", Level::L1, Zone::Pure),
    ("/logic/pure/primitive/", Level::L1, Zone::Pure),
    ("/logic/pure/simple/", Level::L2, Zone::Pure),
    ("/logic/pure/dispatch/", Level::L3, Zone::Pure),
    ("/logic/pure/composed/", Level::L4, Zone::Pure),
    // Impure track
    ("/logic/impure/ffi/", Level::L1, Zone::Impure),
    ("/logic/impure/primitive/", Level::L1, Zone::Impure),
    ("/logic/impure/simple/", Level::L2, Zone::Impure),
    ("/logic/impure/dispatch/", Level::L3, Zone::Impure),
    ("/logic/impure/composed/", Level::L4, Zone::Impure),
    // Transform track
    ("/logic/transform/ffi/", Level::L1, Zone::Transform),
    ("/logic/transform/primitive/", Level::L1, Zone::Transform),
    ("/logic/transform/simple/", Level::L2, Zone::Transform),
    ("/logic/transform/dispatch/", Level::L3, Zone::Transform),
    ("/logic/transform/composed/", Level::L4, Zone::Transform),
    // Orchestrate
    ("/logic/orchestrate/", Level::L7, Zone::Orchestrate),
    // Structure (singular only)
    ("/structure/", Level::L0, Zone::Structure),
];

/// Entry point filenames at project root level.
const ENTRY_POINT_NAMES: &[&str] = &["cli.py", "__main__.py"];

/// Classify a Python source file into the v2 zone architecture.
///
/// Two-step classification:
/// 1. Zone from path directory (/logic/pure/, /structure/, etc.)
/// 2. Level from filename (composed.py, simple.py, etc.)
///
/// Special cases:
/// - Structure zone: level is always Structure (no per-file level)
/// - Orchestrate zone: only orchestrate.py and dispatch.py are valid levels
/// - Pure/Impure/Transform zones: ffi, primitive, simple, dispatch, composed, assembled
/// - Entry point files (cli.py, __main__.py): EntryPoint + Orchestrate
/// - __init__.py: inherits zone, level is Outside (no checks needed)
/// - Invalid level for zone (e.g. orchestrate.py in pure/): forced to Outside
///
/// Files matching no zone get Level::Outside — a sentinel that fails
/// all import checks. In a v2 project, every file must be in a zone.
pub fn classify_file_v2(file_path: &str) -> V2Classification {
    let filename = file_path.rsplit('/').next().unwrap_or(file_path);

    // Entry points detected by filename
    if ENTRY_POINT_NAMES.contains(&filename) {
        return V2Classification {
            level: Level::L8,
            zone: Zone::Orchestrate,
        };
    }

    // Find zone from path
    let zone = V2_ZONE_MARKERS.iter().find_map(|&(pattern, zone)| {
        if file_path.contains(pattern) { Some(zone) } else { None }
    });

    let Some(zone) = zone else {
        return V2Classification {
            level: Level::Outside,
            zone: Zone::Outside,
        };
    };

    // Structure zones have a fixed level (L0)
    if zone.is_structure() {
        return V2Classification { level: Level::L0, zone };
    }

    // __init__.py files inherit zone but have no level checks
    if filename == "__init__.py" {
        return V2Classification { level: Level::Outside, zone };
    }

    // Level from filename + zone context
    let level = level_from_filename(file_path, zone);

    // Validate level is legal for this zone
    let valid = match zone {
        Zone::Pure | Zone::Impure | Zone::Transform => matches!(
            level,
            Level::L1 | Level::L2 | Level::L3 | Level::L4 | Level::L5 | Level::Outside
        ),
        Zone::Orchestrate => matches!(
            level,
            Level::L6 | Level::L7 | Level::Outside
        ),
        _ => true, // structure zones handled by early return above
    };
    let level = if valid { level } else { Level::Outside };

    V2Classification { level, zone }
}

/// Classify a dotted import path (e.g. "regin.logic.pure.codegen_transforms.composed")
/// into v2 zone/level. Returns None if the path contains no v2 markers.
///
/// Two strategies:
/// 1. Level from last segment: "regin.logic.pure.module.composed" → Composed + Pure
/// 2. Level from directory position: "regin.logic.pure.composed.module" → Composed + Pure
/// Strategy 1 matches the file layout (level as filename).
/// Strategy 2 is kept for backward compatibility.
pub fn classify_import_path(dotted_path: &str) -> Option<V2Classification> {
    let slashed = dotted_path.replace('.', "/");
    let probe = format!("/{slashed}/");

    // Find zone first
    let zone = V2_ZONE_MARKERS.iter().find_map(|&(pattern, zone)| {
        if probe.contains(pattern) { Some(zone) } else { None }
    })?;

    // Structure zones have a fixed level (L0)
    if zone.is_structure() {
        return Some(V2Classification { level: Level::L0, zone });
    }

    // Try level from last segment (filename convention)
    let last_segment = dotted_path.rsplit('.').next().unwrap_or("");
    let level_from_last = match last_segment {
        "ffi" | "primitive" => Some(Level::L1),
        "simple" => Some(Level::L2),
        "dispatch" => {
            // Zone-aware: orchestrate zone dispatch is L6, logic zone dispatch is L3
            if zone == Zone::Orchestrate { Some(Level::L6) } else { Some(Level::L3) }
        },
        "composed" => Some(Level::L4),
        "assembled" => Some(Level::L5),
        "orchestrate" => Some(Level::L7),
        _ => None,
    };
    if let Some(level) = level_from_last {
        return Some(V2Classification { level, zone });
    }

    // Try level from directory position (backward compat)
    for &(pattern, level, pzone) in V2_IMPORT_PATH_MARKERS {
        if probe.contains(pattern) && pzone == zone {
            return Some(V2Classification { level, zone });
        }
    }

    // In zone but level unknown — caller decides significance
    Some(V2Classification { level: Level::Outside, zone })
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- V1 classification (unchanged) --

    #[test]
    fn script_detected_by_shebang() {
        assert_eq!(
            classify_file("/project/tools/run.py", "#!/usr/bin/env -S uv run"),
            FileKind::Script,
        );
    }

    #[test]
    fn shebang_takes_priority_over_path() {
        assert_eq!(
            classify_file(
                "/project/src/pkg/functions/pure/tool.py",
                "#!/usr/bin/env -S uv run",
            ),
            FileKind::Script,
        );
    }

    #[test]
    fn test_file_detected_by_name() {
        assert_eq!(
            classify_file("/project/tests/test_foo.py", "import pytest"),
            FileKind::Test,
        );
    }

    #[test]
    fn test_in_name_takes_priority_over_path() {
        assert_eq!(
            classify_file(
                "/project/src/pkg/structures/test_models.py",
                "from pydantic import BaseModel",
            ),
            FileKind::Test,
        );
    }

    #[test]
    fn pytest_filenames_are_test_files() {
        for filename in ["test_foo.py", "foo_test.py", "conftest.py"] {
            assert!(is_test_filename(filename), "{filename} is a test file");
        }
    }

    #[test]
    fn filenames_merely_containing_test_are_not_test_files() {
        for filename in ["latest.py", "contest.py", "attestation.py", "testing_tools.py", "test.py"] {
            assert!(!is_test_filename(filename), "{filename} is not a test file");
        }
    }

    #[test]
    fn file_merely_containing_test_in_name_classified_by_path() {
        assert_eq!(
            classify_file("/project/src/pkg/functions/pure/latest.py", "def newest():"),
            FileKind::PureFunction,
        );
    }

    #[test]
    fn structures_path() {
        // V1 still recognizes structures/ (plural)
        assert_eq!(
            classify_file("/project/src/pkg/structures/models.py", "from pydantic import BaseModel"),
            FileKind::DataStructure,
        );
    }

    #[test]
    fn unsafe_impure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/unsafe/impure/io.py", "import os"),
            FileKind::UnsafeImpure,
        );
    }

    #[test]
    fn unsafe_pure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/unsafe/pure/cast.py", "from typing import cast"),
            FileKind::UnsafePure,
        );
    }

    #[test]
    fn functions_impure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/functions/impure/loader.py", "import json"),
            FileKind::ImpureFunction,
        );
    }

    #[test]
    fn functions_pure_path() {
        assert_eq!(
            classify_file("/project/src/pkg/functions/pure/compute.py", "def add(a, b):"),
            FileKind::PureFunction,
        );
    }

    #[test]
    fn outside_file() {
        assert_eq!(
            classify_file("/project/src/app.py", "import sys"),
            FileKind::Outside,
        );
    }

    // -- V2 classification: logic zones --

    #[test]
    fn v2_pure_primitive() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/helpers/primitive.py");
        assert_eq!(c.level, Level::L1);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_pure_ffi() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/rust_binding/ffi.py");
        assert_eq!(c.level, Level::L1); // ffi and primitive are both L1
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_pure_simple() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/validators/simple.py");
        assert_eq!(c.level, Level::L2);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_pure_dispatch() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/converters/dispatch.py");
        assert_eq!(c.level, Level::L3); // L3 in logic zones
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_pure_composed() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/pipeline/composed.py");
        assert_eq!(c.level, Level::L4);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_impure_primitive() {
        let c = classify_file_v2("/project/src/pkg/logic/impure/io_ops/primitive.py");
        assert_eq!(c.level, Level::L1);
        assert_eq!(c.zone, Zone::Impure);
    }

    #[test]
    fn v2_impure_composed() {
        let c = classify_file_v2("/project/src/pkg/logic/impure/workflow/composed.py");
        assert_eq!(c.level, Level::L4);
        assert_eq!(c.zone, Zone::Impure);
    }

    #[test]
    fn v2_transform_simple() {
        let c = classify_file_v2("/project/src/pkg/logic/transform/coerce/simple.py");
        assert_eq!(c.level, Level::L2);
        assert_eq!(c.zone, Zone::Transform);
    }

    #[test]
    fn v2_transform_primitive() {
        let c = classify_file_v2("/project/src/pkg/logic/transform/reshape/primitive.py");
        assert_eq!(c.level, Level::L1);
        assert_eq!(c.zone, Zone::Transform);
    }

    #[test]
    fn v2_transform_dispatch() {
        let c = classify_file_v2("/project/src/pkg/logic/transform/field_map/dispatch.py");
        assert_eq!(c.level, Level::L3); // L3 in logic zones
        assert_eq!(c.zone, Zone::Transform);
    }

    // -- V2 classification: orchestrate zone --

    #[test]
    fn v2_orchestrate() {
        let c = classify_file_v2("/project/src/pkg/logic/orchestrate/main_pipeline/orchestrate.py");
        assert_eq!(c.level, Level::L7);
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_orchestrate_dispatch() {
        let c = classify_file_v2("/project/src/pkg/logic/orchestrate/main_pipeline/dispatch.py");
        assert_eq!(c.level, Level::L6); // L6 in orchestrate zone (not L3!)
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    // -- V2 classification: invalid levels for zone --

    #[test]
    fn v2_orchestrate_py_in_pure_rejected() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/module/orchestrate.py");
        assert_eq!(c.level, Level::Outside);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn v2_primitive_in_orchestrate_rejected() {
        let c = classify_file_v2("/project/src/pkg/logic/orchestrate/module/primitive.py");
        assert_eq!(c.level, Level::Outside);
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_assembled_in_orchestrate_rejected() {
        let c = classify_file_v2("/project/src/pkg/logic/orchestrate/module/assembled.py");
        assert_eq!(c.level, Level::Outside);
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_composed_in_orchestrate_rejected() {
        let c = classify_file_v2("/project/src/pkg/logic/orchestrate/module/composed.py");
        assert_eq!(c.level, Level::Outside);
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    // -- V2 classification: special cases --

    #[test]
    fn v2_init_inherits_zone() {
        let c = classify_file_v2("/project/src/pkg/logic/pure/helpers/__init__.py");
        assert_eq!(c.zone, Zone::Pure);
        assert_eq!(c.level, Level::Outside);
    }

    #[test]
    fn v2_structure_singular() {
        let c = classify_file_v2("/project/src/pkg/structure/models.py");
        assert_eq!(c.level, Level::L0);
        assert_eq!(c.zone, Zone::Structure);
    }

    #[test]
    fn v2_structure_gen() {
        let c = classify_file_v2("/project/src/pkg/structure/gen/output_structure.py");
        assert_eq!(c.level, Level::L0);
        assert_eq!(c.zone, Zone::StructureGen);
    }

    #[test]
    fn v2_structure_model() {
        let c = classify_file_v2("/project/src/pkg/structure/model/section_buffer.py");
        assert_eq!(c.level, Level::L0);
        assert_eq!(c.zone, Zone::StructureModel);
    }

    #[test]
    fn v2_structure_config() {
        let c = classify_file_v2("/project/src/pkg/structure/config/capability_tiers.py");
        assert_eq!(c.level, Level::L0);
        assert_eq!(c.zone, Zone::StructureConfig);
    }

    #[test]
    fn v2_structure_example() {
        let c = classify_file_v2("/project/src/pkg/structure/example/fixtures.py");
        assert_eq!(c.level, Level::L0);
        assert_eq!(c.zone, Zone::StructureExample);
    }

    #[test]
    fn v2_structure_fallback() {
        // Unknown structure sub-dir falls back to Structure
        let c = classify_file_v2("/project/src/pkg/structure/exception/custom.py");
        assert_eq!(c.level, Level::L0);
        assert_eq!(c.zone, Zone::Structure);
    }

    #[test]
    fn v2_structures_plural_not_recognized() {
        // V2 only recognizes structure/ (singular)
        let c = classify_file_v2("/project/src/pkg/structures/schema.py");
        assert_eq!(c.level, Level::Outside); // no zone match
    }

    #[test]
    fn v2_entry_point_cli() {
        let c = classify_file_v2("/project/src/pkg/cli.py");
        assert_eq!(c.level, Level::L8);
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_entry_point_main() {
        let c = classify_file_v2("/project/src/pkg/__main__.py");
        assert_eq!(c.level, Level::L8);
        assert_eq!(c.zone, Zone::Orchestrate);
    }

    #[test]
    fn v2_outside_file() {
        let c = classify_file_v2("/project/src/pkg/random_file.py");
        assert_eq!(c.level, Level::Outside);
    }

    // -- Import path classification --

    #[test]
    fn import_path_pure_composed_from_filename() {
        let c = classify_import_path("regin.logic.pure.codegen_transforms.composed").unwrap();
        assert_eq!(c.level, Level::L4);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn import_path_impure_primitive_from_filename() {
        let c = classify_import_path("regin.logic.impure.gates.primitive").unwrap();
        assert_eq!(c.level, Level::L1);
        assert_eq!(c.zone, Zone::Impure);
    }

    #[test]
    fn import_path_transform_simple_from_filename() {
        let c = classify_import_path("regin.logic.transform.coerce.simple").unwrap();
        assert_eq!(c.level, Level::L2);
        assert_eq!(c.zone, Zone::Transform);
    }

    #[test]
    fn import_path_pure_simple_from_directory() {
        let c = classify_import_path("mypackage.logic.pure.simple.helpers").unwrap();
        assert_eq!(c.level, Level::L2);
        assert_eq!(c.zone, Zone::Pure);
    }

    #[test]
    fn import_path_impure_primitive_from_directory() {
        let c = classify_import_path("mypackage.logic.impure.primitive.io").unwrap();
        assert_eq!(c.level, Level::L1);
        assert_eq!(c.zone, Zone::Impure);
    }

    #[test]
    fn import_path_structure_singular() {
        let c = classify_import_path("regin.structure.gen.schema.agent_raw_definition").unwrap();
        assert_eq!(c.level, Level::L0);
        assert_eq!(c.zone, Zone::StructureGen);
    }

    #[test]
    fn import_path_structures_plural_not_recognized() {
        // V2 import paths only recognize structure/ (singular)
        assert!(classify_import_path("mypackage.structures.models").is_none());
    }

    #[test]
    fn import_path_external_returns_none() {
        assert!(classify_import_path("pydantic.BaseModel").is_none());
        assert!(classify_import_path("os.path").is_none());
    }

    #[test]
    fn import_path_orchestrate_dispatch() {
        let c = classify_import_path("draupnir.logic.orchestrate.compile.dispatch").unwrap();
        assert_eq!(c.level, Level::L6); // orchestrate zone dispatch
        assert_eq!(c.zone, Zone::Orchestrate);
    }
}
