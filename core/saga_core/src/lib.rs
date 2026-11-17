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
    /// The fingerprint of what examined the source: the check logic and the
    /// configuration of the saga that wrote the report.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub examined_by: String,
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

// =============================================================================
// Source kinds — which language a file is, by its name
// =============================================================================

/// The languages saga examines. A file is of a kind by the extension its name
/// ends in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Python,
    Rust,
    Svelte,
}

impl SourceKind {
    /// Every kind.
    pub const ALL: [SourceKind; 3] = [SourceKind::Python, SourceKind::Rust, SourceKind::Svelte];

    /// The extension a file of this kind ends in, without its dot.
    pub fn extension(self) -> &'static str {
        match self {
            SourceKind::Python => "py",
            SourceKind::Rust => "rs",
            SourceKind::Svelte => "svelte",
        }
    }

    /// The kind of a file, by the extension after the last dot of its name.
    /// `None` for a name without an extension, a name that is only an
    /// extension (`.py`), and every other extension.
    pub fn of_file_name(file_name: &str) -> Option<SourceKind> {
        let (stem, extension) = file_name.rsplit_once('.')?;
        if stem.is_empty() {
            return None;
        }
        SourceKind::ALL.into_iter().find(|kind| kind.extension() == extension)
    }

    /// The kind of the source a sidecar belongs to, by the sidecar's name
    /// (`.{source file name}.qa`). `None` for a name that is not a sidecar's.
    pub fn of_sidecar_name(sidecar_name: &str) -> Option<SourceKind> {
        let source = source_path_from_qa(Path::new(sidecar_name))?;
        SourceKind::of_file_name(source.file_name()?.to_str()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_of_the_kind_its_extension_names() {
        assert_eq!(SourceKind::of_file_name("lib.rs"), Some(SourceKind::Rust));
        assert_eq!(SourceKind::of_file_name("test.spec.py"), Some(SourceKind::Python));
        assert_eq!(SourceKind::of_file_name("App.svelte"), Some(SourceKind::Svelte));
    }

    #[test]
    fn a_name_without_a_known_extension_is_of_no_kind() {
        for file_name in ["Makefile", "notes.txt", "test.pyw", "test.js", ".py", "py", ""] {
            assert_eq!(SourceKind::of_file_name(file_name), None, "{file_name}");
        }
    }

    #[test]
    fn every_kind_is_found_by_its_own_extension() {
        for kind in SourceKind::ALL {
            assert_eq!(SourceKind::of_file_name(&format!("name.{}", kind.extension())), Some(kind));
        }
    }

    #[test]
    fn a_sidecar_is_of_the_kind_of_its_source() {
        assert_eq!(SourceKind::of_sidecar_name(".lib.rs.qa"), Some(SourceKind::Rust));
        assert_eq!(SourceKind::of_sidecar_name(".a.py.qa"), Some(SourceKind::Python));
        let sidecar = qa_path(Path::new("/dir/App.svelte"));
        let sidecar_name = sidecar.file_name().unwrap().to_str().unwrap();
        assert_eq!(SourceKind::of_sidecar_name(sidecar_name), Some(SourceKind::Svelte));
    }

    #[test]
    fn a_name_that_is_not_a_sidecar_s_is_of_no_kind() {
        for sidecar_name in ["lib.rs", "lib.rs.qa", ".notes.txt.qa", ".Makefile.qa", ".qa", "..py.qa"] {
            assert_eq!(SourceKind::of_sidecar_name(sidecar_name), None, "{sidecar_name}");
        }
    }

    #[test]
    fn a_report_carries_what_examined_it_through_its_json() {
        let report = SanityReport { file: "/p/a.py".into(), examined_by: "0123abcd".into(), ..Default::default() };
        let json = serde_json::to_string(&report).unwrap();
        let read_back: SanityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(read_back.examined_by, "0123abcd");
    }

    #[test]
    fn a_report_that_records_no_examiner_has_no_such_key_and_reads_back_empty() {
        let report = SanityReport { file: "/p/a.py".into(), ..Default::default() };
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("examined_by"), "{json}");
        let read_back: SanityReport = serde_json::from_str(&json).unwrap();
        assert!(read_back.examined_by.is_empty());
    }

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
