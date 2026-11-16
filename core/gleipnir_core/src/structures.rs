//! Core data structures for gleipnir guardrail checks.

/// Classification of a Python source file by content and path (v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Script,
    Test,
    DataStructure,
    UnsafeImpure,
    UnsafePure,
    ImpureFunction,
    PureFunction,
    Outside,
}

// -------------------------------------------------------------------------
// V2 zone architecture types
// -------------------------------------------------------------------------

/// Numbered composition level in the v2 zone architecture.
///
/// Levels are immutable positions in the import hierarchy. The import rule
/// is a single numeric comparison: `source > target`. No lookup table.
///
/// L0=structure, L1=primitive/ffi, L2=simple, L3=dispatch(logic),
/// L4=composed, L5=assembled, L6=dispatch(orchestrate), L7=orchestrate,
/// L8=entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    L0,      // structure — data shapes
    L1,      // primitive + ffi — leaf functions
    L2,      // simple — building blocks
    L3,      // dispatch (logic zones) — thin routing
    L4,      // composed — business logic
    L5,      // assembled — thin composition
    L6,      // dispatch (orchestrate zone) — routing for L7
    L7,      // orchestrate — pipeline wiring
    L8,      // entry point — thinnest wrapper
    Outside, // sentinel for unclassified files
}

impl Level {
    /// Numeric position in the hierarchy. None for Outside (unclassified).
    pub fn ordinal(self) -> Option<u8> {
        match self {
            Level::L0 => Some(0),
            Level::L1 => Some(1),
            Level::L2 => Some(2),
            Level::L3 => Some(3),
            Level::L4 => Some(4),
            Level::L5 => Some(5),
            Level::L6 => Some(6),
            Level::L7 => Some(7),
            Level::L8 => Some(8),
            Level::Outside => None,
        }
    }

    /// Check whether a file at this level may import from `target` level.
    ///
    /// The rule is: `source > target` (strictly greater ordinal).
    /// Outside cannot import anything. Structure (L0) is accessible from
    /// every logic level because every level number > 0.
    pub fn can_import(self, target: Level) -> bool {
        match (self.ordinal(), target.ordinal()) {
            (Some(s), Some(t)) => s > t,
            _ => false,
        }
    }
}

/// Zone track in the v2 zone architecture.
///
/// Determines which other zones a file may import from.
/// Structure is reachable from all zones via the level matrix, not the zone matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Structure,
    StructureGen,
    StructureModel,
    StructureConfig,
    StructureExample,
    Pure,
    Impure,
    Transform,
    Orchestrate,
    /// The file lies in no zone of the layout.
    Outside,
}

impl Zone {
    /// True if this zone is any structure sub-zone (including the fallback).
    pub fn is_structure(self) -> bool {
        matches!(
            self,
            Zone::Structure
                | Zone::StructureGen
                | Zone::StructureModel
                | Zone::StructureConfig
                | Zone::StructureExample
        )
    }
}

impl Zone {
    /// Check whether a file in this zone may import from `target` zone.
    ///
    /// Encodes the zone matrix from V2_ZONE_ARCHITECTURE.md.
    /// Structure reachability is handled by Level::can_import, not here.
    pub fn can_reach(self, target: Zone) -> bool {
        use Zone::*;
        match self {
            Structure | StructureGen | StructureModel | StructureConfig | StructureExample | Outside => false,
            Pure => matches!(target, Pure),
            Impure => matches!(target, Impure | Pure),
            Transform => matches!(target, Transform),
            Orchestrate => matches!(target, Orchestrate | Pure | Impure | Transform),
        }
    }
}

/// V2 file classification: zone + level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V2Classification {
    pub level: Level,
    pub zone: Zone,
}

impl V2Classification {
    /// Check whether a file with this classification may import from a target.
    ///
    /// Both the level check and the zone check must pass.
    /// Structure targets are checked via level only (zone matrix skipped).
    pub fn can_import(self, target: V2Classification) -> bool {
        if !self.level.can_import(target.level) {
            return false;
        }
        if target.zone.is_structure() {
            return true;
        }
        self.zone.can_reach(target.zone)
    }
}

