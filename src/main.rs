//! Application entry point: parses CLI arguments, dispatches `status`/`msg`
//! subcommands, and otherwise initializes the terminal and launches the main
//! `App` loop until quit.

use std::io::stdout;

use clap::{Parser, error::ErrorKind};
use crossterm::execute;
use pigma::cli::{Cli, run_cli};

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();

    // CLI subcommands (`status`/`msg`/`completions`) are one-shot queries; when
    // they fail, print a single clean line instead of the color_eyre trace.
    let app = match run_cli(cli).await {
        Ok(app) => app,
        Err(err) => {
            let msg = err
                .chain()
                .next()
                .map(ToString::to_string)
                .unwrap_or_default();
            clap::Error::raw(ErrorKind::Io, msg)
                .print()
                .expect("failed to write error");
            eprintln!();
            std::process::exit(1);
        }
    };
    let Some(app) = app else {
        return Ok(());
    };

    color_eyre::install()?;
    let terminal = ratatui::init();
    execute!(stdout(), crossterm::event::EnableMouseCapture)?;
    let result = app.run(terminal).await;
    execute!(stdout(), crossterm::event::DisableMouseCapture)?;
    ratatui::restore();
    result
}
