//! CLI face of write_json_validate_yaml, under the CLI contract
//! (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces). The JSON to write comes on stdin.

use write_json_validate_yaml::{write, TextWriteError, USAGE};

fn main() {
    let code = match tool_face_io::positional_args::<3>() {
        Some([file_path, schema_name, schema_dir]) => match tool_face_io::stdin_text() {
            Ok(json_data) => tool_face_io::cli_report(
                write(&json_data, &file_path, &schema_name, &schema_dir),
                TextWriteError::error_type,
            ),
            Err(code) => code,
        },
        None => tool_face_io::cli_usage(USAGE),
    };
    std::process::exit(code);
}
