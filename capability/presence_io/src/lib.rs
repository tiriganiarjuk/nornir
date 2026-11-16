//! Presence — making a thing exist or not exist, where its existence is the point and its
//! content is not.
//!
//! A **flag** is an empty file whose presence is the information. `create_flag` makes one;
//! `remove_flag` takes one away. Neither ever treats a file with content as a flag: a file
//! of that name that is not empty — or a folder or link of that name — is refused, because
//! it is somebody's work. Both are idempotent: a flag already there is created, a flag
//! already gone is removed. Neither creates or removes a folder.
//!
//! The tools over this engine fix which flag they handle; the engine takes the flag's name.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use failure_mode_core::Failure;

/// Every failure a presence operation can produce. Its class and message come from the error
/// registry (`failure_mode_core`): `error_type()` is the `error.type` a tool returns, the
/// Display string is the `error.message`.
#[derive(Debug)]
pub enum PresenceError {
    /// A path of the flag's name is there and is not an empty file.
    Occupied { directory: String, flag: String },
    FolderAbsent { directory: String, flag: String },
    Unwritable { directory: String, flag: String, reason: String },
    /// The flag's path is there and is not an empty file.
    NotEmpty { directory: String, flag: String },
    Unremovable { directory: String, flag: String, reason: String },
}

impl PresenceError {
    /// The registry's failure mode for this error, its slots filled.
    pub fn failure(&self) -> Failure {
        match self {
            PresenceError::Occupied { directory, flag } => failure_mode_core::flag_occupied(flag, directory),
            PresenceError::FolderAbsent { directory, flag } => failure_mode_core::flag_folder_absent(flag, directory),
            PresenceError::Unwritable { directory, flag, reason } => {
                failure_mode_core::flag_unwritable(flag, directory, reason)
            }
            PresenceError::NotEmpty { directory, flag } => failure_mode_core::flag_not_empty(flag, directory),
            PresenceError::Unremovable { directory, flag, reason } => {
                failure_mode_core::flag_unremovable(flag, directory, reason)
            }
        }
    }

    /// The failure class a caller branches on — the `error.type` of the tool's result.
    pub fn error_type(&self) -> &'static str {
        self.failure().class
    }
}

impl std::fmt::Display for PresenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.failure())
    }
}

impl std::error::Error for PresenceError {}

/// What is at a flag's path.
enum Found {
    Nothing,
    Flag,
    SomethingElse,
}

fn found_at(path: &Path) -> std::io::Result<Found> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() == 0 => Ok(Found::Flag),
        Ok(_) => Ok(Found::SomethingElse),
        Err(io_err) if io_err.kind() == ErrorKind::NotFound => Ok(Found::Nothing),
        Err(io_err) => Err(io_err),
    }
}

/// Create the empty file `flag` in `directory`. A flag already there is nothing to do.
pub fn create_flag(directory: &str, flag: &str) -> Result<(), PresenceError> {
    let unwritable = |reason: String| PresenceError::Unwritable { directory: directory.to_string(), flag: flag.to_string(), reason };
    if !Path::new(directory).is_dir() {
        return Err(PresenceError::FolderAbsent { directory: directory.to_string(), flag: flag.to_string() });
    }
    let path = Path::new(directory).join(flag);
    match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(_) => Ok(()),
        Err(io_err) if io_err.kind() == ErrorKind::AlreadyExists => match found_at(&path) {
            Ok(Found::Flag) => Ok(()),
            Ok(Found::Nothing) => Err(unwritable("it was removed while being created".to_string())),
            Ok(Found::SomethingElse) => {
                Err(PresenceError::Occupied { directory: directory.to_string(), flag: flag.to_string() })
            }
            Err(io_err) => Err(unwritable(io_err.to_string())),
        },
        Err(io_err) => Err(unwritable(io_err.to_string())),
    }
}

/// Remove the empty file `flag` from `directory`. A flag already gone is nothing to do.
pub fn remove_flag(directory: &str, flag: &str) -> Result<(), PresenceError> {
    let unremovable = |reason: String| PresenceError::Unremovable { directory: directory.to_string(), flag: flag.to_string(), reason };
    let path = Path::new(directory).join(flag);
    match found_at(&path) {
        Ok(Found::Nothing) => Ok(()),
        Ok(Found::SomethingElse) => Err(PresenceError::NotEmpty { directory: directory.to_string(), flag: flag.to_string() }),
        Ok(Found::Flag) => match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(io_err) if io_err.kind() == ErrorKind::NotFound => Ok(()),
            Err(io_err) => Err(unremovable(io_err.to_string())),
        },
        Err(io_err) => Err(unremovable(io_err.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const FLAG: &str = "GENERATE.lock";

    fn folder(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nornir_presence_io_{test_name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_flag_is_created_empty_and_creating_it_again_is_nothing_to_do() {
        let dir = folder("create");
        create_flag(dir.to_str().unwrap(), FLAG).unwrap();
        assert_eq!(fs::metadata(dir.join(FLAG)).unwrap().len(), 0);
        create_flag(dir.to_str().unwrap(), FLAG).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_with_content_is_never_taken_for_a_flag() {
        let dir = folder("occupied");
        fs::write(dir.join(FLAG), "somebody's work").unwrap();
        let created = create_flag(dir.to_str().unwrap(), FLAG).unwrap_err();
        assert_eq!(created.error_type(), "write_error");
        let removed = remove_flag(dir.to_str().unwrap(), FLAG).unwrap_err();
        assert_eq!(removed.error_type(), "write_error");
        assert_eq!(fs::read_to_string(dir.join(FLAG)).unwrap(), "somebody's work");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_folder_of_the_flag_s_name_is_refused_and_left_alone() {
        let dir = folder("folder_named");
        fs::create_dir_all(dir.join(FLAG)).unwrap();
        assert!(create_flag(dir.to_str().unwrap(), FLAG).is_err());
        assert!(remove_flag(dir.to_str().unwrap(), FLAG).is_err());
        assert!(dir.join(FLAG).is_dir());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_folder_is_created() {
        let dir = folder("absent_folder");
        let absent = dir.join("absent");
        let error = create_flag(absent.to_str().unwrap(), FLAG).unwrap_err();
        assert_eq!(error.error_type(), "write_error");
        assert!(!absent.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_flag_is_removed_and_removing_it_again_is_nothing_to_do() {
        let dir = folder("remove");
        fs::write(dir.join(FLAG), "").unwrap();
        remove_flag(dir.to_str().unwrap(), FLAG).unwrap();
        assert!(!dir.join(FLAG).exists());
        remove_flag(dir.to_str().unwrap(), FLAG).unwrap();
        fs::remove_dir_all(&dir).ok();
    }
}
