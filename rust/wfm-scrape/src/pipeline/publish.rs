use crate::render::Snapshot;
use std::path::Path;

pub(super) fn snapshot(snapshot: &Snapshot, json_out: &Path) -> Result<(), String> {
    let tmp = json_out.with_extension("json.tmp");
    let json_str = serde_json::to_string(&snapshot).map_err(|e| format!("serialize: {e}"))?;
    let parent = json_out.parent().unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    std::fs::write(&tmp, &json_str).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, json_out).map_err(|e| format!("rename: {e}"))?;
    let meta = std::fs::metadata(json_out).map_err(|e| format!("stat: {e}"))?;
    eprintln!("Wrote {} ({} bytes)", json_out.display(), meta.len());

    Ok(())
}
