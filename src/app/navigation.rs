use std::future::Future;
use std::sync::Arc;

use super::{App, send_event};
use crate::api::ApiEndpoint;
use crate::event::NavigationEvent;
use crate::state::ContentState;

impl App {
    pub(super) fn handle_nav_select(&mut self, api_str: String) -> color_eyre::Result<()> {
        let api = match ApiEndpoint::parse(&api_str) {
            Some(ep) => ep,
            None => {
                self.state
                    .navigation
                    .set_content(ContentState::Error(format!("未知: {api_str}")));
                return Ok(());
            }
        };

        if api == ApiEndpoint::LocalMusic {
            self.state
                .navigation
                .set_content(self.state.local_music.clone());
            self.state.navigation.content_selected = 0;
            return Ok(());
        }

        self.state.navigation.clear_breadcrumb();
        self.state.navigation.set_content(ContentState::Loading);
        self.state.navigation.nav.subtitle = None;
        if api == ApiEndpoint::Search {
            self.state.navigation.nav.subtitle = Some("热搜榜".into());
        }
        let cache = self.service.cache().clone();
        let service = self.service.clone();
        let sender = self.state.events.sender();
        let uid = self.state.navigation.user.as_ref().map(|u| u.uid);
        let ttl = self.config.content_cache_ttl;
        let limit = self.config.search_limit;

        tokio::spawn(async move {
            if api == ApiEndpoint::Download {
                let songs = cache.list_cached_songs_async().await;
                send_event(
                    &sender,
                    NavigationEvent::ContentLoaded(ContentState::Songs(songs)).into(),
                );
                return;
            }

            if ttl > 0
                && api != ApiEndpoint::Search
                && let Some(cached) = cache.load_content_cache_async(&api_str, ttl).await
            {
                send_event(&sender, NavigationEvent::ContentLoaded(cached).into());
                return;
            }

            // Handle LikedSongs separately: also fetch playlist ID for heartbeat mode
            if api == ApiEndpoint::LikedSongs
                && let Some(uid) = uid
            {
                let (state, playlist_id) = service.load_liked_songs(uid, limit).await;
                send_event(&sender, NavigationEvent::ContentLoaded(state).into());
                if let Some(id) = playlist_id {
                    send_event(
                        &sender,
                        crate::event::PlaybackEvent::SetPlaylistId(id).into(),
                    );
                }
                return;
            }

            let (state, pagination) = service.resolve_content(api, uid, limit).await;

            if ttl > 0 && api != ApiEndpoint::Search {
                let cache_clone = cache.clone();
                let api_str_clone = api_str.clone();
                let state_clone = state.clone();
                tokio::task::spawn_blocking(move || {
                    cache_clone.save_content_cache(&api_str_clone, state_clone);
                })
                .await
                .ok();
            }

            if let Some(pg) = pagination {
                send_event(
                    &sender,
                    NavigationEvent::ContentLoadedPaged {
                        content: state,
                        pagination: pg,
                    }
                    .into(),
                );
            } else {
                send_event(&sender, NavigationEvent::ContentLoaded(state).into());
            }
        });
        Ok(())
    }

    pub(super) fn handle_breadcrumb(&mut self, name: String) {
        self.state.navigation.nav.subtitle = Some(name);
    }

    pub(super) fn navigate_to_entity<F, Fut>(&mut self, name: String, api_call: F)
    where
        F: FnOnce(Arc<ncm_api::NcmClient>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Vec<ncm_api::SongInfo>, ncm_api::NcmError>> + Send,
    {
        self.state.navigation.push_breadcrumb();
        self.state.navigation.set_content(ContentState::Loading);

        let client = self.service.client().clone();
        let sender = self.state.events.sender();
        tokio::spawn(async move {
            let result = api_call(client).await;
            let state = match result {
                Ok(songs) => ContentState::Songs(songs),
                Err(e) => ContentState::Error(e.to_string()),
            };
            let _ = sender.send(NavigationEvent::ContentLoaded(state).into());
            let _ = sender.send(NavigationEvent::BreadcrumbSet(name).into());
        });
    }

    pub(super) fn handle_cell_action(&mut self, row: usize, col: usize) -> color_eyre::Result<()> {
        let columns = self
            .config
            .columns
            .for_content(self.state.navigation.content.content_type(), None)
            .to_vec();
        let column = match columns.get(col) {
            Some(c) => c.clone(),
            None => return Ok(()),
        };
        let field = column.field.clone();

        match (self.state.navigation.content.as_ref(), field.as_str()) {
            (ContentState::Songs(songs), "album") => {
                if let Some(song) = songs.get(row) {
                    let album_id = song.album_id;
                    let name = format!("{}: {}", column.header, song.album);
                    self.navigate_to_entity(name, move |client| async move {
                        client.album(album_id).await.map(|d| d.songs)
                    });
                }
            }
            (ContentState::Songs(songs), "singer") => {
                if let Some(song) = songs.get(row) {
                    let artist_id = song.artist_id;
                    if artist_id == 0 {
                        return Ok(());
                    }
                    let name = format!("{}: {}", column.header, song.singer);
                    self.navigate_to_entity(name, move |client| async move {
                        client.singer_songs(artist_id).await
                    });
                }
            }
            (ContentState::Singers(singers), "name") => {
                if let Some(singer) = singers.get(row) {
                    let artist_id = singer.id;
                    if artist_id == 0 {
                        return Ok(());
                    }
                    let name = format!("{}: {}", column.header, singer.name);
                    self.navigate_to_entity(name, move |client| async move {
                        client.singer_songs(artist_id).await
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }
}
