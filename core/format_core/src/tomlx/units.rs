//! Unit conversion tables and logic for .tomlx.
//!
//! Each family of units converts through one base unit. A unit name is read within
//! one family: `m` is minutes in the time family and meters in the length family.
//! A factor is held as a ratio, so a whole-number factor converts exactly.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::LazyLock;

use super::angle_parse::AngleNotation;
use super::types::TargetFamily;

/// Unit family (for cross-family detection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitFamily {
    Time,
    Size,
    Length,
    Mass,
    Area,
    Volume,
    Speed,
    Angle,
    Pressure,
    Frequency,
}

impl UnitFamily {
    /// The family's name in messages.
    pub fn name(&self) -> &'static str {
        match self {
            UnitFamily::Time => "time",
            UnitFamily::Size => "size",
            UnitFamily::Length => "length",
            UnitFamily::Mass => "mass",
            UnitFamily::Area => "area",
            UnitFamily::Volume => "volume",
            UnitFamily::Speed => "speed",
            UnitFamily::Angle => "angle",
            UnitFamily::Pressure => "pressure",
            UnitFamily::Frequency => "frequency",
        }
    }

    /// The first target a section holding this family's units can convert to.
    pub fn base_target(&self) -> &'static str {
        family_definition(*self)
            .and_then(|definition| definition.targets.first())
            .map(|(names, _)| names[0])
            .unwrap_or("")
    }
}

/// How many base units one of a unit is, as a ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ratio {
    pub numerator: f64,
    pub denominator: f64,
}

const fn ratio(numerator: f64, denominator: f64) -> Ratio {
    Ratio { numerator, denominator }
}

/// Unit information.
#[derive(Debug, Clone)]
pub struct UnitInfo {
    pub canonical: &'static str,
    pub family: UnitFamily,
    pub to_base: f64,
    pub ratio: Ratio,
}

/// A unit a section converts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitTarget {
    pub family: UnitFamily,
    /// The name recorded in the output's `section_units`.
    pub name: &'static str,
    pub ratio: Ratio,
}

type Names = &'static [&'static str];

/// One family: the targets a section may name, its units, and its notations.
/// Every entry lists its names, the first of which is its canonical name.
struct FamilyDefinition {
    family: UnitFamily,
    targets: &'static [(Names, Ratio)],
    units: &'static [(Names, Ratio)],
    notations: Names,
}

