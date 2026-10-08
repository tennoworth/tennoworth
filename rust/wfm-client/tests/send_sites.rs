//! New HTTP send sites require explicit provider classification.
#![cfg(test)]

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::path::Path;

/// The source's non-test code as compact text: every `#[cfg(test)]` item is
/// dropped on its own, and literals and comments are blanked so prose that names
/// `.send()` is not code. Splitting the text at the first `#[cfg(test)]` instead
/// hid everything after a test-only type alias near the top of a file - 92% of
/// `wfm_session.rs`.
fn production(source: &str) -> String {
    let tokens: TokenStream = source
        .parse()
        .expect("native source must tokenize for the send-site gate");
    let mut out = String::new();
    render(tokens, &mut out);
    out
}

fn is_cfg_test(tree: Option<&TokenTree>) -> bool {
    matches!(tree, Some(TokenTree::Group(group))
        if group.delimiter() == Delimiter::Bracket
            && group.stream().to_string().split_whitespace().collect::<String>() == "cfg(test)")
}

fn render(tokens: TokenStream, out: &mut String) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut index = 0;
    while let Some(tree) = trees.get(index) {
        if matches!(tree, TokenTree::Punct(p) if p.as_char() == '#') && is_cfg_test(trees.get(index + 1)) {
            // The gated item ends at its body, or at a `;` or `,` on this level.
            // A top-level comma inside generics ends it early, which only keeps
            // more code in view - the gate can over-count, never under-count.
            index += 2;
            while let Some(skipped) = trees.get(index) {
                index += 1;
                match skipped {
                    TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => break,
                    TokenTree::Punct(p) if matches!(p.as_char(), ';' | ',') => break,
                    _ => {}
                }
            }
            continue;
        }
        match tree {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                out.push_str(open);
                render(group.stream(), out);
                out.push_str(close);
            }
            TokenTree::Literal(_) => out.push_str("\"\""),
            other => out.push_str(&other.to_string()),
        }
        index += 1;
    }
}

/// Requests sent by reqwest's free functions. `reqwest::get(url)` builds its own
/// default client and sends in one call, with no `.send()` for the gate to see,
/// so it would skip the shared governor and the descriptive user agent
/// unnoticed. An imported `get` is counted at the import, because its call site
/// reads like any `client.get(url)`.
fn free_function_sends(compact: &str) -> usize {
    let calls = compact.matches("reqwest::get(").count() + compact.matches("blocking::get(").count();
    let imports = compact
        .split("usereqwest::")
        .skip(1)
        .filter(|rest| {
            let statement = rest.split(';').next().unwrap_or_default();
            // Identifiers render without spaces, so `get as fetch` reads
            // `getasfetch`; a glob import may bring `get` in as well.
            statement
                .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '*'))
                .any(|name| name == "get" || name.starts_with("getas") || name == "*")
        })
        .count();
    calls + imports
}

#[test]
fn free_function_requests_count_as_send_sites() {
    let source = r##"
        use reqwest::blocking::{Client, Response};
        fn plain() { client.get(url).send(); }
        fn direct() { let _ = reqwest::get(url); let _ = reqwest::blocking::get(url); }
        use reqwest::blocking::get;
        use reqwest::{blocking::get as fetch, Client};
        use reqwest::blocking::*;
        #[cfg(test)]
        fn fake() { reqwest::blocking::get(url); }
    "##;
    let compact = production(source);
    assert_eq!(free_function_sends(&compact), 5, "{compact}");
    assert_eq!(
        free_function_sends(&production("use reqwest::blocking::{RequestBuilder, Response};")),
        0
    );
}

#[test]
fn test_only_items_do_not_hide_later_production_code() {
    let source = r##"
        #[cfg(test)]
        type Hook = fn();
        struct Session { #[cfg(test)] hook: Option<Hook>, id: u8 }
        /// Calls `.send()` on the builder.
        fn login() { let note = "x.send()"; client.get(url).send(); }
        #[cfg(test)]
        mod tests { fn fake() { client.send(); } }
        fn after_tests() { other.send(); }
    "##;
    let compact = production(source);
    assert_eq!(compact.matches(".send()").count(), 2, "{compact}");
    assert!(compact.contains("id:u8"));
    assert!(!compact.contains("hook"));
}

#[test]
fn raw_http_send_sites_are_classified() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let allowed = [
        "wfm-client/src/transport.rs",
        "wfm-client/src/policy.rs",
        "wfm-core/src/acquisition/inventory.rs",
        "tennoworth-desktop/src/services/market.rs",
        // First-party, consent-gated counter: must not inherit WFM identity headers.
        "tennoworth-desktop/src/services/usage.rs",
        // First-party, consent-gated price reports: same client as the counter.
        "tennoworth-desktop/src/services/reports.rs",
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
                let compact = production(&std::fs::read_to_string(&path).unwrap());
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let free = free_function_sends(&compact);
                if compact.contains(".send()")
                    || free > 0
                    || (compact.contains("reqwest") && compact.contains(".execute("))
                {
                    assert_eq!(
                        compact.matches(".send()").count() + free,
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
