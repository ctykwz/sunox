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

use std::io::Write;

#[tokio::main]
async fn main() {
    let result = tokio::select! {
        result = app::run() => result,
        signal = tokio::signal::ctrl_c() => match signal {
            Ok(()) => Err(core::CliError::Interrupted),
            Err(error) => Err(core::CliError::Io(error)),
        },
    };
    if let Err(e) = result {
        let json_mode = std::env::args().any(|a| a == "--json")
            || !std::io::IsTerminal::is_terminal(&std::io::stdout());

        if json_mode {
            output::json::error_with_details(
                e.error_code(),
                &e.to_string(),
                e.suggestion(),
                e.details(),
            );
        } else {
            eprintln!("Error [{}]: {}", e.error_code(), e);
            eprintln!("Hint: {}", e.suggestion());
        }
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        std::process::exit(e.exit_code());
    }
}
