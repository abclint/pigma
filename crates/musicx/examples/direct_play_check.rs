use musicx::{MusicFinder, MusicSource, SearchConfig, SearchQuery};

#[tokio::main]
fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let finder = MusicFinder::new(
        SearchConfig::new()
            .with_providers(vec![MusicSource::Kuwo])
            .with_timeout(15000),
    );
    let result = finder
        .search(&SearchQuery::new("只有爱 许巍"))
        .await
        .unwrap();
    for song in result.songs.iter().take(3) {
        match finder
            .get_play_url_for_song(song, Some(musicx::Quality::High))
            .await
        {
            Ok(play) => println!("[{song:?}] url={}", play.url),
            Err(e) => println!("[{song:?}] ERR {e}"),
        }
    }
}
