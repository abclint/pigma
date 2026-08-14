//! Unix-domain-socket IPC between the running TUI and the `pigma status` /
//! `pigma msg` CLI commands.
//!
//! The TUI binds a socket at `~/.cache/pigma/pigma.sock` and accepts one-line
//! JSON requests:
//!
//! - `{"cmd":"status"}` → the server replies with a serialized `StatusSnapshot`.
//! - `{"cmd":"msg","action":...}` → the server forwards an `IpcEvent` into the
//!   app's event channel and replies `{"ok":true}`.
//!
//! The socket file is user-scoped (`pigma_cache_dir`) so no authentication is
//! needed.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use color_eyre::eyre::{OptionExt, WrapErr};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use crate::event::{AppEvent, Event};
use crate::playback::PlayMode;
use crate::utils::pigma_cache_dir;

/// Socket file name inside `pigma_cache_dir()`.
pub const SOCKET_FILE: &str = "pigma.sock";

/// Request sent from the CLI to the running TUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcRequest {
    Status,
    Msg { action: MsgAction },
}

/// A playback control action for `pigma msg`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MsgAction {
    Previous,
    Next,
    Pause,
    Play,
    /// Exactly one of `delta` / `absolute` is set:
    /// - `delta`: fraction of 0..=1 to add/subtract (mirrors the TUI's `+`/`-`).
    /// - `absolute`: target fraction of 0..=1.
    Volume { delta: Option<f64>, absolute: Option<f64> },
    Mode,
    Like,
    Dislike,
}

/// Runtime event dispatched to the app loop for a `msg` action.
#[derive(Debug, Clone)]
pub enum IpcEvent {
    Previous,
    Next,
    Pause,
    Play,
    Volume { delta: Option<f64>, absolute: Option<f64> },
    Mode,
    Like,
    Dislike,
}

impl From<MsgAction> for IpcEvent {
    fn from(action: MsgAction) -> Self {
        match action {
            MsgAction::Previous => IpcEvent::Previous,
            MsgAction::Next => IpcEvent::Next,
            MsgAction::Pause => IpcEvent::Pause,
            MsgAction::Play => IpcEvent::Play,
            MsgAction::Volume { delta, absolute } => IpcEvent::Volume { delta, absolute },
            MsgAction::Mode => IpcEvent::Mode,
            MsgAction::Like => IpcEvent::Like,
            MsgAction::Dislike => IpcEvent::Dislike,
        }
    }
}

/// Live playback state snapshot served to `pigma status`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub id: u64,
    pub name: String,
    pub artist: String,
    pub album: String,
    /// Total length in milliseconds (0 when nothing is loaded).
    pub duration_ms: u64,
    /// Playback position in milliseconds.
    pub position_ms: u64,
    /// Volume as a fraction of 0..=1.
    pub volume: f64,
    pub playing: bool,
    pub paused: bool,
    /// Stable play-mode key: `sequential` / `repeat_one` / `repeat_all` /
    /// `shuffle` / `heartbeat`.
    pub mode: String,
    pub liked: bool,
}

impl StatusSnapshot {
    pub fn from_playback(state: &crate::playback::PlaybackState) -> Self {
        let song = state.current_song.as_ref();
        let duration_ms = song.map(|s| s.duration).unwrap_or(0);
        let position_ms = song
            .map(|s| (state.progress * s.duration as f64) as u64)
            .unwrap_or(0);
        Self {
            id: song.map(|s| s.id).unwrap_or(0),
            name: song.map(|s| s.name.clone()).unwrap_or_default(),
            artist: song.map(|s| s.singer.clone()).unwrap_or_default(),
            album: song.map(|s| s.album.clone()).unwrap_or_default(),
            duration_ms,
            position_ms,
            volume: state.volume,
            playing: state.playing,
            paused: state.paused,
            mode: mode_key(&state.mode).to_string(),
            liked: state.liked,
        }
    }
}

fn mode_key(mode: &PlayMode) -> &'static str {
    match mode {
        PlayMode::Sequential => "sequential",
        PlayMode::RepeatOne => "repeat_one",
        PlayMode::RepeatAll => "repeat_all",
        PlayMode::Shuffle => "shuffle",
        PlayMode::Heartbeat { .. } => "heartbeat",
    }
}

fn socket_path() -> PathBuf {
    pigma_cache_dir().join(SOCKET_FILE)
}

/// Resolve the socket path, honoring `PIGMA_SOCKET` for tests/tools that need a
/// non-default location (e.g. one socket per integration test).
fn resolve_socket_path() -> PathBuf {
    match std::env::var("PIGMA_SOCKET") {
        Ok(path) if !path.is_empty() => PathBuf::from(path),
        _ => socket_path(),
    }
}

