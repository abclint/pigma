use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BorderConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub rounded: bool,
    /// 横竖边框是否跟随 corner_color 的颜色
    #[serde(default)]
    pub follow_corner_color: bool,
    /// 边框渐变预设（warm / rainbow / turbo / spectral / viridis / cubehelix）
    /// None = 纯色，Some = 渐变
    #[serde(default)]
    pub border_gradient: Option<String>,
    /// 渐变流动速度，0 = 静态，>0 = 流动
    #[serde(default)]
    pub border_gradient_speed: f64,
}

fn default_true() -> bool {
    true
}

impl Default for BorderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rounded: false,
            follow_corner_color: false,
            border_gradient: None,
            border_gradient_speed: 0.0,
        }
    }
}
