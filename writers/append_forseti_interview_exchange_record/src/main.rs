use schemas_embedded::INTERVIEW_EXCHANGE;
use write_engine::{OutputFormat, OutputPath, Stamps, WriteFrequency, WriterConfig};

fn main() {
    let base = write_engine::ai_home();
    match write_engine::run(&WriterConfig {
        name: "append_forseti_interview_exchange_record",
        schema: &INTERVIEW_EXCHANGE,
        schema_source_path: base.join("smidja/nornir/schemas/tools/interview-exchange.schema.json").display().to_string(),
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Record,
        output: OutputPath::FixedFile(
            base.join("spaces/forseti/interview/OPEN.jsonl"),
        ),
        batch_size: None,
        stamps: Stamps {
            sequence: Some("exchange"),
            timestamp: Some("ts"),
            session: Some("session"),
        },
    }) {
        Ok(msg) => println!("{msg}"),
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}