/// Violation severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Blocked,
    Error,
    Warning,
}

/// A single guardrail violation detected by a check.
#[derive(Debug, Clone)]
pub struct Violation {
    pub line: usize,
    pub check_name: String,
    pub severity: Severity,
    pub message: String,
    pub detail: String,
    pub signal: String,
    pub direction: String,
    pub canary: String,
}

/// Per-file check configuration. Values loaded from gleipnir_statistics.toml.
#[derive(Debug, Clone)]
pub struct CheckConfig {
    pub max_function_lines: usize,
    pub max_function_params: usize,
    pub max_nesting_depth: usize,
    pub max_functions_outside_zones: usize,
    pub max_simple_union_members: usize,
    pub max_named_union_members: usize,
    pub min_param_length: usize,
    pub max_function_imports: usize,
    pub max_other_imports: usize,
    pub min_cc: usize,
    pub max_cc: usize,
    pub min_functions_for_pub_check: usize,
}

impl CheckConfig {
    /// Build config for a v1 Python FileKind from parsed statistics.
    pub fn for_kind(kind: FileKind, stats: &StatisticsToml) -> Self {
        let key = match kind {
            FileKind::PureFunction => Some("pure_function"),
            FileKind::ImpureFunction => Some("impure_function"),
            FileKind::UnsafePure => Some("unsafe_pure"),
            FileKind::UnsafeImpure => Some("unsafe_impure"),
            FileKind::DataStructure => Some("data_structure"),
            FileKind::Script => Some("script"),
            FileKind::Test => Some("test"),
            FileKind::Outside => Some("outside"),
        };
        let override_values = key.and_then(|k| {
            stats.v1.as_ref().and_then(|m| m.get(k))
        });
        Self::resolve(override_values, &stats.defaults)
    }

    /// Build config for a v2 level from parsed statistics.
    /// Zone is used to apply zone-specific overrides (e.g. transform has tighter CC/nesting).
    pub fn for_v2(level: Level, zone: Zone, stats: &StatisticsToml) -> Self {
        let key = match level {
            Level::L0 => None,              // structure — no level-specific config
            Level::L1 => Some("l1"),
            Level::L2 => Some("l2"),
            Level::L3 => Some("l3"),
            Level::L4 => Some("l4"),
            Level::L5 => Some("l5"),
            Level::L6 => Some("l6"),
            Level::L7 => Some("l7"),
            // A file with no level is held to the outermost level's length,
            // parameter and nesting limits. It has no complexity band (below).
            Level::L8 | Level::Outside => Some("l8"),
        };
        let v2_entry = key.and_then(|k| {
            stats.v2.as_ref().and_then(|m| m.levels.get(k))
        });
        let override_values = v2_entry.map(|e| &e.values);
        let mut config = Self::resolve(override_values, &stats.defaults);
        if let Some(entry) = v2_entry {
            config.min_cc = entry.min_cc.unwrap_or(0);
            config.max_cc = entry.max_cc.unwrap_or(usize::MAX);
        }
        if level == Level::Outside {
            config.min_cc = 0;
            config.max_cc = usize::MAX;
        }
        // Apply zone-specific overrides (e.g. [v2.transform.l2])
        if let Some(level_key) = key {
            if let Some(zone_entry) = stats.v2_zone_override(zone, level_key) {
                if let Some(cc) = zone_entry.max_cc {
                    config.max_cc = cc;
                }
                if let Some(cc) = zone_entry.min_cc {
                    config.min_cc = cc;
                }
                if let Some(nd) = zone_entry.values.max_nesting_depth {
                    config.max_nesting_depth = nd;
                }
            }
        }
        config
    }

    /// Build config for Rust files from parsed statistics.
    pub fn for_rust(stats: &StatisticsToml) -> Self {
        Self::resolve(stats.rust.as_ref(), &stats.defaults)
    }

    /// Build config for TypeScript/Svelte files from parsed statistics.
    pub fn for_typescript(stats: &StatisticsToml) -> Self {
        Self::resolve(stats.typescript.as_ref(), &stats.defaults)
    }

