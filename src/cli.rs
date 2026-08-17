//! CLI entry: `pigma status` / `pigma msg` / `pigma list` subcommands plus the
//! argument parser. The TUI itself runs when no subcommand is given.

use std::{env, path::PathBuf, sync::Arc};

use clap::{
    Parser, Subcommand,
    builder::{Styles, styling::AnsiColor},
};
use serde::Serialize;
use tokio::io::AsyncBufReadExt;

use crate::{
    app::App,
    cli,
    config::Config,
    ipc::{self, MsgAction, StatusSnapshot},
    logger::init_logger,
    utils::format_duration,
};

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Yellow.on_default().bold())
    .usage(AnsiColor::Yellow.on_default().bold())
    .literal(AnsiColor::Cyan.on_default().bold())
    .placeholder(AnsiColor::Cyan.on_default());

#[derive(Debug, Parser)]
#[command(
    name = "pigma",
    version,
    disable_version_flag = true,
    about = "A netease cloud music client",
    long_about = "A netease cloud music client.\n\nNo subcommand launches the TUI; `status` / `msg` / `list` query or control a running instance.",
    args_conflicts_with_subcommands = true,
    styles = STYLES
)]
pub struct Cli {
    /// Print version and exit.
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    pub version: Option<bool>,
    /// Run headless, loading an endpoint (default `__liked__`).
    #[arg(
        long,
        short = 'd',
        value_name = "ENDPOINT",
        num_args = 0..=1,
        default_missing_value = "__liked__"
    )]
    pub daemon: Option<String>,
    /// Load the N-th playlist of the endpoint (1-based).
    #[arg(long, value_name = "INDEX")]
    pub playlist: Option<usize>,
    /// IPC socket path.
    #[arg(long, value_name = "SOCKET")]
    pub socket: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show the running instance's status.
    Status {
        /// Output template: {name} {artist} {album} {duration} {position}
        /// {volume} {status} {mode} {id} {liked}. Defaults to config.
        #[arg(long, default_value = "")]
        template: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
        /// List the playback queue (`>` marks the current song).
        #[arg(short = 'L', long)]
        list: bool,
        /// Output waybar-compatible JSON (icons, classes, tooltip, marquee).
        #[arg(long)]
        waybar: bool,
        /// Button icon for `--waybar`: `like` | `play` | `prev` | `next`.
        #[arg(long, value_name = "NAME", value_parser = ["like", "play", "prev", "next"])]
        icon: Option<String>,
        /// Stream status updates (subscribe) instead of printing once. Runs
        /// forever; each snapshot change prints one waybar JSON line.
        #[arg(long)]
        watch: bool,
        /// Marquee window width in characters (default 20, or `$PIGMA_WINDOW`).
        #[arg(long, value_name = "N")]
        window: Option<usize>,
        /// Marquee scroll speed in characters/second (default 7, or `$PIGMA_SPEED`).
        #[arg(long, value_name = "N")]
        speed: Option<usize>,
        /// IPC socket path.
        #[arg(long, value_name = "SOCKET")]
        socket: Option<PathBuf>,
    },
    /// List an endpoint's playlists (or songs) with 1-based indexes.
    List {
        /// API endpoint: `toplist`, `top_song_list`, `__liked__`, etc.
        endpoint: String,
    },
    /// Control the running instance.
    Msg {
        /// Action: previous | next | pause | play | toggle_play | volume |
        /// mode | like | dislike | toggle_like | switch-list | search.
        action: String,
        /// Value for `play` (a song id), `search` (a keyword), `volume`
        /// (`75`/`+5`/`-5`) or the endpoint for `switch-list`.
        #[arg(allow_hyphen_values = true)]
        value: Option<String>,
        /// Playlist index for `switch-list` (1-based).
        #[arg(long, value_name = "INDEX")]
        playlist: Option<usize>,
        /// IPC socket path.
        #[arg(long, value_name = "SOCKET")]
        socket: Option<PathBuf>,
    },
}

/* -------------------------------------------------------------------------- */
/*                               command actions                              */
/* -------------------------------------------------------------------------- */

