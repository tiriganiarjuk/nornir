//! CLI face of read_toml_validate_json, under the CLI contract
//! (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces):
//!   success              — the validated JSON on stdout, exit 0
//!   refusal or failure   — the failure class on stderr's first line, then the message, exit 1
//!   wrong arguments      — the usage on stderr, exit 2

use std::process;

struct Config {
    file_path: String,
    schema_name: String,
    schema_dir: String,
}

fn parse_args() -> Result<Config, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [file_path, schema_name, schema_dir] => Ok(Config {
            file_path: file_path.clone(),
            schema_name: schema_name.clone(),
            schema_dir: schema_dir.clone(),
        }),
        _ => Err("usage: read_toml_validate_json <file_path> <schema_name> <schema_dir>\n\
                  Pass the three arguments in that order; schema_dir is an absolute path."
            .to_string()),
    }
}

fn main() {
    let config = match parse_args() {
        Ok(c) => c,
        Err(usage) => { eprintln!("{usage}"); process::exit(2); }
    };
    match read_toml_validate_json::read(&config.file_path, &config.schema_name, &config.schema_dir) {
        Ok(data) => print!("{data}"),
        Err(e) => { eprintln!("{}\n{e}", e.error_type()); process::exit(1); }
    }
}
