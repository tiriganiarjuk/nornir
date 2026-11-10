use schemas_embedded::EMBEDDING_TARGET;
use write_engine::{OutputFormat, OutputPath, Stamps, WriteFrequency, WriterConfig};

fn main() {
    let base = write_engine::ai_home();
    match write_engine::run(&WriterConfig {
        name: "append_embedding_normalize_batch_20",
        schema: &EMBEDDING_TARGET,
        schema_source_path: base.join("spaces/bragi/definitions/schemas/embedding-target.schema.json").display().to_string(),
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Batch,
        output: OutputPath::FixedFile(
            base.join("spaces/bragi/interview/embedding_format/normalized.jsonl"),
        ),
        batch_size: Some(20),
        stamps: Stamps::NONE,
    }) {
        Ok(msg) => println!("{msg}"),
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}
