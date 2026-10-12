use std::path::PathBuf;
use tennoworth_publisher::{auth::Auth, store::Store, App};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = PathBuf::from(
        std::env::var("TENNOWORTH_PUBLISHER_DEV_STATE").map_err(|_| {
            anyhow::anyhow!("set TENNOWORTH_PUBLISHER_DEV_STATE to a scratch directory")
        })?,
    );
    let seed = r#"{"version":1,"updated_at":"2026-01-01T00:00:00Z","messages":[]}"#;
    let store = Store::open(
        &state.join("announcements.sqlite"),
        state.join("public/announcements.json"),
        seed,
    )?;
    // Job directories exist from the start, as the deployment creates them on the box.
    for job in tennoworth_publisher::ops::JOBS {
        std::fs::create_dir_all(state.join("box/jobs").join(job.id))?;
    }
    let app = App::new(store, Auth::Local, "http://127.0.0.1:5194".into())?
        .with_ops(tennoworth_publisher::ops::Ops::simulated(
            &state.join("box"),
        ))
        .with_public_feed(tennoworth_publisher::public::PublicFeed::File(
            state.join("public/announcements.json"),
        ));
    eprintln!("Local dashboard API listening on 127.0.0.1:8084; no production authentication.");
    tennoworth_publisher::serve(app, 8084).await
}
