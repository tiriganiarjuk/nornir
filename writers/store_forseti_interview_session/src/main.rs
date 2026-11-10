use schemas_embedded::INTERVIEW_SESSION;
use store_engine::{StoreConfig, StoreOutcome};

fn main() {
    let base = store_engine::ai_home();
    match store_engine::run(&StoreConfig {
        name: "store_forseti_interview_session",
        open_file: base.join("spaces/forseti/interview/OPEN.jsonl"),
        sessions_dir: base.join("spaces/forseti/interview/sessions"),
        index_file: base.join("spaces/forseti/interview/index.jsonl"),
        index_schema: &INTERVIEW_SESSION,
        index_schema_source_path: base.join("smidja/nornir/schemas/tools/interview-session.schema.json").display().to_string(),
        session_field: "session",
        timestamp_field: "ts",
    }) {
        Ok(StoreOutcome::Stored(msg)) | Ok(StoreOutcome::Help(msg)) => println!("{msg}"),
        Err((code, msg)) => {
            eprintln!("{msg}");
            std::process::exit(code);
        }
    }
}
