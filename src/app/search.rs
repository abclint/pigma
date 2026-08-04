use super::{App, send_event};
use crate::event::NavigationEvent;
use crate::state::{ContentState, SearchProvider};

impl App {
    pub(super) fn handle_search_song(&mut self, keyword: String) {
        match self.state.navigation.search.provider {
            SearchProvider::Ncm => self.search_ncm(keyword),
            provider => self.search_musicx(keyword, provider),
        }
    }

    fn search_ncm(&mut self, keyword: String) {
        self.state.navigation.set_content(ContentState::Loading);
        self.state.navigation.content_is_search = true;
        self.state.navigation.nav.subtitle = Some(format!("搜索: {keyword}"));
        self.state.navigation.content_selected = 0;
        let service = self.service.clone();
        let sender = self.state.events.sender();
        let limit = self.config.search_limit;
        tokio::spawn(async move {
            let state = service.search_songs(&keyword, limit).await;
            send_event(&sender, NavigationEvent::ContentLoaded(state).into());
        });
    }

    fn search_musicx(&mut self, keyword: String, provider: SearchProvider) {
        self.state.navigation.set_content(ContentState::Loading);
        self.state.navigation.content_is_search = true;
        self.state.navigation.nav.subtitle =
            Some(format!("({}): {keyword}", provider.display_name()));
        self.state.navigation.content_selected = 0;
        let source = provider.to_musicx().expect("musicx provider");
        let sender = self.state.events.sender();
        let registry = self.musicx_songs.clone();
        let limit = self.config.search_limit as usize;
        tokio::spawn(async move {
            let config = musicx::SearchConfig::new()
                .with_providers(vec![source])
                .with_timeout(15000);
            let finder = musicx::MusicFinder::new(config);
            let state = match finder.search(&musicx::SearchQuery::new(&keyword)).await {
                Ok(result) => {
                    let songs: Vec<ncm_api::SongInfo> = result
                        .songs
                        .into_iter()
                        .take(limit)
                        .map(|song| {
                            let info = to_song_info(source, &song);
                            if let Ok(mut map) = registry.lock() {
                                map.insert(info.id, std::sync::Arc::new(song));
                            }
                            info
                        })
                        .collect();
                    if songs.is_empty() {
                        ContentState::Error("没有找到结果".into())
                    } else {
                        ContentState::Songs(songs)
                    }
                }
                Err(e) => ContentState::Error(format!("搜索失败: {e}")),
            };
            send_event(&sender, NavigationEvent::ContentLoaded(state).into());
        });
    }

    pub(super) fn handle_search_activate(&mut self) {
        let nav = &mut self.state.navigation;
        nav.search.active = true;
        nav.search.input = crate::text_input::TextInput::new();
        nav.search.filter_queue_only = false;
        nav.search.unfiltered_songs = None;
        nav.search.provider = SearchProvider::Ncm;

        nav.push_breadcrumb();

        nav.nav.subtitle = None;
        nav.content_selected = 0;

        nav.nav.restore_focus_by_api("search");

        nav.set_content(ContentState::Empty);

        let api = nav
            .nav
            .section_states
            .get(nav.nav.focus_section)
            .and_then(|st| st.selected())
            .and_then(|i| nav.nav.sections.get(nav.nav.focus_section)?.items.get(i))
            .and_then(|item| item.api.as_ref());
        if let Some(api) = api {
            let sender = self.state.events.sender();
            send_event(&sender, NavigationEvent::NavSelect(api.clone()).into());
        }
    }

    pub(super) fn handle_search_deactivate(&mut self) {
        let nav = &mut self.state.navigation;
        if nav.search.filter_queue_only {
            nav.search.filter_queue_only = false;
            if let Some(songs) = nav.search.unfiltered_songs.take() {
                self.playback.set_queue_songs(songs);
            }
        } else {
            nav.pop_breadcrumb();
        }
        nav.search.active = false;
        nav.search.input = crate::text_input::TextInput::new();
        nav.nav.subtitle = None;
    }

    pub(super) fn handle_content_restore(&mut self) {
        let nav = &mut self.state.navigation;
        nav.pop_breadcrumb();
    }
}

/// Map a musicx search result onto the app's `SongInfo`, using a flagged
/// synthetic id so playback routes through the musicx fallback (see
/// `crate::utils::musicx`).
fn to_song_info(source: musicx::MusicSource, song: &musicx::Song) -> ncm_api::SongInfo {
    let singer = song
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(" / ");
    ncm_api::SongInfo {
        id: crate::utils::musicx::make_song_id(source, &song.id),
        name: song.name.clone(),
        singer,
        artist_id: 0,
        album: song
            .album
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_default(),
        album_id: 0,
        pic_url: song.pic_url.clone(),
        duration: song.duration,
        copyright: ncm_api::SongCopyright::Free,
    }
}
