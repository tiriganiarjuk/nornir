use schemas_embedded::SUMMARIES;
use write_engine::{OutputFormat, OutputPath, Stamps, WriteFrequency, WriterConfig};

fn main() {
    let base = write_engine::ai_home();
    match write_engine::run(&WriterConfig {
        name: "append_interview_summaries_record",
        schema: &SUMMARIES,
        schema_source_path: base.join("spaces/bragi/schemas/summaries.schema.json").display().to_string(),
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Record,
        output: OutputPath::DirectoryPrefix {
            dir: base.join("spaces/bragi/interview/interviews"),
            suffix: ".summaries.jsonl",
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
