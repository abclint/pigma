use std::sync::Arc;

use crate::api::ApiEndpoint;
use crate::cache::CacheManager;
use crate::state::{ContentState, PaginationInfo};

/// Centralized API service that handles endpoint resolution, caching, and error mapping.
///
/// Callers are still responsible for `tokio::spawn` + `send_event`.
#[derive(Clone)]
pub struct ApiService {
    client: Arc<ncm_api::NcmClient>,
    cache: CacheManager,
}

impl ApiService {
    pub fn new(client: Arc<ncm_api::NcmClient>, cache: CacheManager) -> Self {
        Self { client, cache }
    }

    pub fn client(&self) -> &Arc<ncm_api::NcmClient> {
        &self.client
    }

    pub fn cache(&self) -> &CacheManager {
        &self.cache
    }

    /// Resolve a navigation endpoint into a `ContentState`.
    ///
    /// Handles the API call, maps the result to `ContentState`, and logs errors.
    /// Does NOT handle caching — callers check/save cache before/after this call.
    pub async fn resolve_content(
        &self,
        api: ApiEndpoint,
        uid: Option<u64>,
        limit: u16,
    ) -> (ContentState, Option<PaginationInfo>) {
        match api {
            ApiEndpoint::RecommendSongs => match self.client.recommend_songs().await {
                Ok(songs) => (ContentState::Songs(songs), None),
                Err(e) => (ContentState::Error(e.to_string()), None),
            },
            ApiEndpoint::RecommendResource => match self.client.recommend_resource().await {
                Ok(lists) => (ContentState::SongLists(lists), None),
                Err(e) => (ContentState::Error(e.to_string()), None),
            },
            ApiEndpoint::Toplist => match self.client.toplist().await {
                Ok(lists) => (ContentState::TopLists(lists), None),
                Err(e) => (ContentState::Error(e.to_string()), None),
            },
            ApiEndpoint::TopSongList => {
                match self.client.top_song_list("全部", "hot", 0, limit).await {
                    Ok(lists) => (ContentState::SongLists(lists), None),
                    Err(e) => (ContentState::Error(e.to_string()), None),
                }
            }
            ApiEndpoint::UserRadioSublist => match self.client.user_radio_sublist(0, limit).await {
                Ok(lists) => (ContentState::SongLists(lists), None),
                Err(e) => (ContentState::Error(e.to_string()), None),
            },
            ApiEndpoint::UserCloudDisk => {
                match self.client.user_cloud_disk(0, limit as u32).await {
                    Ok(result) => (
                        ContentState::Songs(result.songs),
                        Some(PaginationInfo {
                            api: "user_cloud_disk".into(),
                            offset: 0,
                            limit: limit as u32,
                            has_more: result.has_more,
                            total: result.count,
                            loading: false,
                        }),
                    ),
                    Err(e) => (ContentState::Error(e.to_string()), None),
                }
            }
            ApiEndpoint::Recent => match self.client.recent_songs(limit).await {
                Ok(songs) => (ContentState::Songs(songs), None),
                Err(e) => (ContentState::Error(e.to_string()), None),
            },
            ApiEndpoint::UserSongList => match uid {
                Some(uid) => match self.client.user_song_list(uid, 0, limit).await {
                    Ok(lists) => (ContentState::SongLists(lists), None),
                    Err(e) => (ContentState::Error(e.to_string()), None),
                },
                None => (ContentState::Error("未登录".into()), None),
            },
            ApiEndpoint::UserCreatedSongList => match uid {
                Some(uid) => match self.client.user_created_playlist(uid, 0, limit).await {
                    Ok(lists) => (ContentState::SongLists(lists), None),
                    Err(e) => (ContentState::Error(e.to_string()), None),
                },
                None => (ContentState::Error("未登录".into()), None),
            },
            ApiEndpoint::UserSubscribedSongList => match uid {
                Some(uid) => match self.client.user_collected_playlist(uid, 0, limit).await {
                    Ok(lists) => (ContentState::SongLists(lists), None),
                    Err(e) => (ContentState::Error(e.to_string()), None),
                },
                None => (ContentState::Error("未登录".into()), None),
            },
            ApiEndpoint::Search => match self.client.search_hot().await {
                Ok(items) => (
                    ContentState::HotSearch(items.into_iter().map(|h| h.keyword).collect()),
                    None,
                ),
                Err(e) => (ContentState::Error(e.to_string()), None),
            },
            ApiEndpoint::TopSingers => match self.client.top_artists(0, limit).await {
                Ok(singers) => (ContentState::Singers(singers), None),
                Err(e) => (ContentState::Error(e.to_string()), None),
            },
            ApiEndpoint::Download | ApiEndpoint::LocalMusic | ApiEndpoint::LikedSongs => {
                unreachable!("handled separately by caller")
            }
        }
    }