    /// Resolve a config by overlaying specific values onto defaults.
    fn resolve(specific: Option<&StatisticValues>, defaults: &StatisticValues) -> Self {
        let get = |f: fn(&StatisticValues) -> Option<usize>| -> usize {
            specific.and_then(|s| f(s)).or_else(|| f(defaults)).unwrap_or(0)
        };
        Self {
            max_function_lines: get(|v| v.max_function_lines),
            max_function_params: get(|v| v.max_function_params),
            max_nesting_depth: get(|v| v.max_nesting_depth),
            max_functions_outside_zones: get(|v| v.max_functions_outside_zones),
            max_simple_union_members: get(|v| v.max_simple_union_members),
            max_named_union_members: get(|v| v.max_named_union_members),
            min_param_length: get(|v| v.min_param_length),
            max_function_imports: get(|v| v.max_function_imports),
            max_other_imports: get(|v| v.max_other_imports),
            min_cc: 0,
            max_cc: usize::MAX,
            min_functions_for_pub_check: get(|v| v.min_functions_for_pub_check),
        }
    }
}

// -------------------------------------------------------------------------
// Statistics loaded from embedded TOML
// -------------------------------------------------------------------------

/// Top-level structure of gleipnir_statistics.toml.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct StatisticsToml {
    pub defaults: StatisticValues,
    #[serde(default)]
    pub rust: Option<StatisticValues>,
    #[serde(default)]
    pub typescript: Option<StatisticValues>,
    #[serde(default)]
    pub v1: Option<std::collections::HashMap<String, StatisticValues>>,
    #[serde(default)]
    pub v2: Option<V2Section>,
}

impl StatisticsToml {
    /// Look up a zone-specific override for a v2 level.
    /// Returns the zone override entry if `[v2.{zone}.{level}]` exists.
    pub fn v2_zone_override(&self, zone: Zone, level_key: &str) -> Option<&V2LevelEntry> {
        let section = self.v2.as_ref()?;
        let zone_levels = match zone {
            Zone::Transform => section.transform.as_ref()?,
            _ => return None, // Only transform has overrides for now
        };
        zone_levels.get(level_key)
    }
}

/// The `[v2]` table: one entry per level (`[v2.l1]` ...), plus the
/// transform zone's per-level overrides (`[v2.transform.l1]` ...).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct V2Section {
    #[serde(default)]
    pub transform: Option<std::collections::HashMap<String, V2LevelEntry>>,
    #[serde(flatten)]
    pub levels: std::collections::HashMap<String, V2LevelEntry>,
}

/// Numeric parameters. All fields optional — absence means "inherit from defaults".
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct StatisticValues {
    pub max_function_lines: Option<usize>,
    pub max_function_params: Option<usize>,
    pub max_nesting_depth: Option<usize>,
    pub min_param_length: Option<usize>,
    pub max_simple_union_members: Option<usize>,
    pub max_named_union_members: Option<usize>,
    pub max_functions_outside_zones: Option<usize>,
    pub max_function_imports: Option<usize>,
    pub max_other_imports: Option<usize>,
    pub min_functions_for_pub_check: Option<usize>,
}

/// V2 level entry: standard values plus CC bounds.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct V2LevelEntry {
    pub min_cc: Option<usize>,
    pub max_cc: Option<usize>,
    #[serde(flatten)]
    pub values: StatisticValues,
}

/// Parsed Python source file. Borrows source bytes for zero-copy access.
pub struct ParsedSource<'a> {
    pub file_path: &'a str,
    pub source_bytes: &'a [u8],
    pub lines: Vec<&'a str>,
    pub tree: tree_sitter::Tree,
}

/// Check function signature. Every check conforms to this.
pub type CheckFn = fn(&ParsedSource, &CheckConfig) -> Vec<Violation>;

/// Registry entry for a single check.
#[derive(Clone)]
pub struct CheckEntry {
    pub messages: &'static CheckMessages,
    pub severity: Severity,
    pub check_fn: CheckFn,
}

