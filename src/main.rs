pub mod cli;
pub mod mcp;
pub mod workspace;

use serde_json::json;

#[tokio::main]
async fn main() {
    if let Err(err) = cli::run_cli().await {
        let err_json = json!({
            "status": "error",
            "error": err.to_string(),
        });
        eprintln!("{}", err_json);
        std::process::exit(1);
    }
}
