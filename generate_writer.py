#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "loguru>=0.7",
# ]
# ///
"""Generate a Nornir writer or store crate from parameters.

A writer appends or writes one validated record (or batch) through
write_engine. A store closes an open append file as one stored session
through store_engine. Both are thin binaries over their engine; this
script writes the crate so the config is the whole description.

Paths are given relative to the ai root (~/ai) and resolved at runtime
through write_engine::ai_home(), exactly as the hand-written writers do.

Usage — a writer:
    ./generate_writer.py \\
        --name append_interview_summaries_record \\
        --schema-const SUMMARIES \\
        --schema-path spaces/bragi/schemas/summaries.schema.json \\
        --format jsonl \\
        --frequency record \\
        --output-kind directory_prefix \\
        --dir-path spaces/bragi/interview/interviews \\
        --suffix .summaries.jsonl

Usage — a stamping writer (fields the writer sets, the author may not):
    ./generate_writer.py \\
        --name append_forseti_interview_exchange_record \\
        --schema-const INTERVIEW_EXCHANGE \\
        --schema-path schemas/tools/interview-exchange.schema.json \\
        --format jsonl --frequency record \\
        --output-kind fixed_file --file-path spaces/forseti/interview/OPEN.jsonl \\
        --stamp-sequence exchange --stamp-timestamp ts --stamp-session session

Usage — a store:
    ./generate_writer.py --kind store \\
        --name store_forseti_interview_session \\
        --schema-const INTERVIEW_SESSION \\
        --schema-path schemas/tools/interview-session.schema.json \\
        --open-file spaces/forseti/interview/OPEN.jsonl \\
        --sessions-dir spaces/forseti/interview/sessions \\
        --index-file spaces/forseti/interview/index.jsonl \\
        --session-field session --timestamp-field ts

Generates:
    writers/{name}/Cargo.toml
    writers/{name}/src/main.rs

Prints (does NOT auto-modify):
    - workspace Cargo.toml member to add
    - deploy_categories.toml [writers].crates entry to add
    - tool_registry.toml entry to add (writers only)
"""

import argparse
import sys
from pathlib import Path

from loguru import logger

NORNIR_DIR = Path(__file__).resolve().parent

MAIN_TAIL = """    }) {
        Ok(msg) => println!("{msg}"),
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}
"""


def build_cargo_toml(name: str, engine: str) -> str:
    return f"""[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{name}"
path = "src/main.rs"

[dependencies]
{engine} = {{ path = "../../capability/{engine}" }}
schemas_embedded = {{ path = "../../capability/schemas_embedded" }}
"""


def build_output_path_rust(config: argparse.Namespace) -> str:
    kind = config.output_kind
    if kind == "fixed_file":
        if not config.file_path:
            logger.error("--file-path required for fixed_file")
            sys.exit(1)
        return (
            "OutputPath::FixedFile(\n"
            f'            base.join("{config.file_path}"),\n'
            "        )"
        )
    if kind == "directory_prefix":
        if not config.dir_path or not config.suffix:
            logger.error("--dir-path and --suffix required for directory_prefix")
            sys.exit(1)
        return (
            "OutputPath::DirectoryPrefix {\n"
            f'            dir: base.join("{config.dir_path}"),\n'
            f'            suffix: "{config.suffix}",\n'
            "        }"
        )
    if kind == "directory_name":
        if not config.dir_path or not config.ext:
            logger.error("--dir-path and --ext required for directory_name")
            sys.exit(1)
        return (
            "OutputPath::DirectoryName {\n"
            f'            dir: base.join("{config.dir_path}"),\n'
            f'            ext: "{config.ext}",\n'
            "        }"
        )
    logger.error("Unknown output_kind: {}", kind)
    sys.exit(1)


def rust_option_str(value: str | None) -> str:
    """Render an optional field name as a Rust `Option<&str>` literal."""
    return f'Some("{value}")' if value else "None"


