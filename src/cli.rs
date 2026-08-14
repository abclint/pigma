//! CLI entry: `pigma status` and `pigma msg` subcommands plus the argument
//! parser. The TUI itself runs when no subcommand is given.

use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::ipc::{self, MsgAction, StatusSnapshot};

#[derive(Debug, Parser)]
#[command(
    name = "pigma",
    version,
    about = "A netease cloud music client",
    long_about = "A netease cloud music client.\n\nWithout a subcommand this launches the interactive TUI. `pigma status` reports the state of a running instance and `pigma msg` controls it."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show the status of the running pigma instance.
    Status {
        /// Output template (plain format only). Tokens: {current}, {duration},
        /// {artist}, {name}, {album}, {volume}, {status}, {position}, {mode},
        /// {id}, {liked}. Defaults to config's `cli_status_template`.
        #[arg(long, default_value = "")]
        template: String,
        /// Output format: `json` or `plain`. Defaults to config's
        /// `cli_status_format`.
        #[arg(long)]
        format: Option<String>,
    },
    /// Control the running pigma instance.
    Msg {
        /// Action: previous | next | pause | play | volume | mode | like | dislike.
        action: String,
        /// Value for `volume`: absolute percent (e.g. `75`) or a signed delta
        /// percent (e.g. `+5`, `-5`) matching the TUI's `+`/`-` behavior.
        value: Option<String>,
    },
}

/// `pigma status` handler.
pub async fn status(template: &str, format: Option<&str>) -> color_eyre::Result<()> {
    let config = Config::load();
    let snapshot = ipc::fetch_status().await?;
    let format = format.unwrap_or(&config.cli_status_format);
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        "plain" => {
            let template = if template.is_empty() {
                &config.cli_status_template
            } else {
                template
            };
            println!("{}", format_status(template, &snapshot));
        }
        other => {
            eprintln!("unknown format `{other}` (expected json or plain)");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// `pigma msg` handler.
pub async fn msg(action: &str, value: Option<&str>) -> color_eyre::Result<()> {
    let action = parse_msg_action(action, value)?;
    ipc::send_msg(action).await?;
    Ok(())
}

fn parse_msg_action(action: &str, value: Option<&str>) -> color_eyre::Result<MsgAction> {
    match action {
        "previous" | "prev" => Ok(MsgAction::Previous),
        "next" => Ok(MsgAction::Next),
        "pause" => Ok(MsgAction::Pause),
        "play" => Ok(MsgAction::Play),
        "volume" => {
            let value = value.ok_or_else(|| {
                color_eyre::eyre::eyre!("volume requires a value like `75`, `+5` or `-5`")
            })?;
            parse_volume(value)
        }
        "mode" => Ok(MsgAction::Mode),
        "like" => Ok(MsgAction::Like),
        "dislike" => Ok(MsgAction::Dislike),
        other => Err(color_eyre::eyre::eyre!(
            "unknown action `{other}` (expected previous/next/pause/play/volume/mode/like/dislike)"
        )),
    }
}

/// Parse a volume value. A leading `+`/`-` is a delta (percent) applied via
/// `adjust_volume`, matching the TUI's `+`/`-` keys; a bare number is an
/// absolute volume percent.
fn parse_volume(value: &str) -> color_eyre::Result<MsgAction> {
    let err = || color_eyre::eyre::eyre!("invalid volume `{value}`");
    if let Some(delta) = value.strip_prefix('+').or_else(|| value.strip_prefix('-')) {
        let number: f64 = delta.parse().map_err(|_| err())?;
        let signed = if value.starts_with('-') { -number } else { number };
        Ok(MsgAction::Volume {
            delta: Some(signed / 100.0),
            absolute: None,
        })
    } else {
        let number: f64 = value.parse().map_err(|_| err())?;
        if !(0.0..=100.0).contains(&number) {
            return Err(err());
        }
        Ok(MsgAction::Volume {
            delta: None,
            absolute: Some(number / 100.0),
        })
    }
}

/// Replace `{token}` placeholders in a plain-status template.
fn format_status(template: &str, s: &StatusSnapshot) -> String {
    let duration = format_duration(s.duration_ms);
    let current = format_duration(s.position_ms);
    let volume = (s.volume * 100.0).round() as u64;
    let status = if !s.playing {
        "stopped"
    } else if s.paused {
        "paused"
    } else {
        "playing"
    };
    template
        .replace("{current}", &current)
        .replace("{position}", &current)
        .replace("{duration}", &duration)
        .replace("{artist}", &s.artist)
        .replace("{name}", &s.name)
        .replace("{album}", &s.album)
        .replace("{volume}", &volume.to_string())
        .replace("{status}", status)
        .replace("{mode}", &s.mode)
        .replace("{id}", &s.id.to_string())
        .replace("{liked}", if s.liked { "true" } else { "false" })
}

fn format_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    format!("{}:{:02}", total_secs / 60, total_secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> StatusSnapshot {
        StatusSnapshot {
            id: 123,
            name: "Example Song".into(),
            artist: "Example Artist".into(),
            album: "Example Album".into(),
            duration_ms: 125000,
            position_ms: 65000,
            volume: 0.75,
            playing: true,
            paused: false,
            mode: "sequential".into(),
            liked: true,
        }
    }

    #[test]
    fn format_status_plain() {
        let out = format_status(
            "{current}/{duration} {artist} {name} {volume}% {status}",
            &snapshot(),
        );
        assert_eq!(out, "1:05/2:05 Example Artist Example Song 75% playing");
    }

    #[test]
    fn format_status_unknown_tokens_left_alone() {
        let out = format_status("{name} {bogus}", &snapshot());
        assert_eq!(out, "Example Song {bogus}");
    }

    #[test]
    fn parse_volume_absolute() {
        let action = parse_volume("75").unwrap();
        assert_eq!(
            action,
            MsgAction::Volume {
                delta: None,
                absolute: Some(0.75),
            }
        );
    }

    #[test]
    fn parse_volume_delta() {
        let plus = parse_volume("+5").unwrap();
        assert_eq!(
            plus,
            MsgAction::Volume {
                delta: Some(0.05),
                absolute: None,
            }
        );
        let minus = parse_volume("-10").unwrap();
        assert_eq!(
            minus,
            MsgAction::Volume {
                delta: Some(-0.10),
                absolute: None,
            }
        );
    }

    #[test]
    fn parse_volume_out_of_range() {
        assert!(parse_volume("150").is_err());
        assert!(parse_volume("abc").is_err());
    }
}
