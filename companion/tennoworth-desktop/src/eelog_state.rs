//! Managed state: where (if anywhere) the EE.log tailer is reading from.

pub struct EeLogState {
    pub path: Option<std::path::PathBuf>,
}
