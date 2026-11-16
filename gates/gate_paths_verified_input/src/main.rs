//! CLI face of gate_paths_verified_input, under the CLI contract
//! (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces).

use gate_paths_verified_input::{validate, GateError, USAGE};

fn main() {
    let code = match tool_face_io::positional_args::<1>() {
        Some([path]) => tool_face_io::cli_report(validate(&path), GateError::error_type),
        None => tool_face_io::cli_usage(USAGE),
    };
    std::process::exit(code);
}
