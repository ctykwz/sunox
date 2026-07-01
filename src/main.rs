mod api;
mod app;
mod auth;
mod browser;
mod captcha;
mod cli;
mod commands;
mod core;
mod media;
mod net;
mod output;
mod workflow;

#[tokio::main]
async fn main() {
    if let Err(e) = app::run().await {
        let json_mode = std::env::args().any(|a| a == "--json")
            || !std::io::IsTerminal::is_terminal(&std::io::stdout());

        if json_mode {
            output::json::error(e.error_code(), &e.to_string(), e.suggestion());
        } else {
            eprintln!("Error [{}]: {}", e.error_code(), e);
            eprintln!("Hint: {}", e.suggestion());
        }
        std::process::exit(e.exit_code());
    }
}
