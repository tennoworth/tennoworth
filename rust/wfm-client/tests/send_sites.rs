//! New HTTP send sites require explicit provider classification.
use std::path::Path;
#[test]
fn raw_http_send_sites_are_classified() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let allowed = [
        "wfm-client/src/transport.rs",
        "wfm-client/src/policy.rs",
        "wfm-core/src/acquisition/inventory.rs",
        "tennoworth-desktop/src/services/market.rs",
        "tennoworth-desktop/src/services/definitions.rs",
        "wfm-scrape/src/ingest/catalog.rs",
        "wfm-scrape/src/ingest/transport.rs",
    ];
    fn check(directory: &Path, root: &Path, allowed: &[&str]) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                check(&path, root, allowed);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                let production = source.split("#[cfg(test)]").next().unwrap();
                let compact: String = production.chars().filter(|c| !c.is_whitespace()).collect();
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if compact.contains(".send()")
                    || (production.contains("reqwest") && compact.contains(".execute("))
                {
                    assert_eq!(
                        compact.matches(".send()").count(),
                        1,
                        "Raw HTTP send count changed in {relative}; classify the new endpoint"
                    );
                    assert!(allowed.contains(&relative.as_str()), "Unclassified HTTP send site in {relative}; route WFM requests through the shared governor, and classify other providers.");
                }
            }
        }
    }
    for member in ["wfm-core", "wfm-client", "wfm-scrape", "tennoworth-desktop"] {
        check(&root.join(member).join("src"), root, &allowed);
    }
}
