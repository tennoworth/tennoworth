//! The app declares its commands in `build.rs`, which makes Tauri refuse any
//! command a window's capability does not grant. A command registered in the
//! handler but missing from that list or from the main window's set compiles
//! fine and is rejected only at runtime, so the three lists are compared here.

#![cfg(test)]

use std::collections::BTreeSet;
use std::path::Path;

fn read(relative: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)).unwrap()
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let (_, rest) = text.split_once(start).unwrap();
    rest.split_once(end).unwrap().0
}

fn registered() -> BTreeSet<String> {
    between(&read("src/shell/startup.rs"), "generate_handler![", "])")
        .split(',')
        .map(|path| path.trim().rsplit("::").next().unwrap().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

fn quoted(list: &str) -> BTreeSet<String> {
    list.split('"').skip(1).step_by(2).map(str::to_string).collect()
}

fn permission(command: &str) -> String {
    format!("allow-{}", command.replace('_', "-"))
}

#[test]
fn every_registered_command_is_declared_and_granted_to_the_main_window() {
    let handler = registered();
    assert!(handler.len() > 50, "the handler parse found {}", handler.len());
    let declared = quoted(between(&read("build.rs"), "const COMMANDS: &[&str] = &[", "];"));
    assert_eq!(declared, handler, "build.rs COMMANDS must match generate_handler!");
    let main = quoted(between(&read("permissions/main-window.toml"), "permissions = [", "]"));
    let expected: BTreeSet<_> = handler.iter().map(|c| permission(c)).collect();
    assert_eq!(main, expected, "permissions/main-window.toml must grant every command");
    let default: serde_json::Value =
        serde_json::from_str(&read("capabilities/default.json")).unwrap();
    assert!(default["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p == "main-window"));
}

#[test]
fn the_overlay_can_only_read_its_result() {
    let overlay: serde_json::Value =
        serde_json::from_str(&read("capabilities/overlay.json")).unwrap();
    assert_eq!(overlay["windows"], serde_json::json!(["relic-overlay"]));
    assert_eq!(
        overlay["permissions"],
        serde_json::json!([permission("current_overlay_result")])
    );
    for file in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities")).unwrap() {
        let capability: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(file.unwrap().path()).unwrap()).unwrap();
        if capability["identifier"] != "overlay" {
            assert!(
                !capability["windows"].to_string().contains("relic-overlay"),
                "only the overlay capability may name the overlay window"
            );
        }
    }
}
