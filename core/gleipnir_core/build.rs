//! Compiles gleipnir_messages.toml into one constant per check.
//!
//! An entry with a missing, empty or unknown field, or a check name that is not snake_case,
//! fails the build here. So does a check named in gleipnir_matrix.toml that has no entry.
//! A check table that names a check with no entry fails when the generated code compiles,
//! because the constant it refers to does not exist.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

const MESSAGES: &str = "gleipnir_messages.toml";
const MATRIX: &str = "gleipnir_matrix.toml";
const INVENTORY: &str = "../../deploy_categories.toml";
const FIELDS: [&str; 4] = ["detail", "signal", "direction", "canary"];
const SEVERITIES: [&str; 3] = ["blocked", "error", "warning"];

/// Where a message text names the readers nornir deploys. Filled from the inventory,
/// so a new reader is named without anyone editing the messages.
const READERS_SLOT: &str = "{readers}";

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed={MESSAGES}");
    println!("cargo:rerun-if-changed={MATRIX}");
    println!("cargo:rerun-if-changed={INVENTORY}");

    let messages = read_table(MESSAGES)?;
    let readers = deployed_readers()?;
    let mut generated = String::new();
    for (name, entry) in &messages {
        let entry = entry.as_table().ok_or_else(|| format!("{MESSAGES}: [{name}] is not a table"))?;
        generated.push_str(&entry_constant(name, entry, &readers)?);
    }

    let matrix_names = matrix_check_names(&read_table(MATRIX)?)?;
    for name in &matrix_names {
        if !messages.contains_key(name) {
            return Err(format!("{MATRIX}: names the check '{name}', which has no entry in {MESSAGES}"));
        }
    }
    let quoted: Vec<String> = matrix_names.iter().map(|name| format!("{name:?}")).collect();
    let _ = writeln!(generated, "/// Every check gleipnir_matrix.toml names.");
    let _ = writeln!(generated, "pub const MATRIX_CHECK_NAMES: &[&str] = &[{}];", quoted.join(", "));

    let out_dir = std::env::var("OUT_DIR").map_err(|e| format!("OUT_DIR: {e}"))?;
    std::fs::write(Path::new(&out_dir).join("gleipnir_messages.rs"), generated)
        .map_err(|e| format!("writing the generated check messages: {e}"))
}

fn read_table(file: &str) -> Result<toml::Table, String> {
    let source = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    source.parse().map_err(|e| format!("{file}: {e}"))
}

/// The readers the inventory deploys, as the list a message names them by.
fn deployed_readers() -> Result<String, String> {
    let inventory = read_table(INVENTORY)?;
    let listed = inventory
        .get("readers")
        .and_then(|readers| readers.get("pyo3_crates"))
        .and_then(|crates| crates.as_array())
        .ok_or_else(|| format!("{INVENTORY}: [readers] has no pyo3_crates list"))?;
    let mut names = Vec::new();
    for reader in listed {
        names.push(reader.as_str().ok_or_else(|| format!("{INVENTORY}: [readers] pyo3_crates holds a name that is not text"))?);
    }
    if names.is_empty() {
        return Err(format!("{INVENTORY}: [readers] pyo3_crates is empty"));
    }
    Ok(names.join(", "))
}

/// The Rust constant for one check's messages entry.
fn entry_constant(name: &str, entry: &toml::Table, readers: &str) -> Result<String, String> {
    check_name(name)?;
    for field in entry.keys() {
        if !FIELDS.contains(&field.as_str()) {
            return Err(format!("{MESSAGES}: [{name}] has an unknown field '{field}'"));
        }
    }
    let mut constant = String::new();
    let _ = writeln!(constant, "/// The messages entry of the check `{name}`.");
    let _ = writeln!(constant, "pub const {}: CheckMessages = CheckMessages {{", name.to_ascii_uppercase());
    let _ = writeln!(constant, "    name: {name:?},");
    for field in FIELDS {
        let text = text_field(name, entry, field)?.replace(READERS_SLOT, readers);
        let _ = writeln!(constant, "    {field}: {text:?},");
    }
    let _ = writeln!(constant, "}};\n");
    Ok(constant)
}

fn check_name(name: &str) -> Result<(), String> {
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if valid {
        Ok(())
    } else {
        Err(format!("{MESSAGES}: [{name}] — a check's name is snake_case"))
    }
}

fn text_field<'entry>(name: &str, entry: &'entry toml::Table, field: &str) -> Result<&'entry str, String> {
    let value = entry.get(field).ok_or_else(|| format!("{MESSAGES}: [{name}] has no '{field}'"))?;
    let text = value.as_str().ok_or_else(|| format!("{MESSAGES}: [{name}] '{field}' is not text"))?;
    if text.trim().is_empty() {
        return Err(format!("{MESSAGES}: [{name}] '{field}' is empty"));
    }
    Ok(text)
}

/// Every check name the matrix lists, under any version, classification and severity.
fn matrix_check_names(matrix: &toml::Table) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    for (version, classifications) in matrix {
        let classifications =
            classifications.as_table().ok_or_else(|| format!("{MATRIX}: [{version}] is not a table"))?;
        for (classification, section) in classifications {
            let key = format!("{version}.{classification}");
            let section = section.as_table().ok_or_else(|| format!("{MATRIX}: [{key}] is not a table"))?;
            for severity in SEVERITIES {
                let Some(listed) = section.get(severity) else { continue };
                let listed = listed.as_array().ok_or_else(|| format!("{MATRIX}: {key}.{severity} is not a list"))?;
                for name in listed {
                    let name = name
                        .as_str()
                        .ok_or_else(|| format!("{MATRIX}: {key}.{severity} holds a name that is not text"))?;
                    names.insert(name.to_string());
                }
            }
        }
    }
    Ok(names)
}
