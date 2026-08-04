use musicx::{MusicFinder, MusicSource, SearchConfig, SearchQuery};

#[tokio::main]
fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    for src in [
        MusicSource::Kuwo,
        MusicSource::Kugou,
        MusicSource::BiliVideo,
        MusicSource::Youtube,
    ] {
        let finder = MusicFinder::new(
            SearchConfig::new()
                .with_providers(vec![src])
                .with_timeout(15000),
        );
        let result = finder.search(&SearchQuery::new("只有爱 许巍")).await;
        match result {
            Ok(r) => println!("{:?} -> {} 条", src, r.songs.len()),
            Err(e) => println!("{:?} -> ERR {e}", src),
        }
    }
}
