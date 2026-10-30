use chrono::Utc;
use std::path::PathBuf;
use tennoworth_collectors::usage::{Collector, Store};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = PathBuf::from(
        std::env::var("TENNOWORTH_USAGE_DB")
            .unwrap_or_else(|_| "/var/lib/tennoworth-usage/usage.db".into()),
    );
    let now = Utc::now();
    let action = std::env::args().nth(1);
    let mut store = if action.as_deref() == Some("export") {
        Store::existing(&path)?
    } else {
        Store::open(&path, now)?
    };
    match action.as_deref() {
        Some("export") => {
            println!("{}", serde_json::to_string(&store.daily(now, true)?)?);
            return Ok(());
        }
        Some("restore") => {
            store.restore(serde_json::from_reader(std::io::stdin())?, now)?;
            return Ok(());
        }
        Some(_) => anyhow::bail!("expected export, restore, or no argument"),
        None => {}
    }
    let state = Collector::new(store);
    tennoworth_collectors::gate::serve(
        "usage",
        state.clone(),
        Collector::tick,
        tennoworth_collectors::usage::router(state),
        "TENNOWORTH_USAGE_PORT",
        8082,
    )
    .await
}
