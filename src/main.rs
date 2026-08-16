//! Application entry point: parses CLI arguments, dispatches `status`/`msg`
//! subcommands, and otherwise initializes the terminal and launches the main
//! `App` loop until quit.

use std::io::stdout;

use clap::Parser;
use crossterm::execute;

use pigma::cli::{Cli, run_cli};

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    color_eyre::install()?;
    let cli = Cli::parse();

    let app = match run_cli(cli).await? {
        Some(app) => app,
        None => return Ok(()),
    };

    let terminal = ratatui::init();
    execute!(stdout(), crossterm::event::EnableMouseCapture)?;
    let result = app.run(terminal).await;
    execute!(stdout(), crossterm::event::DisableMouseCapture)?;
    ratatui::restore();
    result
}
