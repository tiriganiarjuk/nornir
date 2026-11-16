//! Saga core types: Issue, SanityReport, and pure path functions.
//!
//! Pure library — no I/O, no side effects, no subprocess execution.
//! All I/O operations live in `capability/saga_runner`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// =============================================================================
// Data structures — the .qa sidecar format
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub tool: String,
    pub code: String,
    pub severity: String,
    pub line: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub fixable: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signal: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub direction: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub canary: String,
}

/// An instrument that could not do its work on a file. The file is then not
/// known to be clean, whatever the issues say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    /// The instrument that did not do its work.
    pub tool: String,
    /// The failure mode, by its name in the error registry.
    pub mode: String,
    /// The failure class of that mode.
    pub class: String,
    /// The mode's standard message with its route.
    pub message: String,
}

/// The shape of this type on disk is `schemas/tools/sanity-report.schema.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SanityReport {
    pub file: String,
    #[serde(default)]
    pub relative_path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_hash: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub elapsed_ms: u64,
    #[serde(default)]
    pub issues: Vec<Issue>,
    #[serde(default)]
    pub faults: Vec<Fault>,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub by_tool: HashMap<String, usize>,
    #[serde(default)]
    pub by_severity: HashMap<String, usize>,
    #[serde(default)]
    pub by_category: HashMap<String, usize>,
}

// =============================================================================
// Pure path functions
// =============================================================================

/// Get the .qa sidecar path for a source file.
pub fn qa_path(file_path: &Path) -> PathBuf {
    let name = file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    file_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!(".{}.qa", name))
}

/// Reverse of qa_path: given `.foo.py.qa`, returns `foo.py` path in the same directory.
/// Returns None if the filename doesn't match the sidecar pattern (.*.qa).
pub fn source_path_from_qa(qa_path: &Path) -> Option<PathBuf> {
    let name = qa_path.file_name()?.to_string_lossy();
    if name.len() < 5 || !name.starts_with('.') || !name.ends_with(".qa") {
        return None;
    }
    let source_name = &name[1..name.len() - 3];
    if source_name.is_empty() {
        return None;
    }
    Some(
        qa_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(source_name),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_path_roundtrip() {
        let source = Path::new("/some/dir/foo.py");
        let qa = qa_path(source);
        let recovered = source_path_from_qa(&qa).unwrap();
        assert_eq!(recovered, source);
    }

    #[test]
    fn source_path_rejects_non_sidecar() {
        assert!(source_path_from_qa(Path::new("foo.py")).is_none());
        assert!(source_path_from_qa(Path::new(".qa")).is_none());
        assert!(source_path_from_qa(Path::new("regular.txt")).is_none());
    }

    #[test]
    fn source_path_from_hidden_qa() {
        let qa = Path::new("/dir/.foo.py.qa");
        let source = source_path_from_qa(qa).unwrap();
        assert_eq!(source, Path::new("/dir/foo.py"));
    }

    #[test]
    fn qa_path_file_in_root_directory() {
        let source = Path::new("/foo.py");
        let qa = qa_path(source);
        assert_eq!(qa, PathBuf::from("/.foo.py.qa"));
    }

    #[test]
    fn qa_path_multiple_extensions() {
        let source = Path::new("/dir/test.spec.py");
        let qa = qa_path(source);
        assert_eq!(qa, PathBuf::from("/dir/.test.spec.py.qa"));
        let recovered = source_path_from_qa(&qa).unwrap();
        assert_eq!(recovered, source);
    }

    #[test]
    fn qa_path_no_extension() {
        let source = Path::new("/dir/Makefile");
        let qa = qa_path(source);
        assert_eq!(qa, PathBuf::from("/dir/.Makefile.qa"));
        let recovered = source_path_from_qa(&qa).unwrap();
        assert_eq!(recovered, source);
    }
}
