//! CLI face of write_json_validate_directory, under the CLI contract
//! (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces). The request comes on stdin;
//! the run's report goes to stdout.

use write_json_validate_directory::{write, DirectoryWriteError, USAGE};

fn main() {
    let code = match tool_face_io::positional_args::<2>() {
        Some([directory_root, schema_dir]) => match tool_face_io::stdin_text() {
            Ok(request_json) => tool_face_io::cli_report(
                write(&directory_root, &request_json, &schema_dir),
                DirectoryWriteError::error_type,
            ),
            Err(code) => code,
        },
        None => tool_face_io::cli_usage(USAGE),
    };
    std::process::exit(code);
}
