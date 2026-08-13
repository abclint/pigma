//! Application entry point: parses CLI arguments, dispatches `status`/`msg`
//! subcommands, and otherwise initializes the terminal and launches the main
//! `App` loop until quit.

use std::io::stdout;

use clap::Parser;
use crossterm::execute;

use pigma::app::App;
use pigma::cli::{self, Cli, Command};
use pigma::config::Config;
use pigma::logger::init_logger;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Status { template, format }) => {
            return cli::status(&template, format.as_deref()).await;
        }
        Some(Command::Msg { action, value }) => {
            return cli::msg(&action, value.as_deref()).await;
        }
        None => {}
    }

    let _ = rustls::crypto::ring::default_provider().install_default();
    color_eyre::install()?;
    let config = Config::load();
    init_logger(&config)?;
    let terminal = ratatui::init();
    execute!(stdout(), crossterm::event::EnableMouseCapture)?;
    let result = App::new(config)?.run(terminal).await;
    execute!(stdout(), crossterm::event::DisableMouseCapture)?;
    ratatui::restore();
    result
}
