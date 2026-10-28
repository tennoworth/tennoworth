//! Opt-in price reports: anonymous sale prices from EE.log and riven
//! fingerprint shapes. The contract and its privacy rules are in
//! docs/price-reports.md; the request plumbing mirrors tennoworth-usage, with
//! its own process, database and quota so neither service can starve the other.

pub mod contract;
pub mod store;

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub use store::Store;

/// Requests allowed per wall-clock second across the public routes. The window
/// bounds work, it is not fairness; see the usage service for the reasoning.
pub const REQUESTS_PER_WINDOW: u32 = 20;
/// A full sales batch is about 8 KiB; anything larger is not a client of ours.
const MAX_BODY: usize = 16 * 1024;
/// Published aggregates change slowly; recomputing them per GET would let
/// unauthenticated reads buy a full aggregation each.
const PUBLISHED_TTL: Duration = Duration::from_secs(300);

#[derive(Default)]
struct Published {
    prices: Option<(Instant, String)>,
    shapes: Option<(Instant, String)>,
}

pub struct Collector {
    store: Mutex<Store>,
    published: Mutex<Published>,
    gate: collector_gate::Gate,
}

impl collector_gate::Gated for Collector {
    fn gate(&self) -> &collector_gate::Gate {
        &self.gate
    }
}

impl Collector {
    pub fn new(store: Store) -> Arc<Self> {
        Self::with_limit(store, REQUESTS_PER_WINDOW)
    }
    pub fn with_limit(store: Store, limit_per_window: u32) -> Arc<Self> {
        Arc::new(Self {
            store: Mutex::new(store),
            published: Mutex::new(Published::default()),
            gate: collector_gate::Gate::new(limit_per_window),
        })
    }
    pub fn tick(&self) -> anyhow::Result<()> {
        self.store
            .lock()
            .map_err(|_| anyhow::anyhow!("store unavailable"))?
            .tick(Utc::now())
    }
}

pub fn router(state: Arc<Collector>) -> Router {
    // /health stays outside the quota: a flood must not read as an outage and
    // roll back a working binary.
    let public = Router::new()
        .route("/api/reports/sales", post(sales))
        .route("/api/reports/riven-shape", post(riven_shape))
        .route("/api/reports/erase", post(erase))
        .route("/api/reports/prices", get(prices))
        .route("/api/reports/riven-shapes", get(riven_shapes))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            collector_gate::admit::<Collector>,
        ));
    let unmetered = Router::new().route("/health", get(health));
    public.merge(unmetered).with_state(state)
}

type Body<T> = Result<Json<T>, axum::extract::rejection::JsonRejection>;

async fn sales(
    State(state): State<Arc<Collector>>,
    payload: Body<contract::SalesBatch>,
) -> StatusCode {
    let Ok(Json(batch)) = payload else {
        return StatusCode::BAD_REQUEST;
    };
    let Ok(batch) = contract::validate_sales(batch, Utc::now().date_naive()) else {
        return StatusCode::BAD_REQUEST;
    };
    let Ok(mut store) = state.store.try_lock() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    store
        .add_sales(batch)
        .map_or_else(|s| s, |_| StatusCode::NO_CONTENT)
}

async fn riven_shape(
    State(state): State<Arc<Collector>>,
    payload: Body<contract::ShapeReport>,
) -> StatusCode {
    let Ok(Json(report)) = payload else {
        return StatusCode::BAD_REQUEST;
    };
    let Ok(shape) = contract::validate_shape(report) else {
        return StatusCode::BAD_REQUEST;
    };
    let Ok(mut store) = state.store.try_lock() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    store
        .add_shape(shape, Utc::now())
        .map_or_else(|s| s, |_| StatusCode::NO_CONTENT)
}

/// Answers 204 whether or not anything matched, so the route says nothing
/// about which weeks hold reports.
async fn erase(
    State(state): State<Arc<Collector>>,
    payload: Body<contract::EraseRequest>,
) -> StatusCode {
    let Ok(Json(request)) = payload else {
        return StatusCode::BAD_REQUEST;
    };
    let Ok(request) = contract::validate_erase(request) else {
        return StatusCode::BAD_REQUEST;
    };
    let Ok(mut store) = state.store.try_lock() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    store
        .erase(&request)
        .map_or_else(|s| s, |_| StatusCode::NO_CONTENT)
}

fn cached(
    state: &Collector,
    pick: fn(&mut Published) -> &mut Option<(Instant, String)>,
    build: impl FnOnce(&Store) -> anyhow::Result<String>,
) -> Response {
    let Ok(mut published) = state.published.try_lock() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let slot = pick(&mut published);
    let fresh = slot
        .as_ref()
        .filter(|(at, _)| at.elapsed() < PUBLISHED_TTL)
        .map(|(_, body)| body.clone());
    let body = match fresh {
        Some(body) => body,
        None => {
            let Ok(store) = state.store.try_lock() else {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            };
            let Ok(body) = build(&store) else {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            };
            *slot = Some((Instant::now(), body.clone()));
            body
        }
    };
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        body,
    )
        .into_response()
}

async fn prices(State(state): State<Arc<Collector>>) -> Response {
    cached(
        &state,
        |p| &mut p.prices,
        |store| Ok(serde_json::to_string(&store.prices(Utc::now(), false)?)?),
    )
}

async fn riven_shapes(State(state): State<Arc<Collector>>) -> Response {
    cached(
        &state,
        |p| &mut p.shapes,
        |store| Ok(serde_json::to_string(&store.shapes(Utc::now())?)?),
    )
}

async fn health(State(state): State<Arc<Collector>>) -> StatusCode {
    let Ok(store) = state.store.try_lock() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    if store.healthy(Utc::now()) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

#[cfg(test)]
mod tests;