def build_stamps_rust(config: argparse.Namespace) -> str:
    if not (config.stamp_sequence or config.stamp_timestamp or config.stamp_session):
        return "Stamps::NONE"
    return (
        "Stamps {\n"
        f"            sequence: {rust_option_str(config.stamp_sequence)},\n"
        f"            timestamp: {rust_option_str(config.stamp_timestamp)},\n"
        f"            session: {rust_option_str(config.stamp_session)},\n"
        "        }"
    )


def build_writer_main_rs(config: argparse.Namespace) -> str:
    format_variant = "Jsonl" if config.format == "jsonl" else "Json"
    freq_variant = "Record" if config.frequency == "record" else "Batch"
    batch_line = f"Some({config.batch_size})" if config.batch_size else "None"
    return (
        f"use schemas_embedded::{config.schema_const};\n"
        "use write_engine::{OutputFormat, OutputPath, Stamps, WriteFrequency, WriterConfig};\n"
        "\n"
        "fn main() {\n"
        "    let base = write_engine::ai_home();\n"
        "    match write_engine::run(&WriterConfig {\n"
        f'        name: "{config.name}",\n'
        f"        schema: &{config.schema_const},\n"
        f'        schema_source_path: base.join("{config.schema_path}").display().to_string(),\n'
        f"        format: OutputFormat::{format_variant},\n"
        f"        frequency: WriteFrequency::{freq_variant},\n"
        f"        output: {build_output_path_rust(config)},\n"
        f"        batch_size: {batch_line},\n"
        f"        stamps: {build_stamps_rust(config)},\n"
        + MAIN_TAIL
    )


def build_store_main_rs(config: argparse.Namespace) -> str:
    required = {
        "--open-file": config.open_file,
        "--sessions-dir": config.sessions_dir,
        "--index-file": config.index_file,
    }
    missing = [flag for flag, value in required.items() if not value]
    if missing:
        logger.error("--kind store requires {}", ", ".join(missing))
        sys.exit(1)
    return (
        f"use schemas_embedded::{config.schema_const};\n"
        "use store_engine::{StoreConfig, StoreOutcome};\n"
        "\n"
        "fn main() {\n"
        "    let base = store_engine::ai_home();\n"
        "    match store_engine::run(&StoreConfig {\n"
        f'        name: "{config.name}",\n'
        f'        open_file: base.join("{config.open_file}"),\n'
        f'        sessions_dir: base.join("{config.sessions_dir}"),\n'
        f'        index_file: base.join("{config.index_file}"),\n'
        f"        index_schema: &{config.schema_const},\n"
        f'        index_schema_source_path: base.join("{config.schema_path}").display().to_string(),\n'
        f'        session_field: "{config.session_field}",\n'
        f'        timestamp_field: "{config.timestamp_field}",\n'
        "    }) {\n"
        "        Ok(StoreOutcome::Stored(msg)) | Ok(StoreOutcome::Help(msg)) => println!(\"{msg}\"),\n"
        "        Err((code, msg)) => {\n"
        "            eprintln!(\"{msg}\");\n"
        "            std::process::exit(code);\n"
        "        }\n"
        "    }\n"
        "}\n"
    )


def build_invocation(name: str, output_kind: str) -> str:
    """Build the canonical invocation pattern from binary name and output kind."""
    if output_kind == "fixed_file":
        return f"{name} <<'EOF'\\n{{json_data}}\\nEOF"
    return f"{name} {{name}} <<'EOF'\\n{{json_data}}\\nEOF"


def build_registry_entry(config: argparse.Namespace) -> str:
    invocation = build_invocation(config.name, config.output_kind)
    lines = [
        "[[tools]]",
        f'binary_name = "{config.name}"',
        f'invocation = "{invocation}"',
        f'output_format = "{config.format}"',
        f'write_frequency = "{config.frequency}"',
        f'output_path_kind = "{config.output_kind}"',
        f'schema_path = "{config.schema_path}"',
    ]
    if config.output_kind == "fixed_file":
        lines.append(f'file_path = "{config.file_path}"')
    elif config.output_kind == "directory_name":
        lines.append(f'directory_path = "{config.dir_path}"')
        lines.append(f'name_extension = "{config.ext}"')
    elif config.output_kind == "directory_prefix":
        lines.append(f'directory_path = "{config.dir_path}"')
        lines.append(f'name_suffix = "{config.suffix}"')
    return "\n".join(lines)


