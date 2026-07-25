use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use ncm_api::SongInfo;
use serde::{Deserialize, Deserializer, Serialize};
use stream_download::storage::StorageProvider;

use crate::state::ContentState;

#[derive(Serialize, Deserialize)]
struct ContentCacheEntry {
    data: ContentState,
    cached_at: u64,
}

/// Entry in the audio cache index, mapping song ID to filename and duration.
#[derive(Clone, Serialize, Deserialize)]
struct CacheEntry {
    filename: String,
    #[serde(default)]
    duration: u64,
}

/// Backward-compatible deserializer: accepts both the old format (plain string)
/// and the new format (object with filename + duration).
fn deserialize_cache_entry<'de, D>(deserializer: D) -> Result<CacheEntry, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Str(String),
        Obj(CacheEntry),
    }

    match Raw::deserialize(deserializer)? {
        Raw::Str(filename) => Ok(CacheEntry {
            filename,
            duration: 0,
        }),
        Raw::Obj(entry) => Ok(entry),
    }
}

type CacheIndex = HashMap<u64, CacheEntryWrapper>;

#[derive(Clone, Serialize)]
struct CacheEntryWrapper {
    filename: String,
    duration: u64,
    #[serde(default)]
    accessed_at: u64,
}

impl<'de> Deserialize<'de> for CacheEntryWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entry = deserialize_cache_entry(deserializer)?;
        Ok(Self {
            filename: entry.filename,
            duration: entry.duration,
            accessed_at: 0,
        })
    }
}

/// Default maximum cache size in bytes (2 GB).
const DEFAULT_MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone)]
/// Manages content, lyrics, and audio caches on disk.
pub struct CacheManager {
    downloads_dir: PathBuf,
    lyrics_dir: PathBuf,
    content_dir: PathBuf,
    template: String,
    index: Arc<RwLock<CacheIndex>>,
    max_cache_bytes: u64,
}

impl CacheManager {
    pub fn new(downloads_dir: PathBuf, base_dir: PathBuf, template: String) -> Self {
        let lyrics_dir = base_dir.join("lyrics");
        let content_dir = base_dir.join("content");
        let index = Self::load_index_static(&downloads_dir);
        Self {
            downloads_dir,
            lyrics_dir,
            content_dir,
            template,
            index: Arc::new(RwLock::new(index)),
            max_cache_bytes: DEFAULT_MAX_CACHE_BYTES,
        }
    }

    fn index_path(dir: &Path) -> PathBuf {
        dir.join("cache_index.json")
    }

    fn load_index_static(dir: &Path) -> CacheIndex {
        let path = Self::index_path(dir);
        if !path.exists() {
            return HashMap::new();
        }
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Snapshot the index under a read lock, then serialize and write to disk
    /// without holding any lock.
    fn save_index(&self) {
        let snapshot = {
            let index = self.index.read().unwrap_or_else(|e| e.into_inner());
            serde_json::to_string_pretty(&*index).unwrap_or_default()
        };
        let path = Self::index_path(&self.downloads_dir);
        if let Err(e) = fs::write(&path, snapshot) {
            log::warn!("Failed to write cache index: {e}");
        }
    }

    pub fn remove_from_index(&self, song_id: u64) {
        self.index
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&song_id);
    }

    /// Persist the in-memory cache index to disk.
    pub fn flush_index(&self) {
        self.save_index();
    }

