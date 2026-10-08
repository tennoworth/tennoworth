use chrono::Utc;
use std::{path::PathBuf, time::Duration};
use tennoworth_reports::{Collector, Store};

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
    let maintenance = state.clone();
    tokio::spawn(async move {
        loop {
            if maintenance.tick().is_err() {
                eprintln!("reports maintenance unavailable");
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
    let port: u16 = std::env::var("TENNOWORTH_REPORTS_PORT")
        .unwrap_or_else(|_| "8083".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    axum::serve(listener, tennoworth_reports::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
