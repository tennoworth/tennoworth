use chrono::Utc;
use std::{path::PathBuf, time::Duration};
use tennoworth_usage::{Collector, Store};

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
    let maintenance = state.clone();
    tokio::spawn(async move {
        loop {
            if maintenance.tick().is_err() {
                eprintln!("usage maintenance unavailable");
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
    let port: u16 = std::env::var("TENNOWORTH_USAGE_PORT")
        .unwrap_or_else(|_| "8082".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    axum::serve(listener, tennoworth_usage::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