/// Every family, in the order they are searched and listed.
static FAMILIES: [FamilyDefinition; 10] = [
    // Base: second.
    FamilyDefinition {
        family: UnitFamily::Time,
        targets: &[
            (&["seconds", "s", "sec", "secs"], ratio(1.0, 1.0)),
            (&["milliseconds", "ms", "millis"], ratio(1.0, 1_000.0)),
        ],
        units: &[
            (&["ms", "millisecond", "milliseconds", "millis"], ratio(1.0, 1_000.0)),
            (&["s", "sec", "secs", "second", "seconds"], ratio(1.0, 1.0)),
            (&["m", "min", "mins", "minute", "minutes"], ratio(60.0, 1.0)),
            (&["h", "hr", "hrs", "hour", "hours"], ratio(3_600.0, 1.0)),
            (&["d", "day", "days"], ratio(86_400.0, 1.0)),
            (&["w", "wk", "week", "weeks"], ratio(604_800.0, 1.0)),
        ],
        notations: &[],
    },
    // Base: byte.
    FamilyDefinition {
        family: UnitFamily::Size,
        targets: &[(&["bytes", "b"], ratio(1.0, 1.0))],
        units: &[
            (&["b", "byte", "bytes"], ratio(1.0, 1.0)),
            (&["kb", "kilobyte", "kilobytes"], ratio(1_000.0, 1.0)),
            (&["mb", "megabyte", "megabytes"], ratio(1_000_000.0, 1.0)),
            (&["gb", "gigabyte", "gigabytes"], ratio(1_000_000_000.0, 1.0)),
            (&["tb", "terabyte", "terabytes"], ratio(1_000_000_000_000.0, 1.0)),
            (&["kib", "kibibyte", "kibibytes"], ratio(1_024.0, 1.0)),
            (&["mib", "mebibyte", "mebibytes"], ratio(1_048_576.0, 1.0)),
            (&["gib", "gibibyte", "gibibytes"], ratio(1_073_741_824.0, 1.0)),
            (&["tib", "tebibyte", "tebibytes"], ratio(1_099_511_627_776.0, 1.0)),
        ],
        notations: &[],
    },
    // Base: meter.
    FamilyDefinition {
        family: UnitFamily::Length,
        targets: &[(&["meters", "metres", "meter", "metre", "m"], ratio(1.0, 1.0))],
        units: &[
            (&["mm", "millimeter", "millimeters", "millimetre", "millimetres"], ratio(1.0, 1_000.0)),
            (&["cm", "centimeter", "centimeters", "centimetre", "centimetres"], ratio(1.0, 100.0)),
            (&["m", "meter", "meters", "metre", "metres"], ratio(1.0, 1.0)),
            (&["km", "kilometer", "kilometers", "kilometre", "kilometres"], ratio(1_000.0, 1.0)),
        ],
        notations: &[],
    },
    // Base: gram.
    FamilyDefinition {
        family: UnitFamily::Mass,
        targets: &[
            (&["kilograms", "kilogram", "kg"], ratio(1_000.0, 1.0)),
            (&["grams", "gram", "g"], ratio(1.0, 1.0)),
        ],
        units: &[
            (&["mg", "milligram", "milligrams"], ratio(1.0, 1_000.0)),
            (&["g", "gram", "grams"], ratio(1.0, 1.0)),
            (&["kg", "kilogram", "kilograms"], ratio(1_000.0, 1.0)),
            (&["t", "tonne", "tonnes"], ratio(1_000_000.0, 1.0)),
        ],
        notations: &[],
    },
    // Base: square meter.
    FamilyDefinition {
        family: UnitFamily::Area,
        targets: &[(&["square_meters", "square_metres", "m2", "m²"], ratio(1.0, 1.0))],
        units: &[
            (&["mm2", "mm²"], ratio(1.0, 1_000_000.0)),
            (&["cm2", "cm²"], ratio(1.0, 10_000.0)),
            (&["m2", "m²"], ratio(1.0, 1.0)),
            (&["ha", "hectare", "hectares"], ratio(10_000.0, 1.0)),
            (&["km2", "km²"], ratio(1_000_000.0, 1.0)),
        ],
        notations: &[],
    },
    // Base: liter.
    FamilyDefinition {
        family: UnitFamily::Volume,
        targets: &[
            (&["liters", "litres", "liter", "litre", "l"], ratio(1.0, 1.0)),
            (&["cubic_meters", "cubic_metres", "m3", "m³"], ratio(1_000.0, 1.0)),
        ],
        units: &[
            (&["ml", "milliliter", "milliliters", "millilitre", "millilitres", "cm3", "cm³"], ratio(1.0, 1_000.0)),
            (&["l", "liter", "liters", "litre", "litres"], ratio(1.0, 1.0)),
            (&["m3", "m³"], ratio(1_000.0, 1.0)),
        ],
        notations: &[],
    },
    // Base: meter per second.
    FamilyDefinition {
        family: UnitFamily::Speed,
        targets: &[
            (&["meters_per_second", "metres_per_second", "m/s", "mps"], ratio(1.0, 1.0)),
            (&["kilometers_per_hour", "kilometres_per_hour", "km/h", "kmh", "kph"], ratio(1_000.0, 3_600.0)),
        ],
        units: &[
            (&["m/s", "mps"], ratio(1.0, 1.0)),
            (&["km/h", "kmh", "kph"], ratio(1_000.0, 3_600.0)),
        ],
        notations: &[],
    },
    // Base: degree.
    FamilyDefinition {
        family: UnitFamily::Angle,
        targets: &[
            (&["degrees", "degree", "deg"], ratio(1.0, 1.0)),
            (&["radians", "radian", "rad"], ratio(180.0, PI)),
        ],
        units: &[
            (&["deg", "degree", "degrees", "°"], ratio(1.0, 1.0)),
            (&["rad", "radian", "radians"], ratio(180.0, PI)),
        ],
        notations: &AngleNotation::NAMES,
    },
    // Base: pascal.
    FamilyDefinition {
        family: UnitFamily::Pressure,
        targets: &[
            (&["pascals", "pascal", "pa"], ratio(1.0, 1.0)),
            (&["hectopascals", "hectopascal", "hpa"], ratio(100.0, 1.0)),
        ],
        units: &[
            (&["pa", "pascal", "pascals"], ratio(1.0, 1.0)),
            (&["hpa", "hectopascal", "hectopascals"], ratio(100.0, 1.0)),
            (&["kpa", "kilopascal", "kilopascals"], ratio(1_000.0, 1.0)),
            (&["mbar", "millibar", "millibars"], ratio(100.0, 1.0)),
            (&["bar", "bars"], ratio(100_000.0, 1.0)),
        ],
        notations: &[],
    },
    // Base: hertz.
    FamilyDefinition {
        family: UnitFamily::Frequency,
        targets: &[(&["hertz", "hz"], ratio(1.0, 1.0))],
        units: &[
            (&["hz", "hertz"], ratio(1.0, 1.0)),
            (&["khz", "kilohertz"], ratio(1_000.0, 1.0)),
            (&["mhz", "megahertz"], ratio(1_000_000.0, 1.0)),
            (&["ghz", "gigahertz"], ratio(1_000_000_000.0, 1.0)),
        ],
        notations: &[],
    },
];

