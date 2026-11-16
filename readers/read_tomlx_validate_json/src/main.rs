//! CLI face of read_tomlx_validate_json, under the CLI contract
//! (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces).

use read_tomlx_validate_json::{read, ReaderError, USAGE};

fn main() {
    let code = match tool_face_io::positional_args::<3>() {
        Some([file_path, schema_name, schema_dir]) => tool_face_io::cli_report(read(&file_path, &schema_name, &schema_dir), ReaderError::error_type),
        None => tool_face_io::cli_usage(USAGE),
    };
    std::process::exit(code);
}
