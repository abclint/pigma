mod border;
mod column;
mod navigation;
mod playerbar;
pub mod theme;
mod titles;

pub use border::*;
pub use column::*;
pub use navigation::*;
pub use playerbar::*;
pub use theme::{Theme, ThemeRegistry};
pub use titles::*;

use serde::{Deserialize, Serialize};
use std::fs;

use crate::utils::GradientPreset;
use crate::{logger::Logger, utils};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_theme: String,
    pub border: BorderConfig,
    pub seek_interval_secs: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub themes: Vec<Theme>,
    pub logger: Logger,
    pub navigation: NavConfig,
    /// Content cache TTL in seconds (0 to disable).
    #[serde(default = "default_content_cache_ttl")]
    pub content_cache_ttl: u64,
    #[serde(default)]
    pub playerbar: PlayerbarConfig,
    #[serde(default)]
    pub titles: TitlesConfig,
    #[serde(default)]
    pub columns: ColumnsConfig,
    /// 歌词高亮渐变风格：warm / cubehelix / rainbow / spectral / viridis / turbo。
    #[serde(default)]
    pub lyric_gradient: GradientPreset,
    /// 边听边存缓存目录（绝对路径或相对于 ~/.cache/pigma/ 的路径）。
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,
    /// 边听边存音质等级：standard / higher / exhigh / lossless / hires。
    #[serde(default = "default_quality")]
    pub quality: String,
    /// 缓存文件命名模板。变量：{id} {name} {singer} {album}。
    /// 例："{name}-{singer}"
    #[serde(default = "default_cache_template")]
    pub cache_template: String,
    /// YouTube fallback 代理地址（留空则不使用代理）。
    #[serde(default = "default_proxy")]
    pub proxy: String,
    /// 代理目标：`ncm` 代理 NCM API（海外用户），`yt` 代理 YouTube（默认，国内用户）。
    #[serde(default = "default_proxy_target")]
    pub proxy_target: ProxyTarget,
    /// 搜索结果数量上限。
    #[serde(default = "default_search_limit")]
    pub search_limit: u16,
}

fn default_content_cache_ttl() -> u64 {
    300
}

fn default_quality() -> String {
    "standard".into()
}

fn default_cache_dir() -> String {
    "downloads".into()
}

fn default_cache_template() -> String {
    "{name}-{singer}".into()
}

fn default_proxy() -> String {
    "http://127.0.0.1:7890".into()
}

fn default_proxy_target() -> ProxyTarget {
    ProxyTarget::Yt
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyTarget {
    Ncm,
    Yt,
    Both,
}

fn default_search_limit() -> u16 {
    100
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_theme: Theme::default().name,
            border: BorderConfig::default(),
            seek_interval_secs: 15,
            themes: Vec::new(),
            logger: Logger::default(),
            navigation: NavConfig::default(),
            content_cache_ttl: 300,
            playerbar: PlayerbarConfig::default(),
            titles: TitlesConfig::default(),
            columns: ColumnsConfig::default(),
            lyric_gradient: GradientPreset::default(),
            cache_dir: default_cache_dir(),
            quality: default_quality(),
            cache_template: default_cache_template(),
            proxy: default_proxy(),
            proxy_target: default_proxy_target(),
            search_limit: default_search_limit(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let config_dir = dirs::config_dir().map(|d| d.join("pigma"));
        let config_path = config_dir.as_ref().map(|d| d.join("config.toml"));

        let default = Config::default();
        let config = if let Some(path) = &config_path {
            if path.exists() {
                match fs::read_to_string(path) {
                    Ok(content) => match toml_edit::de::from_str(&content) {
                        Ok(cfg) => cfg,
                        Err(e) => {
                            log::warn!("Failed to parse config.toml: {e}, using defaults");
                            default
                        }
                    },
                    Err(e) => {
                        log::warn!("Failed to read config.toml: {e}, using defaults");
                        default
                    }
                }
            } else {
                default
            }
        } else {
            default
        };

        if let Some(dir) = &config_dir
            && !dir.join("config.toml").exists()
        {
            let _ = fs::create_dir_all(dir);
            let content = config.to_toml();
            if let Err(e) = fs::write(dir.join("config.toml"), content) {
                log::warn!("Failed to write default config: {e}");
            }
        }
        config
    }

    pub fn save(&self) {
        if let Some(dir) = dirs::config_dir().map(|d| d.join("pigma")) {
            if let Err(e) = fs::create_dir_all(&dir) {
                log::error!("Failed to create config directory: {e}");
                return;
            }
            let content = self.to_toml();
            if let Err(e) = fs::write(dir.join("config.toml"), content) {
                log::error!("Failed to write config.toml: {e}");
            }
        }
    }

    fn to_toml(&self) -> String {
        let mut doc = toml_edit::ser::to_string_pretty(self)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        // navigation 设为隐式
        doc["navigation"].as_table_mut().unwrap().set_implicit(true);

        // 遍历每个 section，把 items 转为内联表数组
        let sections = doc["navigation"]["sections"]
            .as_array_of_tables_mut()
            .unwrap();

        for section in sections.iter_mut() {
            utils::format::convert_aot_to_inline(section, "items", "\n  ");
        }

        let columns = doc["columns"].as_table_mut().unwrap();
        columns.set_implicit(true);

        let overrides = columns["overrides"].as_table_mut().unwrap();
        overrides.set_implicit(true);

        utils::format::convert_all_aot_to_inline(overrides, "\n  ");

        let columns = doc["columns"].as_table_mut().unwrap();
        utils::format::convert_aot_to_inline(columns, "songs", "\n  ");
        utils::format::convert_aot_to_inline(columns, "songlist", "\n  ");
        columns.set_implicit(true);

        doc.to_string()
    }
}
