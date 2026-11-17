//! Compiles gleipnir_messages.toml into one constant per check.
//!
//! An entry with a missing, empty or unknown field, or a check name that is not snake_case,
//! fails the build here. So does a check named in gleipnir_matrix.toml that has no entry.
//! A check table that names a check with no entry fails when the generated code compiles,
//! because the constant it refers to does not exist.
//!
//! Also compiles `LOGIC_FINGERPRINT`: one hash over everything that decides what gleipnir
//! reports — the files under src/, the three data files, the inventory that fills the
//! readers slot, and the versions of the tree-sitter crates the workspace lock file pins.
//! The build only reads them.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const MESSAGES: &str = "gleipnir_messages.toml";
const MATRIX: &str = "gleipnir_matrix.toml";
const STATISTICS: &str = "gleipnir_statistics.toml";
const SOURCES: &str = "src";
const INVENTORY: &str = "../../deploy_categories.toml";
/// The workspace lock file, relative to this crate's manifest directory.
const LOCK_FILE: &str = "../../Cargo.lock";
/// The prefix of the name of every crate that parses a source for gleipnir.
const GRAMMAR_PREFIX: &str = "tree-sitter";
const FIELDS: [&str; 4] = ["detail", "signal", "direction", "canary"];
const SEVERITIES: [&str; 3] = ["blocked", "error", "warning"];

/// Where a message text names the readers nornir deploys. Filled from the inventory,
/// so a new reader is named without anyone editing the messages.
const READERS_SLOT: &str = "{readers}";

fn main() -> Result<(), String> {
    // One line per logic file, and none for the src directory: cargo watches a named
    // directory whole, and saga writes hidden sidecars under src/. A new source file is
    // declared with `mod` in a file that is watched, so it reruns this script too.
    let files = logic_files()?;
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    let lock_file = lock_file_path()?;
    println!("cargo:rerun-if-changed={}", lock_file.display());
    let grammars = grammar_versions(&read_table(&lock_file.to_string_lossy())?)?;

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
        .map_err(|e| format!("writing the generated check messages: {e}"))?;
    let fingerprint = logic_fingerprint(&files, &grammars)?;
    std::fs::write(Path::new(&out_dir).join("gleipnir_logic_fingerprint.rs"), fingerprint_constant(&fingerprint))
        .map_err(|write_error| format!("writing the generated logic fingerprint: {write_error}"))
}

/// The Rust constant holding the logic fingerprint.
fn fingerprint_constant(fingerprint: &str) -> String {
    let mut constant = String::new();
    let _ = writeln!(constant, "/// The fingerprint of gleipnir's check logic: one hash over every file under src/,");
    let _ = writeln!(constant, "/// over {MATRIX}, {MESSAGES}, {STATISTICS} and deploy_categories.toml,");
    let _ = writeln!(constant, "/// and over the versions of the tree-sitter crates in the workspace Cargo.lock,");
    let _ = writeln!(constant, "/// compiled by the build script. Two builds of the same files carry the same value.");
    let _ = writeln!(constant, "pub const LOGIC_FINGERPRINT: &str = {fingerprint:?};");
    constant
}

/// One hash over every logic file, in sorted path order — each file's path, its length
/// and its content — and then over every grammar's name and version, each preceded by
/// its length. FNV-1a, 64 bits, as 16 hex digits.
fn logic_fingerprint(files: &[PathBuf], grammars: &[(String, String)]) -> Result<String, String> {
    let mut hash = FNV_OFFSET_BASIS;
    for file in files {
        let content = std::fs::read(file).map_err(|read_error| format!("{}: {read_error}", file.display()))?;
        hash = fnv1a(hash, file.to_string_lossy().as_bytes());
        hash = fnv1a(hash, &(content.len() as u64).to_le_bytes());
        hash = fnv1a(hash, &content);
    }
    for (name, version) in grammars {
        for text in [name, version] {
            hash = fnv1a(hash, &(text.len() as u64).to_le_bytes());
            hash = fnv1a(hash, text.as_bytes());
        }
    }
    Ok(format!("{hash:016x}"))
}

/// The workspace lock file, found from this crate's manifest directory.
fn lock_file_path() -> Result<PathBuf, String> {
    let manifest_directory =
        std::env::var("CARGO_MANIFEST_DIR").map_err(|variable_error| format!("CARGO_MANIFEST_DIR: {variable_error}"))?;
    Ok(Path::new(&manifest_directory).join(LOCK_FILE))
}

/// The name and version of every package in the lock file whose name starts with
/// `tree-sitter`, sorted. The parsers decide what gleipnir sees in a source, so another
/// version of one is another examiner.
fn grammar_versions(lock: &toml::Table) -> Result<Vec<(String, String)>, String> {
    let packages = lock
        .get("package")
        .and_then(|listed| listed.as_array())
        .ok_or_else(|| format!("{LOCK_FILE}: holds no package list"))?;
    let mut grammars = Vec::new();
    for package in packages {
        let name = package_text(package, "name")?;
        if name.starts_with(GRAMMAR_PREFIX) {
            grammars.push((name.to_string(), package_text(package, "version")?.to_string()));
        }
    }
    if grammars.is_empty() {
        return Err(format!("{LOCK_FILE}: names no {GRAMMAR_PREFIX} package"));
    }
    grammars.sort();
    Ok(grammars)
}

/// One text field of a lock file package.
fn package_text<'package>(package: &'package toml::Value, field: &str) -> Result<&'package str, String> {
    package
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("{LOCK_FILE}: a package has no '{field}' text"))
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a over the bytes, continuing from the hash so far.
fn fnv1a(hash_so_far: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash_so_far, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME))
}

/// Every file that decides what gleipnir reports, sorted: the three data files, the
/// inventory that fills the readers slot, and the files under src/.
fn logic_files() -> Result<Vec<PathBuf>, String> {
    let mut files =
        vec![PathBuf::from(MATRIX), PathBuf::from(MESSAGES), PathBuf::from(STATISTICS), PathBuf::from(INVENTORY)];
    collect_source_files(Path::new(SOURCES), &mut files)?;
    files.sort();
    Ok(files)
}

/// The files under a directory, at any depth. A hidden entry is left out: the hidden
/// files under src/ are the sidecars saga writes beside a source, and they decide nothing.
fn collect_source_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory).map_err(|read_error| format!("{}: {read_error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|read_error| format!("{}: {read_error}", directory.display()))?.path();
        if is_hidden(&path) {
            continue;
        }
        if path.is_dir() {
            collect_source_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// Whether a path's last component starts with a dot.
fn is_hidden(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.'))
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
