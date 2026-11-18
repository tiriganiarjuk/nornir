//! dead_code_io — the dead code of every Python project under a directory.
//!
//! A project is a directory holding a `pyproject.toml`. Its files are the Python
//! files under it, walked as saga walks a tree, less those of a project nested
//! inside it; its entry points are `[project.scripts]`. A directory with no
//! project under it belongs to the nearest project above it.
//!
//! What is dead is decided by `dep_resolve_core::liveness`; this crate reads the
//! files it needs and prints what it found. `dead_code_find` and `syn` on a
//! directory are its two callers.

use std::path::{Path, PathBuf};

use dep_resolve_core::liveness::{dead_symbols, EntryPoint, Liveness, ProjectFile};
use dep_resolve_core::SymbolKind;
use failure_mode_core::Failure;

/// The marker of a project's root.
const PROJECT_MARKER: &str = "pyproject.toml";

/// One project's dead code.
pub struct ProjectDeadCode {
    pub root: PathBuf,
    pub liveness: Liveness,
}

/// The dead code of every project under a directory, by project root.
pub fn find_dead_code(directory: &Path) -> Result<Vec<ProjectDeadCode>, Failure> {
    let directory = directory.canonicalize().map_err(|io_err| failure_mode_core::file_unreadable(directory.display(), io_err))?;
    let roots = project_roots(&directory)?;
    let mut found = Vec::new();
    for root in &roots {
        found.push(ProjectDeadCode { root: root.to_path_buf(), liveness: project_liveness(root, &roots)? });
    }
    Ok(found)
}

/// The projects under a directory; with none under it, the nearest above it.
fn project_roots(directory: &Path) -> Result<Vec<PathBuf>, Failure> {
    let markers = saga_runner::find_files(directory, &[], &|name| name == PROJECT_MARKER)?;
    let roots: Vec<PathBuf> = markers.iter().filter_map(|marker| marker.parent().map(Path::to_path_buf)).collect();
    if !roots.is_empty() {
        return Ok(roots);
    }
    Ok(directory.ancestors().find(|ancestor| ancestor.join(PROJECT_MARKER).is_file()).map(Path::to_path_buf).into_iter().collect())
}

/// One project's liveness: its own Python files, read, against its entry points.
fn project_liveness(root: &Path, all_roots: &[PathBuf]) -> Result<Liveness, Failure> {
    let nested: Vec<&PathBuf> = all_roots.iter().filter(|other| *other != root && other.starts_with(root)).collect();
    let files = saga_runner::find_files(root, &[], &|name| name.ends_with(".py"))?;
    let own = files.into_iter().filter(|file| !nested.iter().any(|inner| file.starts_with(inner)));
    let mut sources = Vec::new();
    let mut unreadable = Vec::new();
    for file in own {
        let relative = file.strip_prefix(root).unwrap_or(&file).to_string_lossy().to_string();
        match std::fs::read(&file) {
            Ok(bytes) => sources.push((relative, bytes)),
            Err(_) => unreadable.push(relative),
        }
    }
    let project: Vec<ProjectFile> = sources.iter().map(|(path, bytes)| ProjectFile { relative_path: path, source: bytes }).collect();
    let mut liveness = dead_symbols(&project, &entry_points(root)?);
    liveness.unparsed.extend(unreadable);
    Ok(liveness)
}

/// The entry points `pyproject.toml` declares under `[project.scripts]`.
fn entry_points(root: &Path) -> Result<Vec<EntryPoint>, Failure> {
    let marker = root.join(PROJECT_MARKER);
    let text = std::fs::read_to_string(&marker).map_err(|io_err| failure_mode_core::file_unreadable(marker.display(), io_err))?;
    let document: toml::Table = text.parse().map_err(|toml_err| failure_mode_core::file_unreadable(marker.display(), toml_err))?;
    let scripts = document.get("project").and_then(|project| project.get("scripts")).and_then(|scripts| scripts.as_table());
    let targets = scripts.into_iter().flat_map(|table| table.values()).filter_map(|target| target.as_str());
    Ok(targets.filter_map(EntryPoint::parse).collect())
}

