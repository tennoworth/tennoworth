//! Managed state: where (if anywhere) the EE.log tailer is reading from, and
//! whether the trades it reads are being recorded.

pub struct EeLogState {
    pub path: Option<std::path::PathBuf>,
    /// Shared with the tailer. Read through `eelog_status` so a surface that
    /// mounts after a pause - or after the notification was missed - can ask
    /// what is true now, instead of depending on having been listening.
    pub recording: std::sync::Arc<crate::services::recording::Recorder>,
}