fn family_definition(family: UnitFamily) -> Option<&'static FamilyDefinition> {
    FAMILIES.iter().find(|definition| definition.family == family)
}

/// Each family's units, by name.
static UNITS: LazyLock<HashMap<UnitFamily, HashMap<&'static str, UnitInfo>>> = LazyLock::new(|| {
    FAMILIES
        .iter()
        .map(|definition| {
            let units = definition
                .units
                .iter()
                .flat_map(|(names, unit_ratio)| {
                    names.iter().map(move |&name| {
                        let info = UnitInfo {
                            canonical: names[0],
                            family: definition.family,
                            to_base: unit_ratio.numerator / unit_ratio.denominator,
                            ratio: *unit_ratio,
                        };
                        (name, info)
                    })
                })
                .collect();
            (definition.family, units)
        })
        .collect()
});

/// Every target a section may name, by name.
static TARGETS: LazyLock<HashMap<&'static str, UnitTarget>> = LazyLock::new(|| {
    FAMILIES
        .iter()
        .flat_map(|definition| {
            definition.targets.iter().flat_map(move |(names, target_ratio)| {
                names.iter().map(move |&name| {
                    (name, UnitTarget { family: definition.family, name: names[0], ratio: *target_ratio })
                })
            })
        })
        .collect()
});

/// Look up a unit by name within one family.
pub fn lookup_unit(name: &str, family: UnitFamily) -> Option<&'static UnitInfo> {
    UNITS.get(&family)?.get(name.to_lowercase().as_str())
}

/// Look up the target a section names.
pub fn lookup_target(name: &str) -> Option<UnitTarget> {
    TARGETS.get(name.trim().to_lowercase().as_str()).copied()
}

/// Whether a family writes values in this notation.
fn has_notation(name: &str, family: UnitFamily) -> bool {
    let name = name.to_lowercase();
    family_definition(family).is_some_and(|definition| definition.notations.contains(&name.as_str()))
}

/// The first other family that has a unit or a notation by this name.
pub fn other_family_of_unit(name: &str, family: UnitFamily) -> Option<UnitFamily> {
    FAMILIES
        .iter()
        .map(|definition| definition.family)
        .filter(|other| *other != family)
        .find(|other| lookup_unit(name, *other).is_some() || has_notation(name, *other))
}

/// Get suggestions for an unknown unit, from one family.
pub fn suggest_units(unknown: &str, family: UnitFamily) -> Vec<String> {
    let unknown = unknown.to_lowercase();
    let notations = family_definition(family).map(|definition| definition.notations).unwrap_or(&[]);
    let mut suggestions: Vec<(usize, String)> = UNITS
        .get(&family)
        .into_iter()
        .flat_map(|units| units.keys())
        .chain(notations)
        .filter_map(|&u| {
            let distance = levenshtein(&unknown, u);
            if distance <= 2 {
                Some((distance, u.to_string()))
            } else {
                None
            }
        })
        .collect();

    suggestions.sort_by_key(|(d, _)| *d);
    suggestions.into_iter().take(3).map(|(_, s)| s).collect()
}

