mod app;
mod bar;

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("nosh=info")),
        )
        .init();

    match app::run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("nosh: {error}");
            ExitCode::FAILURE
        }
    }
}