// -------------------------------------------------------------------------
// Messages loaded from embedded TOML
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Level: numeric ordering truth table --

    #[test]
    fn higher_imports_lower() {
        // Every level can import any strictly lower level
        let levels = [Level::L0, Level::L1, Level::L2, Level::L3, Level::L4, Level::L5, Level::L6, Level::L7, Level::L8];
        for (i, &source) in levels.iter().enumerate() {
            for (j, &target) in levels.iter().enumerate() {
                let expected = i > j;
                assert_eq!(
                    source.can_import(target), expected,
                    "{source:?} importing {target:?}: expected {expected}"
                );
            }
        }
    }

    #[test]
    fn same_level_blocked() {
        for level in [Level::L1, Level::L2, Level::L3, Level::L4, Level::L5, Level::L6, Level::L7, Level::L8] {
            assert!(!level.can_import(level), "{level:?} should not import itself");
        }
    }

    #[test]
    fn l0_cannot_import_l0() {
        // L0 (structure) same-level is blocked by numeric rule (0 > 0 is false).
        // Structure internal imports are handled by the structure boundary check, not level check.
        assert!(!Level::L0.can_import(Level::L0));
    }

    #[test]
    fn outside_imports_nothing() {
        for target in [Level::L0, Level::L1, Level::L2, Level::L3, Level::L4, Level::L5, Level::L6, Level::L7, Level::L8, Level::Outside] {
            assert!(!Level::Outside.can_import(target), "Outside should not import {target:?}");
        }
    }

    #[test]
    fn nothing_imports_outside() {
        for source in [Level::L0, Level::L1, Level::L2, Level::L3, Level::L4, Level::L5, Level::L6, Level::L7, Level::L8] {
            assert!(!source.can_import(Level::Outside), "{source:?} should not import Outside");
        }
    }

    #[test]
    fn l6_dispatch_imports_l5_assembled() {
        // Key test: orchestrate-zone dispatch (L6) can import assembled (L5)
        assert!(Level::L6.can_import(Level::L5));
        assert!(Level::L6.can_import(Level::L4));
        assert!(Level::L6.can_import(Level::L3));
        assert!(Level::L6.can_import(Level::L2));
        assert!(Level::L6.can_import(Level::L1));
        assert!(Level::L6.can_import(Level::L0));
    }

    #[test]
    fn l7_imports_l6() {
        // orchestrate.py (L7) can import dispatch.py in orchestrate zone (L6)
        assert!(Level::L7.can_import(Level::L6));
    }

    #[test]
    fn l8_imports_everything_below() {
        // Entry point follows pure numeric rule — no special cases
        assert!(Level::L8.can_import(Level::L7));
        assert!(Level::L8.can_import(Level::L6));
        assert!(Level::L8.can_import(Level::L0));
        assert!(!Level::L8.can_import(Level::L8));
    }

    #[test]
    fn ordinal_values() {
        assert_eq!(Level::L0.ordinal(), Some(0));
        assert_eq!(Level::L1.ordinal(), Some(1));
        assert_eq!(Level::L8.ordinal(), Some(8));
        assert_eq!(Level::Outside.ordinal(), None);
    }

    // -- Zone matrix truth table (unchanged) --

    #[test]
    fn pure_reaches_pure_only() {
        assert!(Zone::Pure.can_reach(Zone::Pure));
        assert!(!Zone::Pure.can_reach(Zone::Impure));
        assert!(!Zone::Pure.can_reach(Zone::Transform));
        assert!(!Zone::Pure.can_reach(Zone::Orchestrate));
    }

    #[test]
    fn impure_reaches_impure_and_pure() {
        assert!(Zone::Impure.can_reach(Zone::Impure));
        assert!(Zone::Impure.can_reach(Zone::Pure));
        assert!(!Zone::Impure.can_reach(Zone::Transform));
        assert!(!Zone::Impure.can_reach(Zone::Orchestrate));
    }

    #[test]
    fn transform_reaches_transform_only() {
        assert!(Zone::Transform.can_reach(Zone::Transform));
        assert!(!Zone::Transform.can_reach(Zone::Pure));
        assert!(!Zone::Transform.can_reach(Zone::Impure));
        assert!(!Zone::Transform.can_reach(Zone::Orchestrate));
    }

    #[test]
    fn orchestrate_reaches_all() {
        assert!(Zone::Orchestrate.can_reach(Zone::Orchestrate));
        assert!(Zone::Orchestrate.can_reach(Zone::Pure));
        assert!(Zone::Orchestrate.can_reach(Zone::Impure));
        assert!(Zone::Orchestrate.can_reach(Zone::Transform));
    }

    #[test]
    fn structure_zone_reaches_nothing() {
        assert!(!Zone::Structure.can_reach(Zone::Pure));
        assert!(!Zone::Structure.can_reach(Zone::Impure));
        assert!(!Zone::Structure.can_reach(Zone::Transform));
        assert!(!Zone::Structure.can_reach(Zone::Structure));
    }

    // -- V2Classification combined check --

    #[test]
    fn cross_track_same_level_blocked() {
        let impure_l2 = V2Classification { level: Level::L2, zone: Zone::Impure };
        let pure_l2 = V2Classification { level: Level::L2, zone: Zone::Pure };
        assert!(!impure_l2.can_import(pure_l2), "same-level cross-track blocked");
    }

    #[test]
    fn cross_track_lower_level_allowed() {
        let impure_l2 = V2Classification { level: Level::L2, zone: Zone::Impure };
        let pure_l1 = V2Classification { level: Level::L1, zone: Zone::Pure };
        assert!(impure_l2.can_import(pure_l1));
    }

    #[test]
    fn transform_to_pure_blocked() {
        let transform_l2 = V2Classification { level: Level::L2, zone: Zone::Transform };
        let pure_l1 = V2Classification { level: Level::L1, zone: Zone::Pure };
        assert!(!transform_l2.can_import(pure_l1), "transform isolated from pure");
    }

    #[test]
    fn all_zones_can_import_structure() {
        let structure = V2Classification { level: Level::L0, zone: Zone::Structure };
        for zone in [Zone::Pure, Zone::Impure, Zone::Transform, Zone::Orchestrate] {
            let source = V2Classification { level: Level::L2, zone };
            assert!(source.can_import(structure), "{zone:?}/L2 should import structure");
        }
    }

    #[test]
    fn orchestrate_reaches_composed_in_all_zones() {
        let orch = V2Classification { level: Level::L7, zone: Zone::Orchestrate };
        for zone in [Zone::Pure, Zone::Impure, Zone::Transform] {
            let target = V2Classification { level: Level::L4, zone };
            assert!(orch.can_import(target), "orchestrate should reach {zone:?}/L4");
        }
    }

    #[test]
    fn orchestrate_dispatch_reaches_assembled_in_transform() {
        // The test case that motivated the entire refactoring
        let orch_dispatch = V2Classification { level: Level::L6, zone: Zone::Orchestrate };
        let transform_assembled = V2Classification { level: Level::L5, zone: Zone::Transform };
        assert!(orch_dispatch.can_import(transform_assembled),
            "orchestrate dispatch (L6) should reach transform/assembled (L5)");
    }

    #[test]
    fn impure_cannot_reach_transform() {
        let impure_l4 = V2Classification { level: Level::L4, zone: Zone::Impure };
        let transform_l2 = V2Classification { level: Level::L2, zone: Zone::Transform };
        assert!(!impure_l4.can_import(transform_l2), "impure cannot reach transform");
    }
}

/// One check's entry in gleipnir_messages.toml, compiled in by the build script.
/// The only values of this type are the constants in `crate::messages`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckMessages {
    pub name: &'static str,
    pub detail: &'static str,
    pub signal: &'static str,
    pub direction: &'static str,
    pub canary: &'static str,
}

impl CheckMessages {
    /// A violation of this check, carrying the check's name and its four message fields.
    pub fn violation(&self, line: usize, message: String) -> Violation {
        Violation {
            line,
            check_name: self.name.to_string(),
            severity: Severity::Error,
            message,
            detail: self.detail.to_string(),
            signal: self.signal.to_string(),
            direction: self.direction.to_string(),
            canary: self.canary.to_string(),
        }
    }
}
