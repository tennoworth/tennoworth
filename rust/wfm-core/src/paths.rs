use crate::platform::{dirs_home, real_user_home};
use std::path::{Path, PathBuf};

/// `~/.config/wfminv` - the one place companion state lives.
pub fn config_dir() -> PathBuf {
    real_user_home()
        .unwrap_or_else(dirs_home)
        .join(".config")
        .join("wfminv")
}

pub fn default_jwt_path() -> PathBuf {
    config_dir().join("wfm-jwt.enc")
}

/// Keep pending-plan recovery beside a relocated credential file.
pub fn config_dir_for(jwt_path: &Path) -> PathBuf {
    jwt_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}
