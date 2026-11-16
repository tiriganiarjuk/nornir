//! CLI face of remove_flag_generate_lock, under the CLI contract
//! (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces).

use remove_flag_generate_lock::{remove, PresenceError, USAGE};

fn main() {
    let code = match tool_face_io::positional_args::<1>() {
        Some([directory]) => tool_face_io::cli_report(remove(&directory), PresenceError::error_type),
        None => tool_face_io::cli_usage(USAGE),
    };
    std::process::exit(code);
}
