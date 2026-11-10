use schemas_embedded::RAW_JSONL_RECORD;
use write_engine::{OutputFormat, OutputPath, Stamps, WriteFrequency, WriterConfig};

fn main() {
    let base = write_engine::ai_home();
    match write_engine::run(&WriterConfig {
        name: "append_raw_jsonl",
        schema: &RAW_JSONL_RECORD,
        schema_source_path: "schemas/tools/raw-jsonl.schema.json".into(),
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Record,
        output: OutputPath::DirectoryName {
            dir: base.join("traffic"),
            ext: "jsonl",
        },
        batch_size: None,
        stamps: Stamps::NONE,
    }) {
        Ok(msg) => println!("{msg}"),
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}
