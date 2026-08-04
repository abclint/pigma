use musicx::{MusicFinder, MusicSource, SearchConfig, SearchQuery};

#[tokio::main]
fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let finder = MusicFinder::new(SearchConfig::default().with_timeout(15000));

    for src in [
        MusicSource::Kugou,
        MusicSource::Kuwo,
        MusicSource::BiliVideo,
        MusicSource::Youtube,
    ] {
        let result = finder
            .search(&SearchQuery::new("晴天 周杰伦"))
            .await
            .unwrap();
        let song = result.songs.iter().find(|s| s.source == src);
        let Some(song) = song else { continue };
        let lyrics = finder.get_lyrics_fallback(song).await;
        let cover = finder.get_cover_fallback(song).await;
        println!(
            "[{:?}] {} - {}  lyrics={}  cover={}",
            src,
            song.name,
            song.artists[0].name,
            lyrics.as_ref().map(|l| l.lines().count()).unwrap_or(0),
            cover.as_ref().map(|c| c.len()).unwrap_or(0)
        );
    }
}
