//! CLI face of read_directory_links_followed_json, under the CLI contract
//! (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces).

use read_directory_links_followed_json::{read, DirectoryReadError, USAGE};

fn main() {
    let code = match tool_face_io::positional_args::<1>() {
        Some([root]) => tool_face_io::cli_report(read(&root), DirectoryReadError::error_type),
        None => tool_face_io::cli_usage(USAGE),
    };
    std::process::exit(code);
}
