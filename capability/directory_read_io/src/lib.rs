//! Directory reading — the one directory walker.
//!
//! Lists every entry under a root directory: its path relative to the root, its
//! kind, a file's size, and — when asked — a file's sha256. The walk never skips
//! what it could not read: an unreadable directory, an entry the system cannot
//! report, a name that is not UTF-8 and a file that cannot be hashed are each a
//! failure, all of them named in one error. A symlink is listed as a link and
//! never followed, so the walk cannot leave the root or loop. What to keep from
//! the listing is the caller's to decide; the walk applies no skip list of its own.
//!
//! `read_directory_json` returns the listing as JSON validated against
//! `schemas/tools/directory-listing.schema.json`; `list_directory` returns it typed,
//! for Rust callers.

use std::fs;
use std::io::Read;
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

use schemas_embedded::DIRECTORY_LISTING;

/// What to list: how deep, whether hidden entries count, whether files are hashed.
#[derive(Debug, Clone, Copy, Default)]
pub struct ListOptions {
    /// `None` lists the whole tree; `Some(1)` lists the root's own entries only.
    pub max_depth: Option<u32>,
    /// Entries whose name starts with `.`; a hidden directory is neither listed nor entered without it.
    pub include_hidden: bool,
    /// A sha256 of every file's content.
    pub with_hashes: bool,
}

/// One entry, by its path relative to the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirectoryEntry {
    pub path: String,
    pub kind: EntryKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// What an entry is. A symlink is never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
}

/// Every entry under the root, sorted by path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirectoryListing {
    pub root: String,
    pub entries: Vec<DirectoryEntry>,
}

/// Every failure a directory read can produce. `error_type()` is the `error.type`
/// a reader returns to Python; the Display string is the `error.message`.
#[derive(Debug, thiserror::Error)]
pub enum DirectoryReadError {
    #[error("Cannot list '{path}': {message}")]
    RootUnreadable { path: String, message: String },

    /// Every path the walk could not read, at once.
    #[error("Cannot list '{root}' whole — {count} path(s) could not be read:\n  {}", .failures.join("\n  "), count = .failures.len())]
    EntriesUnreadable { root: String, failures: Vec<String> },

    /// The listing did not match its own schema — an engine fault.
    #[error("Listing of '{root}' does not match directory-listing: {message}")]
    ListingInvalid { root: String, message: String },
}

impl DirectoryReadError {
    /// The failure class a caller branches on — the `error.type` of the reader result.
    pub fn error_type(&self) -> &'static str {
        match self {
            DirectoryReadError::RootUnreadable { .. } | DirectoryReadError::EntriesUnreadable { .. } => "read_error",
            DirectoryReadError::ListingInvalid { .. } => "validation_error",
        }
    }
}

/// The walk's working state: the root it lists from, what it found, and what it could not read.
struct Walk<'a> {
    root: &'a Path,
    options: ListOptions,
    entries: Vec<DirectoryEntry>,
    failures: Vec<String>,
}

/// List the tree under `root`, typed. Fails naming every path it could not read.
pub fn list_directory(root: &str, options: ListOptions) -> Result<DirectoryListing, DirectoryReadError> {
    let root_path = Path::new(root);
    let unreadable = |message: String| DirectoryReadError::RootUnreadable { path: root.to_string(), message };
    let metadata = fs::symlink_metadata(root_path).map_err(|io_err| unreadable(io_err.to_string()))?;
    if !metadata.is_dir() {
        return Err(unreadable("not a directory".to_string()));
    }
    let mut walk = Walk { root: root_path, options, entries: Vec::new(), failures: Vec::new() };
    walk_level(&mut walk, root_path, 1);
    if !walk.failures.is_empty() {
        return Err(DirectoryReadError::EntriesUnreadable { root: root.to_string(), failures: walk.failures });
    }
    walk.entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(DirectoryListing { root: root.to_string(), entries: walk.entries })
}

/// List the tree under `root` as JSON validated against `directory-listing`.
pub fn read_directory_json(root: &str, options: ListOptions) -> Result<String, DirectoryReadError> {
    let listing = list_directory(root, options)?;
    let invalid = |message: String| DirectoryReadError::ListingInvalid { root: root.to_string(), message };
    let json = serde_json::to_string_pretty(&listing).map_err(|json_err| invalid(json_err.to_string()))?;
    let result = DIRECTORY_LISTING.validate(&json).map_err(|schema_err| invalid(schema_err.to_string()))?;
    if !result.valid {
        return Err(invalid(result.message));
    }
    Ok(json)
}

/// One directory's entries at `depth` (1 = the root's own), descending while depth allows.
fn walk_level(walk: &mut Walk<'_>, directory: &Path, depth: u32) {
    let read_dir = match fs::read_dir(directory) {
        Ok(read_dir) => read_dir,
        Err(io_err) => return walk.failures.push(format!("{}: {io_err}", directory.display())),
    };
    for dir_entry in read_dir {
        match dir_entry {
            Ok(dir_entry) => visit_entry(walk, &dir_entry.path(), depth),
            Err(io_err) => walk.failures.push(format!("{}: an entry could not be read: {io_err}", directory.display())),
        }
    }
}

