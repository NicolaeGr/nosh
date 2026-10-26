mod app;
mod bar;
mod menu;

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("nosh=info")),
        )
        .init();

    let arguments: Vec<std::ffi::OsString> = std::env::args_os().collect();

    if let Err(message) = validate_arguments(&arguments) {
        eprintln!("nosh: {message}");
        return ExitCode::FAILURE;
    }

    match app::run(arguments) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("nosh: {error}");
            ExitCode::FAILURE
        }
    }
}

fn validate_arguments(arguments: &[std::ffi::OsString]) -> Result<(), String> {
    let words: Vec<&str> = arguments
        .iter()
        .skip(1)
        .filter_map(|argument| argument.to_str())
        .collect();

    match words.as_slice() {
        [] => Ok(()),
        ["menu", id] if !id.is_empty() => Ok(()),
        ["menu"] => Err("menu needs an id: nosh menu <id>".to_string()),
        _ => Err("usage: nosh menu <id>".to_string()),
    }
}
