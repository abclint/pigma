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
        .search(&SearchQuery::new("晴天 周杰伦"))
        .await
        .unwrap();
    for (i, song) in result.songs.iter().enumerate() {
        let lrc = finder.get_lyrics(song).await.ok().flatten();
        println!(
            "#{i} [{:?}] {} - {} pic={} lyrics={}",
            song.source,
            song.name,
            song.artists[0].name,
            !song.pic_url.is_empty(),
            lrc.as_ref().map(|l| l.lines().count()).unwrap_or(0)
        );
    }
}
