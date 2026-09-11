#![recursion_limit = "256"]

mod api;
mod app;
mod auth;
mod browser;
mod browser_bridge;
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
    // Tokio's worker threads have more stack headroom than the Windows executable main thread.
    // Run the command dispatcher there so deeply nested async command paths cannot overflow the
    // platform's smaller main-thread stack before returning validation errors.
    let recovery = core::operation::OperationRecovery::new();
    let task_recovery = recovery.clone();
    let mut app_task = tokio::spawn(async move { task_recovery.scope(app::run()).await });
    let result = tokio::select! {
        result = &mut app_task => match result {
            Ok(result) => result,
            Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
            Err(_) => Err(core::CliError::Interrupted),
        },
        signal = tokio::signal::ctrl_c() => {
            recovery.cancel();
            app_task.abort();
            // Await cancellation so owned temporary files and guards can be dropped. Keep exit
            // bounded even when a legacy blocking filesystem/browser call has not yielded yet.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), &mut app_task).await;
            match signal {
                Ok(()) => Err(core::CliError::Interrupted),
                Err(error) => Err(core::CliError::Io(error)),
            }
        },
    };
    if let Err(e) = result {
        let details = recovery.error_details(&e);
        let json_mode = std::env::args().any(|a| a == "--json")
            || !std::io::IsTerminal::is_terminal(&std::io::stdout());

        if json_mode {
            output::json::error_with_details(
                e.error_code(),
                &e.to_string(),
                e.suggestion(),
                details.as_ref(),
            );
        } else {
            eprintln!("Error [{}]: {}", e.error_code(), e);
            eprintln!("Hint: {}", e.suggestion());
            if let Some(recovery) = details
                .as_ref()
                .and_then(|details| details.get("operation_recovery"))
            {
                if let Some(path) = recovery["checkpoint_path"].as_str() {
                    eprintln!("Recovery checkpoint: {path}");
                }
                if let Some(commands) = recovery["inspection_commands"].as_array() {
                    for command in commands.iter().filter_map(serde_json::Value::as_str) {
                        eprintln!("Inspect: {command}");
                    }
                }
            }
        }
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        std::process::exit(e.exit_code());
    }
    if let Err(error) = recovery.finish() {
        eprintln!(
            "Warning: command completed, but its recovery checkpoint could not be cleaned up: {error}"
        );
    }
}
