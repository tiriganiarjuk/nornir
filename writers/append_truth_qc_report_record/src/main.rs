use schemas_embedded::QC_REPORT;
use write_engine::{OutputFormat, OutputPath, Stamps, WriteFrequency, WriterConfig};

fn main() {
    let base = write_engine::ai_home();
    match write_engine::run(&WriterConfig {
        name: "append_truth_qc_report_record",
        schema: &QC_REPORT,
        schema_source_path: base.join("spaces/bragi/schemas/qc-report.schema.json").display().to_string(),
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Record,
        output: OutputPath::FixedFile(
            base.join("spaces/bragi/truth/qc_semantic_report.jsonl"),
        ),
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
