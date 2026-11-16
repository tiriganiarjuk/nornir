//! Compiles failure_modes.toml into one function per failure mode.
//!
//! A missing or unknown field, a class that is not `*_error`, an adversarial return other
//! than "stop", or a brace that is not a `{slot}` fails the build here. An alert priority
//! that is not a `datagram_core::Priority` fails it when the generated code compiles.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

const REGISTRY: &str = "failure_modes.toml";
const REQUIRED: [&str; 4] = ["class", "standard", "route", "adversarial"];
const OPTIONAL: [&str; 1] = ["alert"];

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed={REGISTRY}");
    let source = std::fs::read_to_string(REGISTRY).map_err(|e| format!("{REGISTRY}: {e}"))?;
    let registry: toml::Table = source.parse().map_err(|e| format!("{REGISTRY}: {e}"))?;

    let mut generated = String::new();
    for (name, entry) in &registry {
        let entry = entry.as_table().ok_or_else(|| format!("{REGISTRY}: [{name}] is not a table"))?;
        generated.push_str(&entry_function(name, entry)?);
    }

    let out_dir = std::env::var("OUT_DIR").map_err(|e| format!("OUT_DIR: {e}"))?;
    std::fs::write(Path::new(&out_dir).join("failure_modes.rs"), generated)
        .map_err(|e| format!("writing the generated failure modes: {e}"))
}

/// The Rust function for one failure mode.
fn entry_function(name: &str, entry: &toml::Table) -> Result<String, String> {
    check_name(name)?;
    check_fields(name, entry)?;
    let class = text_field(name, entry, "class")?;
    let standard = text_field(name, entry, "standard")?;
    let route = text_field(name, entry, "route")?;
    check_class(name, &class)?;
    check_adversarial(name, &text_field(name, entry, "adversarial")?)?;
    let alert = alert_expression(name, entry)?;

    let mut slots = BTreeSet::new();
    let mut ordered = Vec::new();
    for text in [&standard, &route] {
        for slot in slots_in(name, text)? {
            if slots.insert(slot.clone()) {
                ordered.push(slot);
            }
        }
    }
    let parameters: Vec<String> = ordered.iter().map(|slot| format!("{slot}: impl std::fmt::Display")).collect();

    let mut function = String::new();
    let _ = writeln!(function, "/// `{class}` — {}", standard.replace('`', "'"));
    let _ = writeln!(function, "pub fn {name}({}) -> Failure {{", parameters.join(", "));
    let _ = writeln!(function, "    Failure {{");
    let _ = writeln!(function, "        mode: \"{name}\",");
    let _ = writeln!(function, "        class: \"{class}\",");
    let _ = writeln!(function, "        standard: format!(r#\"{standard}\"#),");
    let _ = writeln!(function, "        route: format!(r#\"{route}\"#),");
    let _ = writeln!(function, "        alert: {alert},");
    let _ = writeln!(function, "    }}");
    let _ = writeln!(function, "}}\n");
    Ok(function)
}

fn check_name(name: &str) -> Result<(), String> {
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if valid {
        Ok(())
    } else {
        Err(format!("{REGISTRY}: [{name}] — a failure mode's name is snake_case"))
    }
}

fn check_fields(name: &str, entry: &toml::Table) -> Result<(), String> {
    for field in REQUIRED {
        if !entry.contains_key(field) {
            return Err(format!("{REGISTRY}: [{name}] has no '{field}'"));
        }
    }
    for field in entry.keys() {
        if !REQUIRED.contains(&field.as_str()) && !OPTIONAL.contains(&field.as_str()) {
            return Err(format!("{REGISTRY}: [{name}] has an unknown field '{field}'"));
        }
    }
    Ok(())
}

fn text_field(name: &str, entry: &toml::Table, field: &str) -> Result<String, String> {
    let text = entry
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("{REGISTRY}: [{name}] '{field}' is not text"))?;
    if text.contains("\"#") {
        return Err(format!("{REGISTRY}: [{name}] '{field}' may not contain '\"#'"));
    }
    Ok(text.to_string())
}

fn check_class(name: &str, class: &str) -> Result<(), String> {
    let valid = class.ends_with("_error") && class.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    if valid {
        Ok(())
    } else {
        Err(format!("{REGISTRY}: [{name}] class '{class}' — a class is snake_case ending in _error"))
    }
}

fn check_adversarial(name: &str, adversarial: &str) -> Result<(), String> {
    if adversarial == "stop" {
        Ok(())
    } else {
        Err(format!("{REGISTRY}: [{name}] adversarial '{adversarial}' — the one adversarial return is \"stop\""))
    }
}

/// `None`, or the `datagram_core::Priority` variant the entry names.
fn alert_expression(name: &str, entry: &toml::Table) -> Result<String, String> {
    let Some(value) = entry.get("alert") else {
        return Ok("None".to_string());
    };
    let priority = value.as_str().ok_or_else(|| format!("{REGISTRY}: [{name}] 'alert' is not text"))?;
    let mut variant = priority.to_string();
    if let Some(first) = variant.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    Ok(format!("Some(datagram_core::Priority::{variant})"))
}

/// The {slot} names in a text, in order. Any other brace fails the build.
fn slots_in(name: &str, text: &str) -> Result<Vec<String>, String> {
    let mut slots = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(['{', '}']) {
        if rest[open..].starts_with('}') {
            return Err(format!("{REGISTRY}: [{name}] has a '}}' that closes no slot"));
        }
        let after = &rest[open + 1..];
        let close = after.find('}').ok_or_else(|| format!("{REGISTRY}: [{name}] has a '{{' that opens no slot"))?;
        let slot = &after[..close];
        if slot.is_empty() || !slot.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return Err(format!("{REGISTRY}: [{name}] has a slot '{{{slot}}}' — a slot is a snake_case name"));
        }
        slots.push(slot.to_string());
        rest = &after[close + 1..];
    }
    Ok(slots)
}
