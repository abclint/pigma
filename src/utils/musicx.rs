use std::hash::{Hash, Hasher};

use musicx::MusicSource;

const MUSICX_ID_FLAG: u64 = 1 << 63;

/// Build a stable synthetic id for a musicx song. Deterministic across runs so
/// cache entries and saved sessions survive restarts.
pub fn make_song_id(source: MusicSource, id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{source}:{id}").hash(&mut hasher);
    hasher.finish() | MUSICX_ID_FLAG
}

/// Whether a song id belongs to a musicx search result rather than NCM.
pub fn is_musicx_song_id(id: u64) -> bool {
    id & MUSICX_ID_FLAG != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_musicx_ids() {
        let id = make_song_id(MusicSource::Kuwo, "60478382");
        assert!(is_musicx_song_id(id));
    }

    #[test]
    fn does_not_flag_ncm_ids() {
        assert!(!is_musicx_song_id(186150));
        assert!(!is_musicx_song_id(u64::MAX & !MUSICX_ID_FLAG));
    }

    #[test]
    fn ids_are_stable() {
        let a = make_song_id(MusicSource::Kugou, "abc123");
        let b = make_song_id(MusicSource::Kugou, "abc123");
        assert_eq!(a, b);
        assert_ne!(a, make_song_id(MusicSource::Kuwo, "abc123"));
        assert_ne!(a, make_song_id(MusicSource::Kugou, "abc124"));
    }
}
