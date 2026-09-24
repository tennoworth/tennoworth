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
//!
//! SCOPE: parses absolute `crate::` imports, re-exports, calls and type paths,
//! including grouped imports. Test-only modules are excluded without truncating
//! the remaining production file. Relative `super::` paths and paths generated
//! inside macro bodies are not resolved; this is not a compiler dependency graph.

#![cfg(test)]

use std::path::{Path, PathBuf};

fn crate_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).expect("dependency gate must read every source directory");
    for entry in entries {
        let path = entry
            .expect("dependency gate must read every source entry")
            .path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out
}

fn imported_modules(source: &str) -> Vec<String> {
    use syn::visit::Visit;

    #[derive(Default)]
    struct Dependencies(std::collections::BTreeSet<String>);

    fn use_roots(tree: &syn::UseTree, out: &mut std::collections::BTreeSet<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                out.insert(path.ident.to_string());
            }
            syn::UseTree::Name(name) => {
                out.insert(name.ident.to_string());
            }
            syn::UseTree::Rename(rename) => {
                out.insert(rename.ident.to_string());
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    use_roots(tree, out);
                }
            }
            syn::UseTree::Glob(_) => {}
        }
    }

    impl<'ast> Visit<'ast> for Dependencies {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            if item.attrs.iter().any(|attr| {
                attr.path().is_ident("cfg")
                    && attr
                        .parse_args::<syn::Path>()
                        .is_ok_and(|path| path.is_ident("test"))
            }) {
                return;
            }
            syn::visit::visit_item_mod(self, item);
        }

        fn visit_use_tree(&mut self, tree: &'ast syn::UseTree) {
            if let syn::UseTree::Path(path) = tree {
                if path.ident == "crate" {
                    use_roots(&path.tree, &mut self.0);
                    return;
                }
            }
            syn::visit::visit_use_tree(self, tree);
        }

        fn visit_path(&mut self, path: &'ast syn::Path) {
            let mut segments = path.segments.iter();
            if segments
                .next()
                .is_some_and(|segment| segment.ident == "crate")
            {
                if let Some(module) = segments.next() {
                    self.0.insert(module.ident.to_string());
                }
            }
            syn::visit::visit_path(self, path);
        }
    }

    let ast = syn::parse_file(source).expect("native source must parse for the dependency gate");
    let mut dependencies = Dependencies::default();
    dependencies.visit_file(&ast);
    dependencies.0.into_iter().collect()
}

fn violations(layer: &str, forbidden: &[&str]) -> Vec<String> {
    let root = crate_src().join(layer);
    let mut found = Vec::new();
    for file in rust_files(&root) {
        let source =
            std::fs::read_to_string(&file).expect("dependency gate must read every source file");
        for module in imported_modules(&source) {
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

    // A plain import and a re-export are both edges.
    assert_eq!(
        imported_modules("use crate::services::market::MarketCache;"),
        vec!["services"]
    );
    assert_eq!(
        imported_modules("pub use crate::commands::x;"),
        vec!["commands"]
    );
}

#[test]
fn qualified_calls_and_types_are_dependencies() {
    assert_eq!(
        imported_modules("fn f(x: crate::services::Model) { crate::shell::tray::refresh(x); }"),
        vec!["services", "shell"]
    );
}

#[test]
fn grouped_reexports_are_dependencies() {
    assert_eq!(
        imported_modules("pub(crate) use crate::{services::Model, commands::{one, two}};"),
        vec!["commands", "services"]
    );
}

#[test]
fn fixtures_and_test_modules_do_not_hide_later_production_paths() {
    assert_eq!(
        imported_modules(
            r###"
        // crate::commands::ignored();
        const TEXT: &str = "crate::shell::not_code";
        #[cfg(test)] mod tests { use crate::commands::fixture; }
        fn after_tests() { crate::services::run(); }
    "###
        ),
        vec!["services"]
    );
}