/// `pigma status` handler.
#[allow(clippy::too_many_arguments)]
pub async fn status(
    template: &str,
    json: bool,
    list: bool,
    waybar: bool,
    icon: Option<&str>,
    watch: bool,
    window: Option<usize>,
    speed: Option<usize>,
) -> color_eyre::Result<()> {
    if waybar {
        let window = window
            .or_else(|| env::var("PIGMA_WINDOW").ok().and_then(|v| v.parse().ok()))
            .unwrap_or(20);
        let speed = speed
            .or_else(|| env::var("PIGMA_SPEED").ok().and_then(|v| v.parse().ok()))
            .unwrap_or(7);
        if watch {
            watch_waybar(icon, window, speed).await?;
        } else {
            let out = match ipc::fetch_status().await {
                Ok(snapshot) => waybar_output(&snapshot, icon, window, speed),
                Err(_) => waybar_off(icon),
            };
            println!("{}", serde_json::to_string(&out)?);
        }
        return Ok(());
    }
    if icon.is_some() || watch {
        color_eyre::eyre::bail!("--icon/--watch require --waybar");
    }
    let config = Config::load();
    if list {
        let queue = ipc::fetch_queue().await?;
        if json {
            println!("{}", serde_json::to_string_pretty(&queue)?);
        } else {
            let current = queue.current_index;
            for (i, song) in queue.songs.iter().enumerate() {
                let marker = if Some(i) == current { ">" } else { " " };
                println!(
                    "{marker} {:<3} {:<32} {:<24} {}",
                    i + 1,
                    song.name,
                    song.singer,
                    format_duration(song.duration_ms)
                );
            }
        }
        return Ok(());
    }
    let snapshot = ipc::fetch_status().await?;
    if json || config.cli_status_format == "json" {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        let template = if template.is_empty() {
            &config.cli_status_template
        } else {
            template
        };
        println!("{}", format_status(template, &snapshot));
    }
    Ok(())
}

/* ------------------------------- waybar output ------------------------------ */

/// A single waybar custom-module update (`return-type: json`).
#[derive(Debug, Serialize)]
struct WaybarJson {
    text: String,
    class: Vec<String>,
    alt: String,
    tooltip: String,
}

/// Format one status snapshot as waybar JSON. `icon` selects a button module
/// (`like`/`play`/`prev`/`next`); `None` renders the main track display.
fn waybar_output(
    s: &StatusSnapshot,
    icon: Option<&str>,
    window: usize,
    speed: usize,
) -> WaybarJson {
    match icon {
        Some(name) => waybar_icon(name, s),
        None => waybar_main(s, window, speed),
    }
}

/// The "daemon not running" fallback: the main module shows `off`, button
/// modules fall back to their default icon.
fn waybar_off(icon: Option<&str>) -> WaybarJson {
    match icon {
        Some(name) => waybar_icon(name, &StatusSnapshot::default()),
        None => WaybarJson {
            text: "♪  off".into(),
            class: vec!["off".into()],
            alt: "off".into(),
            tooltip: "pigma is not running".into(),
        },
    }
}

/// Main track display: status icon + marquee title, with a multi-line tooltip.
fn waybar_main(s: &StatusSnapshot, window: usize, speed: usize) -> WaybarJson {
    if s.name.is_empty() {
        return WaybarJson {
            text: "♪  idle".into(),
            class: vec!["idle".into()],
            alt: "idle".into(),
            tooltip: "no song playing".into(),
        };
    }
    let (icon, cls, alt) = if !s.playing {
        ("■", "stopped", "stopped")
    } else if s.paused {
        ("⏸", "paused", "paused")
    } else {
        ("▶", "playing", "playing")
    };
    let full = format!("{} — {}", s.name, s.artist);
    let body = if s.playing && !s.paused {
        marquee(&full, window, speed)
    } else {
        full
    };
    let heart = if s.liked { "" } else { "" };
    let vol_pct = (s.volume * 100.0).round() as u64;
    let tooltip = format!(
        "{}\n{} — {}\n{} / {} · vol {}% · {} {}",
        s.name,
        s.artist,
        s.album,
        fmt_min_sec(s.position_ms),
        fmt_min_sec(s.duration_ms),
        vol_pct,
        s.mode,
        heart
    );
    WaybarJson {
        text: format!("{icon} {body}"),
        class: vec![cls.to_string()],
        alt: alt.into(),
        tooltip,
    }
}

