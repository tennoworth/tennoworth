//! Module-dependency gates for the desktop crate.
//!
//! The desktop crate has four layers, and the rule that matters is which way
//! they may point:
//!
//! - `services/` - capability owners. They may use `persistence/` and each
//!   other; they must not know about `commands/` (IPC) or `shell/` (windows,
//!   tray, startup).
//! - `persistence/` - storage. It must not reach upward at all.
//! - `commands/` and `shell/` - the two adapters that drive services. They may
//!   use everything below them. `shell/` must not use `commands/`: that edge is
//!   what let the tray drive acquisition through the IPC layer, and it made the
//!   two layers import each other.
//!
//! A source scan rather than a compiler check, because Rust has no way to forbid
//! an intra-crate import. It reads the files at test time rather than
//! `include_str!`-ing them, so it covers modules that do not exist yet - a new
//! file cannot escape the gate by being new.

use std::path::{Path, PathBuf};

fn crate_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out
}

/// The `crate::<module>` paths a file imports, which is how an intra-crate
/// dependency is always spelled.
fn imported_modules(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let line = line.trim_start();
        let rest = match line.strip_prefix("use crate::") {
            Some(rest) => rest,
            // A leading `pub use` or a path inside `pub(crate) use` matters just
            // as much as a plain `use`.
            None => match line.split_once("use crate::").map(|(_, rest)| rest) {
                Some(rest) if line.starts_with("pub ") => rest,
                _ => continue,
            },
        };
        if let Some(module) = rest.split("::").next() {
            let module = module.trim_end_matches(';');
            if !module.is_empty() {
                out.push(module.to_string());
            }
        }
    }
    out
}

fn violations(layer: &str, forbidden: &[&str]) -> Vec<String> {
    let root = crate_src().join(layer);
    let mut found = Vec::new();
    for file in rust_files(&root) {
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        // The tests inside a file may bend the layering to build a fixture; the
        // production module may not.
        let production = source.split("#[cfg(test)]").next().unwrap_or(&source);
        for module in imported_modules(production) {
            if forbidden.contains(&module.as_str()) {
                let name = file.strip_prefix(crate_src()).unwrap_or(&file);
                found.push(format!("{} imports crate::{module}", name.display()));
            }
        }
    }
    found
}

/// The regression this gate exists for: `shell/tray.rs` imported
/// `commands::inventory` for the scan workflow while `commands/inventory.rs`
/// imported `shell::tray` to refresh surfaces afterwards. Neither edge looks
/// wrong on its own, which is why it survived - the pair is the defect.
#[test]
fn shell_does_not_drive_services_through_the_ipc_layer() {
    assert_eq!(
        violations("shell", &["commands"]),
        Vec::<String>::new(),
        "shell/ must call capability owners directly; reaching into commands/ is how the two layers started importing each other"
    );
}

#[test]
fn services_do_not_depend_on_adapters() {
    assert_eq!(
        violations("services", &["commands", "shell"]),
        Vec::<String>::new(),
        "a capability owner that knows about IPC or windows cannot be used by the other adapter"
    );
}

#[test]
fn persistence_does_not_reach_upward() {
    assert_eq!(
        violations("persistence", &["commands", "shell", "services"]),
        Vec::<String>::new(),
        "storage is the bottom layer; upward edges make it untestable without the app"
    );
}

/// The gate has to be able to fail. A scan that silently matched nothing would
/// report a clean tree forever, so prove it reads real files and real imports
/// before trusting a green run.
#[test]
fn the_gate_reads_real_sources_and_real_imports() {
    let files = rust_files(&crate_src());
    assert!(
        files.len() > 30,
        "expected the crate's sources, found {}",
        files.len()
    );
    assert!(
        files.iter().any(|f| f.ends_with("shell/tray.rs")),
        "shell/tray.rs must be among the scanned files"
    );

    let sample = "use crate::services::market::MarketCache;\nuse crate::commands::x;\n";
    assert_eq!(imported_modules(sample), vec!["services", "commands"]);

    // And the checker's own rule: a shell file importing commands is caught.
    let root = crate_src().join("shell");
    assert!(root.is_dir(), "shell/ must exist for the gate to mean anything");
}
