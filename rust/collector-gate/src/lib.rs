//! The request gate and serve loop shared by the opt-in collectors. Each
//! collector stays its own process with its own database and quota, so a flood
//! of one cannot starve the other; only this plumbing is common.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Router,
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// One wall-clock second. Named because it decides the burst a caller can reach:
/// a fixed window admits a full allowance on each side of a boundary.
const WINDOW: Duration = Duration::from_secs(1);
/// Public requests one collector works on at once.
const SLOTS: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAINTENANCE_EVERY: Duration = Duration::from_secs(30);

struct Window {
    started: Instant,
    requests: u32,
}

impl Window {
    /// Charge one request, returning `false` when the caller is over quota.
    fn charge(&mut self, allowance: u32) -> bool {
        if self.started.elapsed() >= WINDOW {
            self.started = Instant::now();
            self.requests = 0;
        }
        if self.requests >= allowance {
            return false;
        }
        self.requests += 1;
        true
    }
}

/// A fixed-window quota plus a bound on in-flight work for the public routes.
/// It bounds work, it is not fairness: one source can spend the whole window.
pub struct Gate {
    window: Mutex<Window>,
    slots: tokio::sync::Semaphore,
    allowance: u32,
}

impl Gate {
    pub fn new(allowance: u32) -> Self {
        Self {
            window: Mutex::new(Window {
                started: Instant::now(),
                requests: 0,
            }),
            slots: tokio::sync::Semaphore::new(SLOTS),
            allowance,
        }
    }
}

/// Collector state that carries a [`Gate`].
pub trait Gated: Send + Sync + 'static {
    fn gate(&self) -> &Gate;
}

/// Middleware for the public routes. `/health` must stay outside it: the
/// monitor and the puller's rollback check read it, and a saturated window must
/// not report a working binary as down.
pub async fn admit<S: Gated>(
    State(state): State<Arc<S>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let gate = state.gate();
    {
        let Ok(mut window) = gate.window.try_lock() else {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        };
        if !window.charge(gate.allowance) {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
    }
    let Ok(_slot) = gate.slots.try_acquire() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    tokio::time::timeout(REQUEST_TIMEOUT, next.run(request))
        .await
        .unwrap_or_else(|_| StatusCode::REQUEST_TIMEOUT.into_response())
}

/// Run maintenance every 30 seconds off the async workers and serve `app` on
/// loopback at `$port_var` (or `default_port`) until Ctrl-C.
pub async fn serve<S: Send + Sync + 'static>(
    name: &'static str,
    state: Arc<S>,
    tick: fn(&S) -> anyhow::Result<()>,
    app: Router,
    port_var: &str,
    default_port: u16,
) -> anyhow::Result<()> {
    tokio::spawn(async move {
        loop {
            let state = state.clone();
            if !matches!(tokio::task::spawn_blocking(move || tick(&state)).await, Ok(Ok(()))) {
                eprintln!("{name} maintenance unavailable");
            }
            tokio::time::sleep(MAINTENANCE_EVERY).await;
        }
    });
    let port: u16 = match std::env::var(port_var) {
        Ok(value) => value.parse()?,
        Err(_) => default_port,
    };
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLOWANCE: u32 = 20;

    fn fresh() -> Window {
        Window {
            started: Instant::now(),
            requests: 0,
        }
    }

    fn aged(window: &mut Window, seconds: u64) {
        window.started = Instant::now()
            .checked_sub(Duration::from_secs(seconds))
            .expect("monotonic clock has run long enough");
    }

    #[test]
    fn the_quota_allows_the_allowance_then_refuses() {
        let mut window = fresh();
        for _ in 0..ALLOWANCE {
            assert!(window.charge(ALLOWANCE));
        }
        assert!(!window.charge(ALLOWANCE), "the request after the allowance is over quota");
    }

    #[test]
    fn a_closed_window_starts_a_fresh_allowance() {
        let mut window = fresh();
        window.requests = ALLOWANCE;
        aged(&mut window, 5);
        assert!(window.charge(ALLOWANCE));
        assert_eq!(window.requests, 1, "the window reset before charging");
    }

    #[test]
    fn a_fixed_window_admits_two_allowances_across_a_boundary() {
        // Documented behaviour, not a defect to be surprised by later: a fixed
        // window has no memory of the previous one, so a caller can spend the
        // full allowance on each side of the boundary - twice the nominal rate
        // in any rolling second. Pinned so changing it is deliberate.
        let mut window = fresh();
        for _ in 0..ALLOWANCE {
            assert!(window.charge(ALLOWANCE));
        }
        assert!(!window.charge(ALLOWANCE));
        aged(&mut window, 2);
        for _ in 0..ALLOWANCE {
            assert!(window.charge(ALLOWANCE), "the new window grants a full allowance again");
        }
        assert!(!window.charge(ALLOWANCE));
    }
}