/// A button module icon (replaces the old `pigma --icon` bash wrapper).
fn waybar_icon(name: &str, s: &StatusSnapshot) -> WaybarJson {
    let (text, class) = match name {
        "like" => {
            if s.liked {
                ("", vec!["button".to_string(), "liked".to_string()])
            } else {
                ("", vec!["button".to_string()])
            }
        }
        "play" => {
            // Shows the action the button performs on click: pause while
            // playing, play otherwise.
            if s.playing && !s.paused {
                ("⏸", vec!["button".to_string()])
            } else {
                ("▶", vec!["button".to_string()])
            }
        }
        "prev" => ("", vec!["button".to_string()]),
        "next" => ("", vec!["button".to_string()]),
        _ => ("♪", vec!["button".to_string()]),
    };
    WaybarJson {
        text: text.into(),
        class,
        alt: String::new(),
        tooltip: String::new(),
    }
}

/// Deterministic marquee scroll: the window slides over `full + gap + full`,
/// with the offset derived from wall-clock time so every invocation (and every
/// waybar re-run) shows the same frame at the same instant.
fn marquee(full: &str, window: usize, speed: usize) -> String {
    let len = full.chars().count();
    if len <= window {
        return full.to_string();
    }
    const GAP: usize = 8;
    let span = len + GAP;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let pos = ((now_ms / 1000) * speed as u64 % span as u64) as usize;
    let mut doubled = String::with_capacity(2 * len + GAP);
    doubled.push_str(full);
    doubled.extend(std::iter::repeat_n(' ', GAP));
    doubled.push_str(full);
    let chars: Vec<char> = doubled.chars().collect();
    let end = (pos + window).min(chars.len());
    chars[pos..end].iter().collect()
}

/// `M:SS` clock for waybar tooltips (matches the previous bash `fmt_time`).
fn fmt_min_sec(ms: u64) -> String {
    format!("{}:{:02}", ms / 60000, (ms % 60000) / 1000)
}

/// `pigma status --waybar --watch`: long-running subscriber. Connects to the
/// daemon, prints one waybar JSON line per snapshot change, and keeps retrying
/// (showing `off`) when the daemon is not running. Waybar runs this as a
/// continuous script and re-renders its module on each line.
async fn watch_waybar(icon: Option<&str>, window: usize, speed: usize) -> color_eyre::Result<()> {
    loop {
        match ipc::subscribe_status().await {
            Ok(mut reader) => {
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        // EOF / error: the daemon stopped or restarted.
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if let Ok(snapshot) = serde_json::from_str::<StatusSnapshot>(&line) {
                                let out = waybar_output(&snapshot, icon, window, speed);
                                println!("{}", serde_json::to_string(&out)?);
                            }
                        }
                    }
                }
            }
            Err(_) => {
                let out = waybar_off(icon);
                println!("{}", serde_json::to_string(&out)?);
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

/// `pigma list` handler: resolve an endpoint without a running daemon and print
/// the playlists (or songs) it yields, numbered by their 1-based index.
pub async fn list(endpoint: &str) -> color_eyre::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = Config::load();

    let cookie_path = crate::utils::pigma_config_dir().join("cookies.json");
    let mut api_builder = ncm_api::NcmClient::builder().cookie_path(cookie_path);
    let ncm_proxy = match config.proxy_target {
        crate::config::ProxyTarget::Reversed | crate::config::ProxyTarget::Both => {
            config.proxy.as_str()
        }
        _ => "",
    };
    if !ncm_proxy.is_empty() {
        api_builder = api_builder.proxy(ncm_proxy);
    }
    let api = Arc::new(api_builder.build()?);

    let cache_dir = {
        let path = std::path::Path::new(&config.cache.cache_dir);
        if path.is_absolute() {
            std::path::PathBuf::from(&config.cache.cache_dir)
        } else {
            crate::utils::pigma_cache_dir().join(&config.cache.cache_dir)
        }
    };
    let cache = Arc::new(crate::cache::CacheManager::new(
        cache_dir,
        crate::utils::pigma_cache_dir(),
        config.cache.cache_template.clone(),
    ));
    let service = crate::service::ApiService::new(api.clone(), cache);

    let uid = if api.is_logged_in() {
        api.login_status().await.ok().map(|info| info.uid)
    } else {
        None
    };

    let api_ep = crate::service::ApiEndpoint::parse(endpoint).unwrap_or_else(|| {
        eprintln!("未知端点: {endpoint}");
        std::process::exit(1);
    });

    let content = service
        .resolve_endpoint_content(api_ep, uid, config.search_limit)
        .await;

    match content {
        crate::state::ContentState::SongLists(lists) => {
            for (i, list) in lists.iter().enumerate() {
                println!("{:>3}. {}", i + 1, list.name);
            }
        }
        crate::state::ContentState::TopLists(lists) => {
            for (i, list) in lists.iter().enumerate() {
                println!("{:>3}. {}", i + 1, list.name);
            }
        }
        crate::state::ContentState::Songs(songs) => {
            for (i, song) in songs.iter().enumerate() {
                println!("{:>3}. {} - {}", i + 1, song.name, song.singer);
            }
        }
        crate::state::ContentState::Error(e) => {
            eprintln!("{endpoint}: {e}");
        }
        _ => eprintln!("{endpoint}: 无可列出的内容"),
    }
    Ok(())
}

