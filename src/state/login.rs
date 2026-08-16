#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoginState {
    pub loading: bool,
    pub error: Option<String>,
    pub qr_url: String,
    pub qr_key: String,
    pub qr_status_text: String,
    /// Rendered QR lines (`Dense1x2`) keyed by the url they were encoded from,
    /// so the CPU-heavy QR encoding only runs when `qr_url` changes instead of
    /// on every frame. `None` means the current `qr_url` has not been encoded yet.
    pub qr_cache: Option<(String, Vec<String>)>,
}
