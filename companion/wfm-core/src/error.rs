//! Error types with a deliberate public shape (the rest of the crate leans on
//! `anyhow` for its fallible functions).

/// Why a memory scan / inventory fetch couldn't produce a result.
///
/// `Busy` is the single-flight guard rejecting a second concurrent scan; it is
/// transient (retry). `Failed` wraps the underlying scan or HTTP error.
pub enum ScanError {
    Busy,
    Failed(anyhow::Error),
}

impl ScanError {
    /// Render for the browser-facing `{"error": ...}` body. `Failed` matches the
    /// pre-single-flight `format!("{e:#}")` text exactly.
    pub fn into_message(self) -> String {
        match self {
            ScanError::Busy => {
                "a memory scan is already in progress; retry in a moment".to_string()
            }
            ScanError::Failed(e) => format!("{e:#}"),
        }
    }
}
