use chrono::Utc;
use std::path::PathBuf;
use tennoworth_collectors::reports::{Collector, Store};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = PathBuf::from(
        std::env::var("TENNOWORTH_REPORTS_DB")
            .unwrap_or_else(|_| "/var/lib/tennoworth-reports/reports.db".into()),
    );
    let now = Utc::now();
    match std::env::args().nth(1).as_deref() {
        // Aggregates only: raw rows and contributor ids are never backup material.
        Some("export") => {
            let store = Store::existing(&path)?;
            println!("{}", serde_json::to_string(&store.prices(now, true)?)?);
            return Ok(());
        }
        Some(_) => anyhow::bail!("expected export or no argument"),
        None => {}
    }
    let state = Collector::new(Store::open(&path, now)?);
    tennoworth_collectors::gate::serve(
        "reports",
        state.clone(),
        Collector::tick,
        tennoworth_collectors::reports::router(state),
        "TENNOWORTH_REPORTS_PORT",
        8083,
    )
    .await
}
