use mundus_animarum_cli::error::Error;

#[tokio::main]
async fn main() {
    let _ = dotenv::dotenv();
    let code = match mundus_animarum_cli::run(std::env::args_os()).await {
        // Terminal success: emit the command's JSON result as one JSONL line.
        Ok(value) => {
            print_json(serde_json::to_string(&value));
            0
        }
        // `--help` / `--version` / missing subcommand: informational, not a
        // failure. Emit clap's text as a help line and exit 0.
        Err(Error::Clap(e)) if mundus_animarum_cli::is_informational(&e) => {
            print_json(serde_json::to_string(
                &serde_json::json!({ "type": "help", "help": e.to_string() }),
            ));
            0
        }
        // Real failure: emit an objectiveai SDK error frame and exit non-zero.
        Err(e) => {
            let payload = objectiveai_sdk::cli::Error {
                r#type: objectiveai_sdk::cli::ErrorType::Error,
                level: Some(objectiveai_sdk::cli::Level::Error),
                fatal: Some(true),
                message: serde_json::Value::String(e.to_string()),
            };
            print_json(serde_json::to_string(&payload));
            1
        }
    };
    std::process::exit(code);
}

/// Print one JSONL line, falling back to an inline error frame if the
/// payload itself fails to serialize.
fn print_json(line: Result<String, serde_json::Error>) {
    match line {
        Ok(line) => println!("{line}"),
        Err(e) => {
            println!(r#"{{"type":"error","fatal":true,"message":"serialize error: {e}"}}"#)
        }
    }
}