/// `pigma msg` handler. `search` is a request/response command (the daemon
/// returns matching songs and registers them for a later `pigma msg play <id>`);
/// everything else is a fire-and-forget control action.
pub async fn msg(
    action: &str,
    value: Option<&str>,
    playlist: Option<usize>,
) -> color_eyre::Result<()> {
    if action == "search" {
        let keyword = value.ok_or_else(|| {
            color_eyre::eyre::eyre!("search requires a keyword (e.g. `pigma msg search 周杰伦`)")
        })?;
        let results = ipc::search_songs(keyword).await?;
        if results.is_empty() {
            println!("没有找到与「{keyword}」相关的歌曲");
            return Ok(());
        }
        for entry in &results {
            println!(
                "{:<10} {:<20} {} - {}",
                entry.source, entry.id, entry.name, entry.singer
            );
        }
        return Ok(());
    }
    let action = parse_msg_action(action, value, playlist)?;
    ipc::send_msg(action).await?;
    Ok(())
}

fn parse_msg_action(
    action: &str,
    value: Option<&str>,
    playlist: Option<usize>,
) -> color_eyre::Result<MsgAction> {
    match action {
        "previous" | "prev" => Ok(MsgAction::Previous),
        "next" => Ok(MsgAction::Next),
        "pause" => Ok(MsgAction::Pause),
        "play" => {
            let song_id = match value {
                None => None,
                Some(v) => Some(v.parse().map_err(|_| {
                    color_eyre::eyre::eyre!(
                        "invalid song id `{v}` (expected a number, or omit to play/resume)"
                    )
                })?),
            };
            Ok(MsgAction::Play { song_id })
        }
        "toggle_play" | "play_pause" | "toggle-play" | "play-pause" => Ok(MsgAction::TogglePlay),
        "switch-list" | "list" | "switch" => {
            let endpoint = value.ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "switch-list requires an endpoint (e.g. `pigma msg switch-list toplist`)"
                )
            })?;
            Ok(MsgAction::SwitchList {
                endpoint: endpoint.to_string(),
                playlist,
            })
        }
        "volume" => {
            let value = value.ok_or_else(|| {
                color_eyre::eyre::eyre!("volume requires a value like `75`, `+5` or `-5`")
            })?;
            parse_volume(value)
        }
        "mode" => Ok(MsgAction::Mode),
        "like" => Ok(MsgAction::Like),
        "dislike" => Ok(MsgAction::Dislike),
        "toggle_like" | "unlike" | "toggle" => Ok(MsgAction::ToggleLike),
        other => Err(color_eyre::eyre::eyre!(
            "unknown action `{other}` (expected previous/next/pause/play/switch-list/volume/mode/like/dislike/toggle_like)"
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
        let signed = if value.starts_with('-') {
            -number
        } else {
            number
        };
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

pub async fn run_cli(mut cli: Cli) -> color_eyre::Result<Option<App>> {
    let socket = match &cli.command {
        Some(Command::Status {
            socket: cmd_socket, ..
        })
        | Some(Command::Msg {
            socket: cmd_socket, ..
        }) => match cmd_socket {
            Some(s) => Some(s.clone()),
            None => cli.socket.take(),
        },
        _ => cli.socket.take(),
    };
    ipc::set_socket_path(socket);

    match &cli.command {
        Some(Command::Status {
            template,
            json,
            list,
            waybar,
            icon,
            watch,
            window,
            speed,
            ..
        }) => {
            cli::status(
                template,
                *json,
                *list,
                *waybar,
                icon.as_deref(),
                *watch,
                *window,
                *speed,
            )
            .await?;
            return Ok(None);
        }
        Some(Command::Msg {
            action,
            value,
            playlist,
            ..
        }) => {
            cli::msg(action, value.as_deref(), *playlist).await?;
            return Ok(None);
        }
        Some(Command::List { endpoint }) => {
            cli::list(endpoint).await?;
            return Ok(None);
        }
        None => {}
    }

    let config = Config::load();
    init_logger(&config)?;

    if let Some(endpoint) = cli.daemon {
        App::new(config, false)?
            .run_headless(&endpoint, cli.playlist)
            .await?;
        return Ok(None);
    }

    Ok(Some(App::new(config, true)?))
}

/* -------------------------------------------------------------------------- */
/*                                   Testing                                  */
/* -------------------------------------------------------------------------- */

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
        assert_eq!(out, "01:05/02:05 Example Artist Example Song 75% playing");
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

    #[test]
    fn waybar_main_playing() {
        // Window wide enough that the marquee stays static in this test.
        let out = waybar_main(&snapshot(), 40, 7);
        assert_eq!(out.text, "▶ Example Song — Example Artist");
        assert_eq!(out.class, vec!["playing"]);
        assert_eq!(out.alt, "playing");
        assert!(out.tooltip.contains("1:05 / 2:05"));
        assert!(out.tooltip.contains("75%"));
    }

    #[test]
    fn waybar_main_paused_and_stopped() {
        let mut s = snapshot();
        s.paused = true;
        let out = waybar_main(&s, 40, 7);
        assert_eq!(out.text, "⏸ Example Song — Example Artist");
        assert_eq!(out.class, vec!["paused"]);

        s.playing = false;
        let out = waybar_main(&s, 40, 7);
        assert_eq!(out.text, "■ Example Song — Example Artist");
        assert_eq!(out.class, vec!["stopped"]);
    }

    #[test]
    fn waybar_main_idle_when_no_song() {
        let out = waybar_main(&StatusSnapshot::default(), 20, 7);
        assert_eq!(out.text, "♪  idle");
        assert_eq!(out.class, vec!["idle"]);
    }

    #[test]
    fn waybar_icon_buttons() {
        let s = snapshot(); // liked = true, playing = true, paused = false
        let out = waybar_icon("like", &s);
        assert_eq!(out.text, "");
        assert_eq!(out.class, vec!["button", "liked"]);
        assert_eq!(waybar_icon("like", &StatusSnapshot::default()).text, "");

        assert_eq!(waybar_icon("play", &s).text, "⏸");
        assert_eq!(waybar_icon("play", &StatusSnapshot::default()).text, "▶");
        assert_eq!(waybar_icon("prev", &s).text, "");
        assert_eq!(waybar_icon("next", &s).text, "");
    }

    #[test]
    fn marquee_short_text_unchanged() {
        assert_eq!(marquee("short", 20, 7), "short");
    }

    #[test]
    fn marquee_window_width() {
        let out = marquee("abcdefghijklmnopqrstuvwxyz", 10, 7);
        assert_eq!(out.chars().count(), 10);
    }

    #[test]
    fn parse_msg_play_with_optional_id() {
        let plain = parse_msg_action("play", None, None).unwrap();
        assert_eq!(plain, MsgAction::Play { song_id: None });

        let with_id = parse_msg_action("play", Some("187186"), None).unwrap();
        assert_eq!(
            with_id,
            MsgAction::Play {
                song_id: Some(187186)
            }
        );

        assert!(parse_msg_action("play", Some("abc"), None).is_err());
        assert_eq!(
            parse_msg_action("toggle_play", None, None).unwrap(),
            MsgAction::TogglePlay
        );
    }

    #[test]
    fn fmt_min_sec_formats() {
        assert_eq!(fmt_min_sec(0), "0:00");
        assert_eq!(fmt_min_sec(65_000), "1:05");
        assert_eq!(fmt_min_sec(3_610_000), "60:10");
    }
}
