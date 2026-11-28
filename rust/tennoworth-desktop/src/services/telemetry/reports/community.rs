//! What players' reported riven sales fetched, per weapon and verdict, shown
//! beside DE's weekly band. Display only: contributor ids are free to make, so
//! these prices never feed advice; tests/report_prices_unused.rs lists this
//! file as the one display-only reader.

use crate::services::telemetry::usage;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const RIVEN_PRICES: &str = "https://tennoworth.app/api/reports/riven-prices";
/// The service recomputes at most every five minutes and a week's figures
/// barely move within a session.
const FRESH_FOR: Duration = Duration::from_secs(30 * 60);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CommunityRivenPrice {
    pub weapon: String,
    pub verdict: String,
    pub week: String,
    /// False while the week can still receive reports.
    pub complete: bool,
    pub contributors: u32,
    pub sales: u64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
}

#[derive(Deserialize)]
struct Published {
    weeks: Vec<Week>,
}

#[derive(Deserialize)]
struct Week {
    week: String,
    complete: bool,
    rivens: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    weapon: String,
    verdict: String,
    contributors: u32,
    sales: u64,
    p25: f64,
    median: f64,
    p75: f64,
}

/// The newest published week for each weapon and verdict. Weeks arrive
/// oldest first, so a later week replaces an earlier one.
fn newest(body: Published) -> Vec<CommunityRivenPrice> {
    let mut by_key = std::collections::BTreeMap::new();
    for week in body.weeks {
        for row in week.rivens {
            by_key.insert(
                (row.weapon.clone(), row.verdict.clone()),
                CommunityRivenPrice {
                    weapon: row.weapon,
                    verdict: row.verdict,
                    week: week.week.clone(),
                    complete: week.complete,
                    contributors: row.contributors,
                    sales: row.sales,
                    p25: row.p25,
                    median: row.median,
                    p75: row.p75,
                },
            );
        }
    }
    by_key.into_values().collect()
}

static CACHE: Mutex<Option<(Instant, Vec<CommunityRivenPrice>)>> = Mutex::new(None);

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<CommunityRivenPrice>, String> {
    let failed = || "Could not load players' riven prices.".to_string();
    let response = client.get(url).send().await.map_err(|_| failed())?;
    if !response.status().is_success() {
        return Err(failed());
    }
    let body: Published = response.json().await.map_err(|_| failed())?;
    Ok(newest(body))
}

/// Empty in builds and launches that never contact the service.
#[tauri::command]
pub async fn community_riven_prices() -> Result<Vec<CommunityRivenPrice>, String> {
    if !usage::allowed() {
        return Ok(Vec::new());
    }
    if let Ok(cache) = CACHE.lock() {
        if let Some((at, rows)) = cache.as_ref().filter(|(at, _)| at.elapsed() < FRESH_FOR) {
            let _ = at;
            return Ok(rows.clone());
        }
    }
    let client =
        usage::client().map_err(|_| "Could not load players' riven prices.".to_string())?;
    let rows = fetch(&client, RIVEN_PRICES).await?;
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((Instant::now(), rows.clone()));
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_newest_week_wins_for_each_weapon_and_verdict() {
        let body: Published = serde_json::from_value(json!({
            "updated_at": "2026-10-08T12:00:00+00:00",
            "min_contributors": 5,
            "weeks": [
                {"week": "2026-W39", "complete": true, "rivens": [
                    {"weapon": "acceltra", "verdict": "sellable", "contributors": 6, "sales": 9, "p25": 90.0, "median": 100.0, "p75": 120.0},
                    {"weapon": "acceltra", "verdict": "weak", "contributors": 5, "sales": 5, "p25": 10.0, "median": 15.0, "p75": 20.0},
                ]},
                {"week": "2026-W41", "complete": false, "rivens": [
                    {"weapon": "acceltra", "verdict": "sellable", "contributors": 5, "sales": 5, "p25": 110.0, "median": 130.0, "p75": 150.0},
                ]},
            ],
        }))
        .unwrap();
        let rows = newest(body);
        assert_eq!(rows.len(), 2);
        let sellable = rows.iter().find(|r| r.verdict == "sellable").unwrap();
        assert_eq!(
            (sellable.week.as_str(), sellable.median, sellable.complete),
            ("2026-W41", 130.0, false)
        );
        let weak = rows.iter().find(|r| r.verdict == "weak").unwrap();
        assert_eq!(weak.week, "2026-W39");
    }

    #[tokio::test]
    async fn a_failed_or_malformed_answer_is_an_error() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (status, body) in [(503, "{}"), (200, "{\"weeks\": 7}")] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!(
                "http://{}/api/reports/riven-prices",
                listener.local_addr().unwrap()
            );
            let task = tokio::spawn(async move {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let mut buffer = vec![0u8; 4096];
                    let _ = socket.read(&mut buffer).await;
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                }
            });
            assert!(
                fetch(&usage::client().unwrap(), &url).await.is_err(),
                "{status} {body}"
            );
            task.abort();
        }
    }
}
