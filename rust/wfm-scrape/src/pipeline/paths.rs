use std::path::PathBuf;

/// Where `build` reads the sweep's CSV and writes what it publishes. The box
/// keeps both outside the checkout (`--data-dir`, `--publish-dir`); a local run
/// defaults to the checkout, where the tracked bootstrap snapshot lives.
#[derive(Debug, Clone)]
pub struct Dirs {
    pub data: PathBuf,
    pub publish: PathBuf,
}

impl Dirs {
    pub fn resolve(data: Option<PathBuf>, publish: Option<PathBuf>) -> Result<Self, String> {
        if let (Some(data), Some(publish)) = (&data, &publish) {
            return Ok(Dirs {
                data: data.clone(),
                publish: publish.clone(),
            });
        }
        let root = find_root()?;
        Ok(Dirs {
            data: data.unwrap_or_else(|| root.clone()),
            publish: publish.unwrap_or_else(|| root.join("frontend").join("public")),
        })
    }
}

pub fn find_root() -> Result<PathBuf, String> {
    let dir = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    find_root_from(dir)
}

fn find_root_from(mut dir: PathBuf) -> Result<PathBuf, String> {
    loop {
        if dir.join("frontend").join("public").is_dir() && dir.join("wfm_results.csv").exists() {
            return Ok(dir);
        }
        if dir.join(".git").exists() && dir.join("frontend").join("public").is_dir() {
            return Ok(dir);
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    Err("Cannot find project root (looked for frontend/public/ + wfm_results.csv)".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_dirs_need_no_checkout() {
        let dirs = Dirs::resolve(
            Some("/srv/wfm/data/scrape".into()),
            Some("/srv/wfm/data/public".into()),
        )
        .unwrap();
        assert_eq!(dirs.data, PathBuf::from("/srv/wfm/data/scrape"));
        assert_eq!(dirs.publish, PathBuf::from("/srv/wfm/data/public"));
    }

    #[test]
    fn locates_linked_worktrees_without_requiring_generated_csv() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("snapshot-root-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(root.join("frontend/public")).unwrap();
        std::fs::create_dir_all(root.join("rust/wfm-scrape")).unwrap();
        std::fs::write(root.join(".git"), "gitdir: unused-for-discovery").unwrap();
        let found = find_root_from(root.join("rust/wfm-scrape"));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found.unwrap(), root);
    }
}