    /// Load liked songs and the user's liked playlist ID (for heartbeat mode).
    pub async fn load_liked_songs(&self, uid: u64, limit: u16) -> (ContentState, Option<u64>) {
        match self.client.liked_songs(uid).await {
            Ok(songs) => {
                let playlist_id = self
                    .client
                    .user_song_list(uid, 0, limit)
                    .await
                    .ok()
                    .and_then(|lists| {
                        lists
                            .iter()
                            .find(|l| l.name == "我喜欢的音乐")
                            .map(|l| l.id)
                    });
                (ContentState::Songs(songs), playlist_id)
            }
            Err(e) => (ContentState::Error(e.to_string()), None),
        }
    }

    /// Load lyrics for a song, with cache integration.
    pub async fn load_lyrics(&self, song_id: u64) -> Option<ncm_api::Lyrics> {
        if let Some(cached) = self.cache.load_lyrics_cache_async(song_id).await {
            return Some(cached);
        }
        match self.client.song_lyric(song_id).await {
            Ok(lyrics) => {
                let cache = self.cache.clone();
                let lyrics_clone = lyrics.clone();
                tokio::task::spawn_blocking(move || {
                    cache.save_lyrics_cache(song_id, &lyrics_clone);
                })
                .await
                .ok();
                Some(lyrics)
            }
            Err(e) => {
                log::error!("Failed to fetch lyrics for {song_id}: {e}");
                None
            }
        }
    }

    /// Search songs by keyword.
    pub async fn search_songs(&self, keyword: &str, limit: u16) -> ContentState {
        match self.client.search_song(keyword, 0, limit).await {
            Ok(result) => ContentState::Songs(result.songs),
            Err(e) => ContentState::Error(e.to_string()),
        }
    }

    /// Load playlist detail (regular or radio).
    pub async fn load_playlist_detail(
        &self,
        id: u64,
        is_radio: bool,
    ) -> (ContentState, Option<String>) {
        if is_radio {
            match self.client.radio_program(id, 0, 1000).await {
                Ok(songs) => (ContentState::Songs(songs), None),
                Err(e) => (ContentState::Error(e.to_string()), None),
            }
        } else {
            match self.client.song_list_detail(id).await {
                Ok(detail) => (ContentState::Songs(detail.songs), Some(detail.name)),
                Err(e) => (ContentState::Error(e.to_string()), None),
            }
        }
    }

    /// Load album songs.
    pub async fn load_album(&self, album_id: u64) -> ContentState {
        match self.client.album(album_id).await {
            Ok(detail) => ContentState::Songs(detail.songs),
            Err(e) => ContentState::Error(e.to_string()),
        }
    }

    /// Load artist songs.
    pub async fn load_artist_songs(&self, artist_id: u64) -> ContentState {
        match self.client.singer_songs(artist_id).await {
            Ok(songs) => ContentState::Songs(songs),
            Err(e) => ContentState::Error(e.to_string()),
        }
    }

    /// Load more cloud disk songs (pagination).
    pub async fn load_more_cloud_disk(
        &self,
        offset: u32,
        limit: u32,
        api_str: String,
    ) -> Option<(ContentState, PaginationInfo)> {
        match self.client.user_cloud_disk(offset, limit).await {
            Ok(result) => Some((
                ContentState::Songs(result.songs),
                PaginationInfo {
                    api: api_str,
                    offset,
                    limit,
                    has_more: result.has_more,
                    total: result.count,
                    loading: false,
                },
            )),
            Err(e) => {
                log::error!("Failed to load more cloud disk songs: {e}");
                None
            }
        }
    }
}
