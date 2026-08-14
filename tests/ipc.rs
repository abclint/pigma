use std::sync::{Arc, Mutex};

use pigma::ipc::{self, IpcEvent, StatusSnapshot};

fn tmp_socket(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("pigma-ipc-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{tag}.sock"))
        .to_string_lossy()
        .into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn status_round_trip() {
    unsafe {
        std::env::set_var("PIGMA_SOCKET", tmp_socket("status"));
    }

    let mut snapshot = StatusSnapshot::default();
    snapshot.name = "Test Song".into();
    snapshot.artist = "Test Artist".into();
    snapshot.duration_ms = 100_000;
    snapshot.position_ms = 25_000;
    snapshot.volume = 0.5;
    snapshot.playing = true;
    let snapshot = Arc::new(Mutex::new(snapshot));

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let _guard = ipc::start_server(Arc::clone(&snapshot), tx);

    // Give the server a moment to bind.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let got = ipc::fetch_status().await.expect("fetch status");
    assert_eq!(got.name, "Test Song");
    assert_eq!(got.artist, "Test Artist");
    assert_eq!(got.duration_ms, 100_000);
    assert_eq!(got.position_ms, 25_000);
    assert_eq!(got.volume, 0.5);
    assert!(got.playing);
}

#[tokio::test(flavor = "multi_thread")]
async fn msg_round_trip_forwards_event() {
    unsafe {
        std::env::set_var("PIGMA_SOCKET", tmp_socket("msg"));
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let _guard = ipc::start_server(Arc::new(Mutex::new(StatusSnapshot::default())), tx);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    ipc::send_msg(ipc::MsgAction::Pause)
        .await
        .expect("send msg");

    let event = rx.recv().await.expect("receive event");
    match event {
        pigma::event::Event::App(pigma::event::AppEvent::Ipc(IpcEvent::Pause)) => {}
        other => panic!("expected Ipc(Pause), got {other:?}"),
    }
}
