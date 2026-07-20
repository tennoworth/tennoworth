use std::fs;
use std::path::Path;

fn main() {
    // `tauri::generate_context!()` panics at COMPILE time if frontendDist
    // (../prototype/dist-desktop, gitignored) is missing — which it always is
    // on CI and fresh checkouts, breaking `cargo test --workspace` before any
    // test runs. A stub index.html satisfies the check; real builds overwrite
    // the directory via beforeBuildCommand.
    let dist = Path::new("../../prototype/dist-desktop");
    if !dist.exists() {
        fs::create_dir_all(dist).expect("create dist-desktop placeholder");
        fs::write(
            dist.join("index.html"),
            "<!-- placeholder: run `bun run build:desktop` in prototype/ for the real bundle -->\n",
        )
        .expect("write placeholder index.html");
    }
    tauri_build::build();
}
