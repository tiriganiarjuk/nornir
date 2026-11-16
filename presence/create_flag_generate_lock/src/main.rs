//! CLI face of create_flag_generate_lock, under the CLI contract
//! (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces).

use create_flag_generate_lock::{create, PresenceError, USAGE};

fn main() {
    let code = match tool_face_io::positional_args::<1>() {
        Some([directory]) => tool_face_io::cli_report(create(&directory), PresenceError::error_type),
        None => tool_face_io::cli_usage(USAGE),
    };
    std::process::exit(code);
}