def generate(config: argparse.Namespace) -> bool:
    """Generate the crate files. Returns True on success."""
    engine = "store_engine" if config.kind == "store" else "write_engine"
    cargo_toml = build_cargo_toml(config.name, engine)
    main_rs = build_store_main_rs(config) if config.kind == "store" else build_writer_main_rs(config)
    crate_dir = NORNIR_DIR / "writers" / config.name

    if config.dry_run:
        sys.stdout.write(f"--- writers/{config.name}/Cargo.toml ---\n")
        sys.stdout.write(cargo_toml)
        sys.stdout.write(f"--- writers/{config.name}/src/main.rs ---\n")
        sys.stdout.write(main_rs)
    else:
        crate_dir.mkdir(parents=True, exist_ok=True)
        (crate_dir / "src").mkdir(exist_ok=True)
        (crate_dir / "Cargo.toml").write_text(cargo_toml)
        (crate_dir / "src" / "main.rs").write_text(main_rs)
        logger.info("Generated: writers/{}/Cargo.toml", config.name)
        logger.info("Generated: writers/{}/src/main.rs", config.name)

    sys.stdout.write("\n=== Manual steps required ===\n\n")
    sys.stdout.write("Add to workspace Cargo.toml members:\n")
    sys.stdout.write(f'    "writers/{config.name}",\n\n')
    sys.stdout.write("Add to deploy_categories.toml [writers].crates:\n")
    sys.stdout.write(f'    "{config.name}",\n\n')

    if config.kind == "writer":
        sys.stdout.write("Add to tool_registry.toml:\n")
        sys.stdout.write(build_registry_entry(config))
        sys.stdout.write("\n")

    return True


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate a Nornir writer or store crate")
    parser.add_argument("--kind", choices=["writer", "store"], default="writer")
    parser.add_argument("--name", required=True, help="Binary name = crate name = directory name")
    parser.add_argument("--schema-const", required=True, help="Rust const in schemas_embedded")
    parser.add_argument("--schema-path", required=True, help="Schema path relative to the ai root")

    writer = parser.add_argument_group("writer")
    writer.add_argument("--format", choices=["jsonl", "json"])
    writer.add_argument("--frequency", choices=["record", "batch"])
    writer.add_argument("--output-kind", choices=["fixed_file", "directory_prefix", "directory_name"])
    writer.add_argument("--file-path", help="fixed_file: output file, relative to the ai root")
    writer.add_argument("--dir-path", help="directory_*: output directory, relative to the ai root")
    writer.add_argument("--suffix", help="directory_prefix: suffix appended to the runtime name")
    writer.add_argument("--ext", help="directory_name: extension without the dot")
    writer.add_argument("--batch-size", type=int, help="Batch size limit")
    writer.add_argument("--stamp-sequence", help="Field set to line count + 1")
    writer.add_argument("--stamp-timestamp", help="Field set to the clock, ISO 8601 Zulu")
    writer.add_argument("--stamp-session", help="Field set to the file's session id")

    store = parser.add_argument_group("store")
    store.add_argument("--open-file", help="The open append file, relative to the ai root")
    store.add_argument("--sessions-dir", help="Where stored sessions go, relative to the ai root")
    store.add_argument("--index-file", help="The append-only session index, relative to the ai root")
    store.add_argument("--session-field", default="session", help="Record field carrying the session id")
    store.add_argument("--timestamp-field", default="ts", help="Record field carrying the timestamp")

    parser.add_argument("--dry-run", action="store_true", help="Print files without writing")

    args = parser.parse_args()
    if args.kind == "writer" and not (args.format and args.frequency and args.output_kind):
        parser.error("--kind writer requires --format, --frequency and --output-kind")
    return args


if __name__ == "__main__":
    generate(parse_args())