/// One entry: listed, hashed when asked, and entered when it is a directory within depth.
fn visit_entry(walk: &mut Walk<'_>, path: &Path, depth: u32) {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return walk.failures.push(format!("{}: the name is not UTF-8", path.display()));
    };
    if name.starts_with('.') && !walk.options.include_hidden {
        return;
    }
    let Some(relative) = path.strip_prefix(walk.root).ok().and_then(|relative| relative.to_str()) else {
        return walk.failures.push(format!("{}: the path is not UTF-8", path.display()));
    };
    let relative = relative.to_string();
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(io_err) => return walk.failures.push(format!("{}: {io_err}", path.display())),
    };
    if metadata.file_type().is_symlink() {
        walk.entries.push(DirectoryEntry { path: relative, kind: EntryKind::Symlink, size: None, sha256: None });
    } else if metadata.is_dir() {
        walk.entries.push(DirectoryEntry { path: relative, kind: EntryKind::Directory, size: None, sha256: None });
        if walk.options.max_depth.map_or(true, |max_depth| depth < max_depth) {
            walk_level(walk, path, depth + 1);
        }
    } else {
        let sha256 = file_hash(walk, path);
        walk.entries.push(DirectoryEntry { path: relative, kind: EntryKind::File, size: Some(metadata.len()), sha256 });
    }
}

/// The file's sha256 when hashes were asked for; a file that cannot be read is a failure.
fn file_hash(walk: &mut Walk<'_>, path: &Path) -> Option<String> {
    if !walk.options.with_hashes {
        return None;
    }
    match sha256_of(path) {
        Ok(digest) => Some(digest),
        Err(io_err) => {
            walk.failures.push(format!("{}: cannot be hashed: {io_err}", path.display()));
            None
        }
    }
}

fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            return Ok(format!("{:x}", hasher.finalize()));
        }
        hasher.update(&buffer[..bytes_read]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::PathBuf;

    fn temp_tree(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nornir_directory_read_io_{test_name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("people/Ada")).unwrap();
        fs::write(dir.join("people/Ada/definition.md"), "abc").unwrap();
        fs::write(dir.join("top.txt"), "").unwrap();
        fs::create_dir_all(dir.join(".hidden")).unwrap();
        fs::write(dir.join(".hidden/secret"), "x").unwrap();
        dir
    }

    fn paths(listing: &DirectoryListing) -> Vec<&str> {
        listing.entries.iter().map(|entry| entry.path.as_str()).collect()
    }

    #[test]
    fn lists_the_whole_tree_sorted_with_kinds_and_sizes() {
        let dir = temp_tree("whole");
        let listing = list_directory(dir.to_str().unwrap(), ListOptions::default()).unwrap();
        assert_eq!(paths(&listing), ["people", "people/Ada", "people/Ada/definition.md", "top.txt"]);
        assert_eq!(listing.entries[0].kind, EntryKind::Directory);
        assert_eq!(listing.entries[2].size, Some(3));
        assert_eq!(listing.entries[3].size, Some(0));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn max_depth_stops_the_descent() {
        let dir = temp_tree("depth");
        let options = ListOptions { max_depth: Some(1), ..ListOptions::default() };
        let listing = list_directory(dir.to_str().unwrap(), options).unwrap();
        assert_eq!(paths(&listing), ["people", "top.txt"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hidden_entries_are_listed_only_when_asked() {
        let dir = temp_tree("hidden");
        let options = ListOptions { include_hidden: true, ..ListOptions::default() };
        let listing = list_directory(dir.to_str().unwrap(), options).unwrap();
        assert!(paths(&listing).contains(&".hidden/secret"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_symlink_is_listed_and_never_followed() {
        let dir = temp_tree("symlink");
        symlink(&dir, dir.join("loop")).unwrap();
        let listing = list_directory(dir.to_str().unwrap(), ListOptions::default()).unwrap();
        let link = listing.entries.iter().find(|entry| entry.path == "loop").unwrap();
        assert_eq!(link.kind, EntryKind::Symlink);
        assert!(!paths(&listing).iter().any(|path| path.starts_with("loop/")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hashes_are_the_sha256_of_each_file_when_asked() {
        let dir = temp_tree("hashes");
        let options = ListOptions { with_hashes: true, ..ListOptions::default() };
        let listing = list_directory(dir.to_str().unwrap(), options).unwrap();
        let file = listing.entries.iter().find(|entry| entry.path == "top.txt").unwrap();
        assert_eq!(file.sha256.as_deref(), Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
        assert!(listing.entries.iter().find(|entry| entry.path == "people").unwrap().sha256.is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unreadable_directory_is_a_read_error_naming_it() {
        let dir = temp_tree("unreadable");
        let locked = dir.join("people/Ada");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let error = list_directory(dir.to_str().unwrap(), ListOptions::default()).unwrap_err();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(error.error_type(), "read_error");
        assert!(error.to_string().contains("people/Ada"), "{error}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_root_and_a_file_root_are_read_errors() {
        let dir = temp_tree("bad_root");
        for root in [dir.join("absent"), dir.join("top.txt")] {
            let error = list_directory(root.to_str().unwrap(), ListOptions::default()).unwrap_err();
            assert_eq!(error.error_type(), "read_error");
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_json_matches_its_schema() {
        let dir = temp_tree("json");
        let options = ListOptions { with_hashes: true, ..ListOptions::default() };
        let json = read_directory_json(dir.to_str().unwrap(), options).unwrap();
        assert!(DIRECTORY_LISTING.is_valid(&json).unwrap());
        assert!(json.contains("\"kind\": \"directory\""), "{json}");
        fs::remove_dir_all(&dir).ok();
    }
}