/// Bind the socket, clearing any stale file left by a previous run. Returns the
/// listener, or `None` when another pigma instance already holds the socket.
fn bind() -> Option<UnixListener> {
    let path = resolve_socket_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match UnixListener::bind(&path) {
        Ok(listener) => Some(listener),
        Err(_) => {
            // Either a live instance owns the socket or it is stale.
            // A non-blocking connect probe tells us which: if we can connect,
            // another instance is running and we must not steal the socket.
            if std::os::unix::net::UnixStream::connect(&path).is_ok() {
                log::warn!("ipc: another pigma instance already owns {}", path.display());
                return None;
            }
            let _ = fs::remove_file(&path);
            UnixListener::bind(&path).ok()
        }
    }
}

/// Start the IPC server for the running TUI.
///
/// Spawns a background task that accepts connections, answering `status`
/// requests from `status_snapshot` and forwarding `msg` requests as `IpcEvent`s
/// into `event_tx`. Returns a guard that removes the socket file on drop.
pub fn start_server(
    status_snapshot: Arc<Mutex<StatusSnapshot>>,
    event_tx: mpsc::UnboundedSender<Event>,
) -> IpcServerGuard {
    let listener = match bind() {
        Some(l) => l,
        None => return IpcServerGuard::new(false),
    };
    let path = resolve_socket_path();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let snapshot = Arc::clone(&status_snapshot);
                    let tx = event_tx.clone();
                    tokio::spawn(async move {
                        handle_connection(stream, snapshot, tx).await;
                    });
                }
                Err(e) => {
                    log::error!("ipc: accept failed: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    });
    log::info!("ipc: listening on {}", path.display());
    IpcServerGuard::new(true)
}

/// Removes the socket file on drop (clean shutdown of the TUI).
pub struct IpcServerGuard {
    remove_on_drop: bool,
}

impl IpcServerGuard {
    fn new(remove_on_drop: bool) -> Self {
        Self { remove_on_drop }
    }
}

impl Drop for IpcServerGuard {
    fn drop(&mut self) {
        if self.remove_on_drop {
            let _ = fs::remove_file(resolve_socket_path());
        }
    }
}

/// Remove the socket file unconditionally (used on shutdown paths where the
/// guard may already be dropped).
pub fn remove_socket() {
    let _ = fs::remove_file(resolve_socket_path());
}

async fn handle_connection(
    stream: UnixStream,
    snapshot: Arc<Mutex<StatusSnapshot>>,
    event_tx: mpsc::UnboundedSender<Event>,
) {
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    if stream.read_line(&mut line).await.is_err() {
        return;
    }
    let request: IpcRequest = match serde_json::from_str(&line) {
        Ok(req) => req,
        Err(e) => {
            log::debug!("ipc: invalid request: {e}");
            return;
        }
    };
    let reply = match request {
        IpcRequest::Status => {
            let guard = snapshot.lock().unwrap();
            serde_json::to_string(&*guard).unwrap_or_default()
        }
        IpcRequest::Msg { action } => {
            let event: IpcEvent = action.into();
            let sent = event_tx.send(Event::App(AppEvent::Ipc(event)));
            if sent.is_err() {
                log::error!("ipc: failed to forward msg event: receiver dropped");
            }
            r#"{"ok":true}"#.to_string()
        }
    };
    let mut framed = reply;
    framed.push('\n');
    let mut stream = stream.into_inner();
    let _ = stream.write_all(framed.as_bytes()).await;
}

/// Connect to the running TUI's socket, returning a descriptive error when no
/// instance is up.
async fn connect() -> color_eyre::Result<UnixStream> {
    let path = resolve_socket_path();
    UnixStream::connect(&path)
        .await
        .wrap_err("pigma is not running (socket not found)")
}

/// Send a `status` request and return the live snapshot.
pub async fn fetch_status() -> color_eyre::Result<StatusSnapshot> {
    let mut stream = connect().await?;
    stream
        .write_all(br#"{"cmd":"status"}"#)
        .await
        .wrap_err("failed to send status request")?;
    stream.write_all(b"\n").await?;
    let mut buf = String::new();
    let mut reader = BufReader::new(stream);
    reader
        .read_line(&mut buf)
        .await
        .wrap_err("failed to read status response")?;
    serde_json::from_str(&buf).wrap_err("invalid status response")
}

/// Send a `msg` action to the running TUI. Returns once the server confirms.
pub async fn send_msg(action: MsgAction) -> color_eyre::Result<()> {
    let mut stream = connect().await?;
    let request = serde_json::to_string(&IpcRequest::Msg { action })
        .wrap_err("failed to serialize msg request")?;
    stream
        .write_all(request.as_bytes())
        .await
        .wrap_err("failed to send msg request")?;
    stream.write_all(b"\n").await?;
    let mut buf = String::new();
    let mut reader = BufReader::new(stream);
    reader
        .read_line(&mut buf)
        .await
        .wrap_err("failed to read msg response")?;
    serde_json::from_str::<serde_json::Value>(&buf)
        .ok()
        .and_then(|v| v.get("ok").and_then(|b| b.as_bool()))
        .ok_or_eyre("invalid msg response")
        .map(|_| ())
}