    fn sanitize_filename(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => c,
            })
            .collect::<String>()
            .trim()
            .to_string()
    }

    fn resolve_filename(&self, song: &SongInfo, ext: &str) -> String {
        if self.template == "{id}" {
            return format!("{}.{}", song.id, ext);
        }
        let name = self
            .template
            .replace("{id}", &song.id.to_string())
            .replace("{name}", &Self::sanitize_filename(&song.name))
            .replace("{singer}", &Self::sanitize_filename(&song.singer))
            .replace("{album}", &Self::sanitize_filename(&song.album));
        format!("{}.{}", name, ext)
    }

    pub fn cache_path_for(&self, song: &SongInfo, ext: &str) -> PathBuf {
        let index = self.index.read().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = index.get(&song.id) {
            return self.downloads_dir.join(&entry.filename);
        }
        let filename = self.resolve_filename(song, ext);
        self.downloads_dir.join(filename)
    }

    pub fn cache_path(&self, id: u64, ext: &str) -> PathBuf {
        let index = self.index.read().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = index.get(&id) {
            return self.downloads_dir.join(&entry.filename);
        }
        drop(index);
        self.downloads_dir.join(format!("{}.{}", id, ext))
    }

    pub fn is_cached(&self, id: u64, ext: &str) -> bool {
        self.cache_path(id, ext).exists()
    }

    pub fn ensure_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.downloads_dir)
    }

    pub fn open_cached(&self, id: u64, ext: &str) -> io::Result<File> {
        File::open(self.cache_path(id, ext))
    }

    pub fn create_provider(&self, song: &SongInfo, ext: &str) -> io::Result<CacheFileProvider> {
        self.ensure_dir()?;
        let filename = self.resolve_filename(song, ext);
        let path = self.downloads_dir.join(&filename);

        // Evict oldest entries if cache exceeds size limit
        self.evict();

        Ok(CacheFileProvider { path })
    }

    /// Mark a song as successfully cached. Call this only after the download
    /// completes, so the index never contains entries for incomplete/failed
    /// downloads.
    pub fn mark_cached(&self, song: &SongInfo, ext: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let filename = self.resolve_filename(song, ext);
        self.index
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                song.id,
                CacheEntryWrapper {
                    filename,
                    duration: song.duration,
                    accessed_at: now,
                },
            );
    }

    /// Remove index entries whose files no longer exist or are empty.
    pub fn cleanup_index(&self) {
        let mut index = self.index.write().unwrap_or_else(|e| e.into_inner());
        let stale: Vec<u64> = index
            .iter()
            .filter(|(_, entry)| {
                let path = self.downloads_dir.join(&entry.filename);
                match fs::metadata(&path) {
                    Ok(m) => m.len() == 0,
                    Err(_) => true,
                }
            })
            .map(|(id, _)| *id)
            .collect();
        for id in &stale {
            index.remove(id);
        }
        if !stale.is_empty() {
            log::info!("Cleaned up {} stale cache entries", stale.len());
        }
    }

    fn lyrics_path(&self, id: u64) -> PathBuf {
        self.lyrics_dir.join(format!("{}.json", id))
    }

    fn content_path(&self, api: &str) -> PathBuf {
        self.content_dir.join(format!("{}.json", api))
    }

    pub fn load_lyrics_cache(&self, id: u64) -> Option<ncm_api::Lyrics> {
        let path = self.lyrics_path(id);
        let data = fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub async fn load_lyrics_cache_async(&self, id: u64) -> Option<ncm_api::Lyrics> {
        let path = self.lyrics_path(id);
        tokio::task::spawn_blocking(move || {
            let data = fs::read_to_string(path).ok()?;
            serde_json::from_str(&data).ok()
        })
        .await
        .ok()
        .flatten()
    }

    pub fn save_lyrics_cache(&self, id: u64, lyrics: &ncm_api::Lyrics) {
        if let Err(e) = fs::create_dir_all(&self.lyrics_dir) {
            log::warn!("Failed to create lyrics cache dir: {e}");
            return;
        }
        match serde_json::to_string(lyrics) {
            Ok(json) => {
                if let Err(e) = fs::write(self.lyrics_path(id), json) {
                    log::warn!("Failed to write lyrics cache for {id}: {e}");
                }
            }
            Err(e) => {
                log::warn!("Failed to serialize lyrics cache for {id}: {e}");
            }
        }
    }

    pub fn load_content_cache(&self, api: &str, ttl_secs: u64) -> Option<ContentState> {
        let path = self.content_path(api);
        let data = fs::read_to_string(path).ok()?;
        let entry: ContentCacheEntry = serde_json::from_str(&data).ok()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        if now - entry.cached_at > ttl_secs {
            return None;
        }
        Some(entry.data)
    }

    pub async fn load_content_cache_async(&self, api: &str, ttl_secs: u64) -> Option<ContentState> {
        let path = self.content_path(api);
        tokio::task::spawn_blocking(move || {
            let data = fs::read_to_string(path).ok()?;
            let entry: ContentCacheEntry = serde_json::from_str(&data).ok()?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
            if now - entry.cached_at > ttl_secs {
                return None;
            }
            Some(entry.data)
        })
        .await
        .ok()
        .flatten()
    }

    /// Collect cached songs by iterating the index under a read lock, avoiding
    /// a full clone of the HashMap.
    fn collect_cached_songs(&self, index: &CacheIndex) -> Vec<SongInfo> {
        let mut songs = Vec::new();
        for (id, entry) in index {
            let path = self.downloads_dir.join(&entry.filename);
            if !path.exists() {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let (name, singer) = self.parse_filename(stem, *id);
            songs.push(SongInfo {
                id: *id,
                name,
                singer,
                artist_id: 0,
                album: String::new(),
                album_id: 0,
                pic_url: String::new(),
                duration: entry.duration,
                copyright: ncm_api::SongCopyright::Unknown,
            });
        }
        songs
    }

    pub fn list_cached_songs(&self) -> Vec<SongInfo> {
        let index = self.index.read().unwrap_or_else(|e| e.into_inner());
        self.collect_cached_songs(&index)
    }

    pub async fn list_cached_songs_async(&self) -> Vec<SongInfo> {
        let songs = {
            let index = self.index.read().unwrap_or_else(|e| e.into_inner());
            self.collect_cached_songs(&index)
        };
        songs
    }

    /// Parse a cached filename stem into (name, singer) using the template.
    fn parse_filename(&self, stem: &str, id: u64) -> (String, String) {
        Self::parse_filename_static(stem, id, &self.template)
    }

    fn parse_filename_static(stem: &str, id: u64, template: &str) -> (String, String) {
        if template == "{id}" {
            return (id.to_string(), String::new());
        }

        // Find the last literal separator in the template
        let placeholders = ["{id}", "{name}", "{singer}", "{album}"];
        let mut last_sep_start = 0;
        let mut last_sep_len = 0;
        let mut remaining = template;
        let mut offset = 0;
        while !remaining.is_empty() {
            let mut earliest = remaining.len();
            let mut earliest_len = 0;
            for ph in &placeholders {
                if let Some(pos) = remaining.find(ph)
                    && pos < earliest
                {
                    earliest = pos;
                    earliest_len = ph.len();
                }
            }
            if earliest_len == 0 {
                break;
            }
            if earliest > 0 {
                last_sep_start = offset;
                last_sep_len = earliest;
            }
            remaining = &remaining[earliest + earliest_len..];
            offset += earliest + earliest_len;
        }

        if last_sep_len == 0 {
            return (stem.to_string(), String::new());
        }

        let sep = &template[last_sep_start..last_sep_start + last_sep_len];

        if sep.is_empty() {
            return (stem.to_string(), String::new());
        }

        // Split from the right by the separator
        if let Some(pos) = stem.rfind(sep) {
            let name = stem[..pos].to_string();
            let singer = stem[pos + sep.len()..].to_string();
            return (name, singer);
        }

        (stem.to_string(), String::new())
    }

    pub fn save_content_cache(&self, api: &str, content: ContentState) {
        if let Err(e) = fs::create_dir_all(&self.content_dir) {
            log::warn!("Failed to create content cache dir: {e}");
            return;
        }
        let cached_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let entry = ContentCacheEntry {
            data: content,
            cached_at,
        };
        match serde_json::to_string(&entry) {
            Ok(json) => {
                if let Err(e) = fs::write(self.content_path(api), json) {
                    log::warn!("Failed to write content cache for {api}: {e}");
                }
            }
            Err(e) => {
                log::warn!("Failed to serialize content cache for {api}: {e}");
            }
        }
    }

    /// Evict least-recently-accessed cached audio files when total size exceeds
    /// `max_cache_bytes`. Returns the number of entries evicted.
    pub fn evict(&self) -> usize {
        let total_bytes = {
            let index = self.index.read().unwrap_or_else(|e| e.into_inner());
            self.total_cache_size(&index)
        };
        if total_bytes <= self.max_cache_bytes {
            return 0;
        }

        let mut index = self.index.write().unwrap_or_else(|e| e.into_inner());
        let total = self.total_cache_size(&index);
        if total <= self.max_cache_bytes {
            return 0;
        }

        // Collect (id, filename, accessed_at) and sort by accessed_at ascending
        let mut entries: Vec<(u64, String, u64)> = index
            .iter()
            .map(|(id, e)| (*id, e.filename.clone(), e.accessed_at))
            .collect();
        entries.sort_by_key(|e| e.2);

        let mut evicted = 0;
        let mut freed = 0u64;
        for (id, filename, _) in &entries {
            if total - freed <= self.max_cache_bytes {
                break;
            }
            let path = self.downloads_dir.join(filename);
            if let Ok(meta) = fs::metadata(&path) {
                freed += meta.len();
            }
            let _ = fs::remove_file(&path);
            index.remove(id);
            evicted += 1;
        }

        if evicted > 0 {
            log::info!("Evicted {evicted} cached songs, freed {freed} bytes");
        }
        evicted
    }

    fn total_cache_size(&self, index: &CacheIndex) -> u64 {
        index
            .values()
            .filter_map(|e| {
                let path = self.downloads_dir.join(&e.filename);
                fs::metadata(&path).ok().map(|m| m.len())
            })
            .sum()
    }
}

pub struct CacheFileProvider {
    path: PathBuf,
}

impl StorageProvider for CacheFileProvider {
    type Reader = File;
    type Writer = File;

    fn into_reader_writer(
        self,
        _content_length: Option<u64>,
    ) -> io::Result<(Self::Reader, Self::Writer)> {
        let writer = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .read(true)
            .open(&self.path)?;
        let reader = File::open(&self.path)?;
        Ok((reader, writer))
    }
}