/// What was found, as lines to read and to paste: one per dead definition, its path
/// relative to `base`. Empty when nothing is dead and every file was examined.
pub fn render_dead_code(projects: &[ProjectDeadCode], base: &Path) -> String {
    let base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let mut text = String::new();
    for project in projects.iter().filter(|project| !is_clean(&project.liveness)) {
        let shown_root = project.root.strip_prefix(&base).unwrap_or(&project.root);
        let prefix = if shown_root.as_os_str().is_empty() { String::new() } else { format!("{}/", shown_root.display()) };
        text.push_str(&project_heading(project, shown_root));
        for dead in &project.liveness.dead {
            text.push_str(&format!("  {prefix}{}:{}  {}  ({})\n", dead.file, dead.line, dead.name, kind_label(dead.kind)));
        }
        for unparsed in &project.liveness.unparsed {
            text.push_str(&format!("  not examined: {prefix}{unparsed}\n"));
        }
    }
    text
}

fn is_clean(liveness: &Liveness) -> bool {
    liveness.dead.is_empty() && liveness.unparsed.is_empty()
}

fn project_heading(project: &ProjectDeadCode, shown_root: &Path) -> String {
    let count = project.liveness.dead.len();
    let noun = if count == 1 { "definition" } else { "definitions" };
    let name = if shown_root.as_os_str().is_empty() { project.root.display().to_string() } else { shown_root.display().to_string() };
    format!("[dead code] {count} {noun} nothing reaches in {name}\n")
}

fn kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "function",
        SymbolKind::Class => "class",
        SymbolKind::Constant => "constant",
        SymbolKind::TypeAlias => "type alias",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree of files under a fresh temporary directory.
    fn tree(test_name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("dead_code_io_{test_name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, content) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, content).unwrap();
        }
        root.canonicalize().unwrap()
    }

    const PYPROJECT: &str = "[project]\nname = \"app\"\n\n[project.scripts]\napp = \"app.cli:main\"\n";
    const CLI: &str = "from app.run import run\n\ndef main() -> int:\n    return run()\n";
    const RUN: &str = "def run() -> int:\n    return 1\n\ndef forgotten() -> int:\n    return 2\n";

    fn dead_of(projects: &[ProjectDeadCode]) -> Vec<String> {
        projects.iter().flat_map(|project| project.liveness.dead.iter().map(|dead| dead.name.to_string())).collect()
    }

    #[test]
    fn a_project_s_dead_code_is_found_from_its_declared_entry_point() {
        let root = tree("one", &[("pyproject.toml", PYPROJECT), ("src/app/cli.py", CLI), ("src/app/run.py", RUN)]);
        let found = find_dead_code(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(dead_of(&found), vec!["forgotten"]);
    }

    #[test]
    fn a_nested_project_is_examined_on_its_own() {
        let root = tree(
            "nested",
            &[
                ("pyproject.toml", PYPROJECT),
                ("src/app/cli.py", CLI),
                ("src/app/run.py", "def run() -> int:\n    return 1\n"),
                ("tools/inner/pyproject.toml", "[project]\nname = \"inner\"\n"),
                ("tools/inner/src/inner/lonely.py", "def lonely() -> int:\n    return 1\n"),
            ],
        );
        let found = find_dead_code(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        let roots: Vec<String> = found.iter().map(|project| project.root.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(found.len(), 2, "{roots:?}");
        assert_eq!(dead_of(&found), vec!["lonely"], "the inner project's file is its own, not the outer one's");
    }

    #[test]
    fn a_directory_inside_a_project_examines_that_project() {
        let root = tree("inside", &[("pyproject.toml", PYPROJECT), ("src/app/cli.py", CLI), ("src/app/run.py", RUN)]);
        let found = find_dead_code(&root.join("src/app")).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(dead_of(&found), vec!["forgotten"]);
    }

    #[test]
    fn the_report_names_each_dead_definition_by_path_line_and_kind() {
        let root = tree("render", &[("pyproject.toml", PYPROJECT), ("src/app/cli.py", CLI), ("src/app/run.py", RUN)]);
        let found = find_dead_code(&root).unwrap();
        let text = render_dead_code(&found, &root);
        let _ = std::fs::remove_dir_all(&root);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("[dead code] 1 definition nothing reaches in "), "{text}");
        assert_eq!(lines[1], "  src/app/run.py:4  forgotten  (function)");
    }

    #[test]
    fn nothing_dead_is_nothing_to_report() {
        let root = tree("clean", &[("pyproject.toml", PYPROJECT), ("src/app/cli.py", CLI), ("src/app/run.py", "def run() -> int:\n    return 1\n")]);
        let found = find_dead_code(&root).unwrap();
        let text = render_dead_code(&found, &root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(text, "");
    }

    #[test]
    fn a_directory_with_no_project_has_none_to_examine() {
        let root = tree("no_project", &[("loose/script.py", "def run() -> int:\n    return 1\n")]);
        let found = find_dead_code(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!(found.is_empty());
    }
}