/// One line per family naming its units, for messages.
pub fn supported_units_summary() -> String {
    FAMILIES
        .iter()
        .map(|definition| {
            let names: Vec<&str> = definition
                .units
                .iter()
                .map(|(names, _)| names[0])
                .chain(definition.notations.iter().copied())
                .collect();
            format!("Supported {} units: {}", definition.family.name(), names.join(", "))
        })
        .collect::<Vec<_>>()
        .join("\n  ")
}

/// The targets a section may name, for messages.
pub fn target_names_summary() -> String {
    FAMILIES
        .iter()
        .flat_map(|definition| definition.targets.iter().map(|(names, _)| names[0]))
        .collect::<Vec<_>>()
        .join(", ")
}

fn levenshtein(source: &str, target: &str) -> usize {
    let source_chars: Vec<char> = source.chars().collect();
    let target_chars: Vec<char> = target.chars().collect();
    let source_len = source_chars.len();
    let target_len = target_chars.len();
    let mut dp = vec![vec![0; target_len + 1]; source_len + 1];

    for i in 0..=source_len { dp[i][0] = i; }
    for j in 0..=target_len { dp[0][j] = j; }

    for i in 1..=source_len {
        for j in 1..=target_len {
            let cost = if source_chars[i - 1] == target_chars[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }

    dp[source_len][target_len]
}

/// Get the expected family for a target.
pub fn target_family(target: &TargetFamily) -> Option<UnitFamily> {
    match target {
        TargetFamily::Unit(unit_target) => Some(unit_target.family),
        TargetFamily::Path => None,
    }
}

/// Why a unit name is not a unit of the family: it belongs to another family, or to none.
fn unit_not_in_family_message(unit: &str, family: UnitFamily) -> String {
    if let Some(other) = other_family_of_unit(unit, family) {
        return format!(
            "{} unit '{}' cannot convert to {} target",
            other.name().to_uppercase(), unit, family.name()
        );
    }
    let suggestions = suggest_units(unit, family);
    if suggestions.is_empty() {
        format!("Unknown unit '{}'", unit)
    } else {
        format!("Unknown unit '{}'. Did you mean: {}?", unit, suggestions.join(", "))
    }
}

/// A value in one unit as a value in another of the same family. The two factors
/// are joined before the one division, so whole-number factors give exact results.
fn convert_through_base(value: f64, unit: Ratio, target: Ratio) -> f64 {
    let numerator = unit.numerator * target.denominator;
    let denominator = unit.denominator * target.numerator;
    if numerator == denominator {
        value
    } else {
        value * numerator / denominator
    }
}

/// Convert a value from source unit to target. The unit is read within the target's family.
pub fn convert_unit(value: f64, source_unit: &str, target: &UnitTarget) -> Result<f64, String> {
    let unit_info = lookup_unit(source_unit, target.family)
        .ok_or_else(|| unit_not_in_family_message(source_unit, target.family))?;

    Ok(convert_through_base(value, unit_info.ratio, target.ratio))
}

/// Get the target unit name for output metadata.
pub fn target_unit_name(target: &TargetFamily) -> &'static str {
    match target {
        TargetFamily::Unit(unit_target) => unit_target.name,
        TargetFamily::Path => "path",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str) -> UnitTarget {
        lookup_target(name).unwrap()
    }

    #[test]
    fn test_lookup_time() {
        let info = lookup_unit("minutes", UnitFamily::Time).unwrap();
        assert_eq!(info.family, UnitFamily::Time);
        assert_eq!(info.to_base, 60.0);
    }

    #[test]
    fn test_lookup_size() {
        let info = lookup_unit("mb", UnitFamily::Size).unwrap();
        assert_eq!(info.family, UnitFamily::Size);
        assert_eq!(info.to_base, 1_000_000.0);
    }

    #[test]
    fn test_lookup_length() {
        let info = lookup_unit("km", UnitFamily::Length).unwrap();
        assert_eq!(info.family, UnitFamily::Length);
        assert_eq!(info.to_base, 1_000.0);
    }

    #[test]
    fn test_m_is_read_within_its_family() {
        assert_eq!(lookup_unit("m", UnitFamily::Time).unwrap().to_base, 60.0);
        assert_eq!(lookup_unit("m", UnitFamily::Length).unwrap().to_base, 1.0);
        assert!(lookup_unit("m", UnitFamily::Size).is_none());
    }

    #[test]
    fn test_both_spellings_of_length_units() {
        assert_eq!(lookup_unit("kilometers", UnitFamily::Length).unwrap().to_base, 1_000.0);
        assert_eq!(lookup_unit("kilometres", UnitFamily::Length).unwrap().to_base, 1_000.0);
        assert_eq!(lookup_unit("meter", UnitFamily::Length).unwrap().to_base, 1.0);
        assert_eq!(lookup_unit("metre", UnitFamily::Length).unwrap().to_base, 1.0);
    }

    #[test]
    fn test_every_target_name_names_one_target() {
        let declared: usize = FAMILIES
            .iter()
            .flat_map(|definition| definition.targets.iter())
            .map(|(names, _)| names.len())
            .sum();
        assert_eq!(TARGETS.len(), declared);
    }

    #[test]
    fn test_every_unit_name_names_one_unit_of_its_family() {
        for definition in &FAMILIES {
            let declared: usize = definition.units.iter().map(|(names, _)| names.len()).sum();
            assert_eq!(UNITS[&definition.family].len(), declared, "{}", definition.family.name());
        }
    }

    #[test]
    fn test_target_names() {
        assert_eq!(target("s").name, "seconds");
        assert_eq!(target("ms").name, "milliseconds");
        assert_eq!(target("b").name, "bytes");
        assert_eq!(target("metres").name, "meters");
        assert_eq!(target("kg").name, "kilograms");
        assert_eq!(target("m2").name, "square_meters");
        assert_eq!(target("litres").name, "liters");
        assert_eq!(target("m/s").name, "meters_per_second");
        assert_eq!(target("deg").name, "degrees");
        assert_eq!(target("hpa").name, "hectopascals");
        assert_eq!(target("hz").name, "hertz");
        assert!(lookup_target("furlongs").is_none());
    }

    #[test]
    fn test_convert_minutes_to_seconds() {
        assert_eq!(convert_unit(15.0, "minutes", &target("seconds")).unwrap(), 900.0);
    }

    #[test]
    fn test_convert_minutes_to_milliseconds() {
        assert_eq!(convert_unit(15.0, "minutes", &target("ms")).unwrap(), 900_000.0);
    }

    #[test]
    fn test_convert_mb_to_bytes() {
        assert_eq!(convert_unit(500.0, "mb", &target("bytes")).unwrap(), 500_000_000.0);
    }

    #[test]
    fn test_convert_length_to_meters() {
        let meters = target("meters");
        assert_eq!(convert_unit(5.0, "km", &meters).unwrap(), 5_000.0);
        assert_eq!(convert_unit(5_000.0, "m", &meters).unwrap(), 5_000.0);
        assert_eq!(convert_unit(250.0, "cm", &meters).unwrap(), 2.5);
        assert_eq!(convert_unit(2_000.0, "mm", &meters).unwrap(), 2.0);
    }

    #[test]
    fn test_convert_m_by_target_family() {
        assert_eq!(convert_unit(2.0, "m", &target("seconds")).unwrap(), 120.0);
        assert_eq!(convert_unit(2.0, "m", &target("meters")).unwrap(), 2.0);
    }

    #[test]
    fn test_convert_mass() {
        assert_eq!(convert_unit(2.0, "t", &target("kg")).unwrap(), 2_000.0);
        assert_eq!(convert_unit(750.0, "g", &target("kg")).unwrap(), 0.75);
        assert_eq!(convert_unit(1.5, "kg", &target("grams")).unwrap(), 1_500.0);
        assert_eq!(convert_unit(250.0, "mg", &target("grams")).unwrap(), 0.25);
    }

    #[test]
    fn test_convert_area() {
        let square_meters = target("m2");
        assert_eq!(convert_unit(2.5, "ha", &square_meters).unwrap(), 25_000.0);
        assert_eq!(convert_unit(3.0, "km2", &square_meters).unwrap(), 3_000_000.0);
        assert_eq!(convert_unit(3.0, "km²", &square_meters).unwrap(), 3_000_000.0);
        assert_eq!(convert_unit(5_000.0, "cm2", &square_meters).unwrap(), 0.5);
    }

    #[test]
    fn test_convert_volume() {
        assert_eq!(convert_unit(1_500.0, "ml", &target("liters")).unwrap(), 1.5);
        assert_eq!(convert_unit(2.0, "m3", &target("liters")).unwrap(), 2_000.0);
        assert_eq!(convert_unit(500.0, "l", &target("m3")).unwrap(), 0.5);
    }

    #[test]
    fn test_convert_speed() {
        assert_eq!(convert_unit(36.0, "km/h", &target("m/s")).unwrap(), 10.0);
        assert_eq!(convert_unit(10.0, "m/s", &target("km/h")).unwrap(), 36.0);
        assert_eq!(convert_unit(5.0, "kph", &target("km/h")).unwrap(), 5.0);
    }

    #[test]
    fn test_convert_angle() {
        // The factor is pi over 180, which no float holds exactly.
        assert!((convert_unit(180.0, "deg", &target("radians")).unwrap() - PI).abs() < 1e-12);
        assert!((convert_unit(PI, "rad", &target("degrees")).unwrap() - 180.0).abs() < 1e-12);
        assert_eq!(convert_unit(1.25, "rad", &target("radians")).unwrap(), 1.25);
        assert_eq!(convert_unit(270.0, "deg", &target("degrees")).unwrap(), 270.0);
    }

    #[test]
    fn test_convert_pressure() {
        assert_eq!(convert_unit(1_013.25, "hpa", &target("pascals")).unwrap(), 101_325.0);
        assert_eq!(convert_unit(1.0, "bar", &target("hpa")).unwrap(), 1_000.0);
        assert_eq!(convert_unit(1_013.25, "mbar", &target("hpa")).unwrap(), 1_013.25);
    }

    #[test]
    fn test_convert_frequency() {
        assert_eq!(convert_unit(98.5, "mhz", &target("hertz")).unwrap(), 98_500_000.0);
        assert_eq!(convert_unit(2.4, "ghz", &target("hz")).unwrap(), 2_400_000_000.0);
    }

    #[test]
    fn test_cross_family_error() {
        assert!(convert_unit(100.0, "mb", &target("seconds")).is_err());
    }

    #[test]
    fn test_length_unit_under_time_target_names_its_family() {
        let message = convert_unit(5.0, "km", &target("seconds")).unwrap_err();
        assert!(message.contains("LENGTH"));
        assert!(message.contains("time"));
    }

    #[test]
    fn test_notation_belongs_to_the_angle_family() {
        assert_eq!(other_family_of_unit("dms", UnitFamily::Time), Some(UnitFamily::Angle));
        assert_eq!(other_family_of_unit("dms", UnitFamily::Angle), None);
    }

    #[test]
    fn test_suggest_units() {
        let suggestions = suggest_units("minuts", UnitFamily::Time);
        assert!(!suggestions.is_empty());
    }

    #[test]
    fn test_suggestions_come_from_the_family_asked() {
        let suggestions = suggest_units("kilometrs", UnitFamily::Length);
        assert!(suggestions.iter().any(|unit| unit.starts_with("kilomet")));
        assert!(suggest_units("kilometrs", UnitFamily::Time).is_empty());
    }

    #[test]
    fn test_summaries_are_drawn_from_the_table() {
        let units = supported_units_summary();
        assert!(units.contains("Supported time units: ms, s, m, h, d, w"));
        assert!(units.contains("Supported length units: mm, cm, m, km"));
        assert!(units.contains("Supported angle units: deg, rad, dms, dm"));
        assert!(target_names_summary().starts_with("seconds, milliseconds, bytes, meters"));
    }
}
