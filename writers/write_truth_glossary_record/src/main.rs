use schemas_embedded::GLOSSARY;
use write_engine::{OutputFormat, OutputPath, Stamps, WriteFrequency, WriterConfig};

fn main() {
    let base = write_engine::ai_home();
    match write_engine::run(&WriterConfig {
        name: "write_truth_glossary_record",
        schema: &GLOSSARY,
        schema_source_path: base.join("spaces/bragi/schemas/glossary.schema.json").display().to_string(),
        format: OutputFormat::Json,
        frequency: WriteFrequency::Record,
        output: OutputPath::DirectoryName {
            dir: base.join("spaces/bragi/truth/quarantine"),
            ext: "json",
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
