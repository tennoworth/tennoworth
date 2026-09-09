//! One request budget per process, shared across features and credentials.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const SPACING_MS: u64 = 500;
pub const CONTRACT_SPACING_MS: u64 = 6100;
pub const MAX_QUEUE: usize = 128;
pub const MAX_CONCURRENT: usize = 2;
pub const WATCH_INTERVAL_MS: u64 = 600_000;

pub(crate) fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

pub fn jitter(max_ms: u64) -> Duration {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let seed = unix_ms()
        ^ u64::from(std::process::id())
        ^ SEQUENCE
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(6364136223846793005);
    Duration::from_millis(1 + seed.wrapping_mul(2862933555777941757) % max_ms.max(1))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Read,
    Contract,
    Mutation,
    Authentication,
    WebSocket,
}

#[derive(Clone, Default)]
pub struct Context {
    pub background: bool,
    pub cancelled: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub expires: Option<Instant>,
}
impl Context {
    fn expired(&self) -> bool {
        self.cancelled
            .as_ref()
            .is_some_and(|c| c.load(std::sync::atomic::Ordering::Acquire))
            || self.expires.is_some_and(|t| Instant::now() >= t)
    }
}
thread_local! { static CONTEXT: std::cell::RefCell<Context> = std::cell::RefCell::new(Context::default()); }
pub fn context() -> Context {
    CONTEXT.with(|c| c.borrow().clone())
}
pub fn with_context<T>(value: Context, f: impl FnOnce() -> T) -> T {
    struct Restore(Context);
    impl Drop for Restore {
        fn drop(&mut self) {
            CONTEXT.with(|c| *c.borrow_mut() = self.0.clone());
        }
    }
    let _restore = Restore(CONTEXT.with(|c| c.replace(value)));
    f()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct Restrictions {
    #[ts(type = "number")]
    pub spacing_ms: u64,
    pub concurrency: usize,
    #[ts(type = "number")]
    pub contract_spacing_ms: u64,
    #[ts(type = "number")]
    pub watch_interval_ms: u64,
    pub pause_all: bool,
    pub pause_background: bool,
    pub pause_contracts: bool,
    pub pause_mutations: bool,
    pub pause_websockets: bool,
}
impl Default for Restrictions {
    fn default() -> Self {
        Self {
            spacing_ms: SPACING_MS,
            concurrency: MAX_CONCURRENT,
            contract_spacing_ms: CONTRACT_SPACING_MS,
            watch_interval_ms: WATCH_INTERVAL_MS,
            pause_all: false,
            pause_background: false,
            pause_contracts: false,
            pause_mutations: false,
            pause_websockets: false,
        }
    }
}
impl Restrictions {
    pub fn validate(&self) -> Result<(), AccessError> {
        if !(SPACING_MS..=86_400_000).contains(&self.spacing_ms)
            || !(1..=MAX_CONCURRENT).contains(&self.concurrency)
            || !(CONTRACT_SPACING_MS..=86_400_000).contains(&self.contract_spacing_ms)
            || !(WATCH_INTERVAL_MS..=604_800_000).contains(&self.watch_interval_ms)
        {
            return Err(AccessError::InvalidPolicy);
        }
        Ok(())
    }
    fn paused(&self, kind: Kind, background: bool) -> bool {
        self.pause_all
            || (background && self.pause_background)
            || (kind == Kind::Contract && self.pause_contracts)
            || (kind == Kind::Mutation && self.pause_mutations)
            || (kind == Kind::WebSocket && self.pause_websockets)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct AccessStatus {
    #[ts(type = "number")]
    pub revision: u64,
    pub reason: String,
    #[ts(type = "number")]
    pub cooldown_until_ms: u64,
    pub queue_count: usize,
    pub outstanding: usize,
    pub restrictions: Restrictions,
    #[ts(type = "number")]
    pub requests: u64,
    #[ts(type = "number")]
    pub throttles: u64,
    #[ts(type = "number")]
    pub cache_hits: u64,
    #[ts(type = "number")]
    pub cache_misses: u64,
    #[ts(type = "number")]
    pub queue_rejections: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessError {
    Busy,
    Cooldown(u64),
    Paused(String),
    Cancelled,
    UncertainMutation,
    InvalidPolicy,
    Persistence,
    Transport,
    InvalidRequest,
    Http(u16),
}
impl AccessError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Cooldown(_) => "cooldown",
            Self::Paused(_) => "paused",
            Self::Cancelled => "cancelled",
            Self::UncertainMutation => "uncertain_mutation",
            _ => "wfm",
        }
    }
}
impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => f.write_str("Market request queue is busy. Try again shortly."),
            Self::Cooldown(_) => {
                f.write_str("Market access is cooling down. Try again after the cooldown ends.")
            }
            Self::Paused(reason) => write!(f, "Market access is paused. {reason}"),
            Self::Cancelled => f.write_str("Market request cancelled before dispatch."),
            Self::UncertainMutation => f.write_str(
                "The order may have changed. Refresh and reconcile orders before resending.",
            ),
            Self::InvalidPolicy => f.write_str("Invalid market access policy."),
            Self::Persistence => {
                f.write_str("Could not persist market access safeguards. Access remains paused.")
            }
            Self::Transport => f.write_str("Market request failed."),
            Self::Http(status) => write!(f, "Market returned HTTP {status}."),
            Self::InvalidRequest => f.write_str("Invalid market request."),
        }
    }
}
impl std::error::Error for AccessError {}

pub trait Clock: Send + Sync {
    fn monotonic_ms(&self) -> u64;
    fn wall_ms(&self) -> u64;
    fn sleep(&self, duration: Duration);
}
struct RealClock(Instant);
impl Clock for RealClock {
    fn monotonic_ms(&self) -> u64 {
        self.0.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }
    fn wall_ms(&self) -> u64 {
        unix_ms()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}
struct Waiting {
    id: u64,
    kind: Kind,
    context: Context,
}
struct State {
    status: AccessStatus,
    queue: VecDeque<Waiting>,
    next_id: u64,
    foreground_streak: u8,
    last_start: Option<u64>,
    last_contract: Option<u64>,
    consecutive_throttles: u32,
    persistence: Option<std::path::PathBuf>,
    persistence_failed: bool,
}
pub struct Governor {
    state: Mutex<State>,
    clock: Arc<dyn Clock>,
}
impl Default for Governor {
    fn default() -> Self {
        Self::new(Arc::new(RealClock(Instant::now())))
    }
}
impl Governor {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            state: Mutex::new(State {
                status: AccessStatus {
                    revision: 0,
                    reason: String::new(),
                    cooldown_until_ms: 0,
                    queue_count: 0,
                    outstanding: 0,
                    restrictions: Restrictions::default(),
                    requests: 0,
                    throttles: 0,
                    cache_hits: 0,
                    cache_misses: 0,
                    queue_rejections: 0,
                },
                queue: VecDeque::new(),
                next_id: 0,
                foreground_streak: 0,
                last_start: None,
                last_contract: None,
                consecutive_throttles: 0,
                persistence: None,
                persistence_failed: false,
            }),
        }
    }
    pub fn status(&self) -> AccessStatus {
        let s = lock(&self.state);
        let mut out = s.status.clone();
        out.queue_count = s.queue.len();
        if s.persistence_failed {
            out.restrictions.pause_all = true;
            out.reason = AccessError::Persistence.to_string();
        }
        out
    }
    pub fn configure_persistence(&self, path: std::path::PathBuf) -> Result<(), AccessError> {
        let mut s = lock(&self.state);
        match std::fs::read(&path) {
            Ok(raw) => match serde_json::from_slice::<u64>(&raw) {
                Ok(deadline) => {
                    s.status.cooldown_until_ms = s.status.cooldown_until_ms.max(deadline)
                }
                Err(_) => {
                    s.persistence_failed = true;
                    return Err(AccessError::Persistence);
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                s.persistence_failed = true;
                return Err(AccessError::Persistence);
            }
        }
        s.persistence = Some(path);
        Ok(())
    }
    pub fn apply_policy(
        &self,
        revision: u64,
        reason: String,
        restrictions: Restrictions,
    ) -> Result<(), AccessError> {
        restrictions.validate()?;
        let mut s = lock(&self.state);
        if revision < s.status.revision
            || (revision == s.status.revision
                && (restrictions != s.status.restrictions || reason != s.status.reason))
        {
            return Err(AccessError::InvalidPolicy);
        }
        s.status.revision = revision;
        s.status.reason = reason;
        s.status.restrictions = restrictions;
        Ok(())
    }
    fn check_state(&self, s: &State, kind: Kind, ctx: &Context) -> Result<(), AccessError> {
        if ctx.expired() {
            return Err(AccessError::Cancelled);
        }
        if s.persistence_failed {
            return Err(AccessError::Persistence);
        }
        if s.status.restrictions.paused(kind, ctx.background) {
            return Err(AccessError::Paused(s.status.reason.clone()));
        }
        if s.status.cooldown_until_ms > self.clock.wall_ms() {
            return Err(AccessError::Cooldown(s.status.cooldown_until_ms));
        }
        Ok(())
    }
    pub fn check(&self, kind: Kind, ctx: &Context) -> Result<(), AccessError> {
        self.check_state(&lock(&self.state), kind, ctx)
    }
    pub fn acquire(self: &Arc<Self>, kind: Kind, ctx: Context) -> Result<Permit, AccessError> {
        let id = {
            let mut s = lock(&self.state);
            self.check_state(&s, kind, &ctx)?;
            if s.queue.len() >= MAX_QUEUE {
                s.status.queue_rejections = s.status.queue_rejections.saturating_add(1);
                return Err(AccessError::Busy);
            }
            let id = s.next_id;
            s.next_id = s.next_id.wrapping_add(1);
            s.queue.push_back(Waiting {
                id,
                kind,
                context: ctx.clone(),
            });
            id
        };
        loop {
            {
                let mut s = lock(&self.state);
                if let Err(e) = self.check_state(&s, kind, &ctx) {
                    s.queue.retain(|q| q.id != id);
                    return Err(e);
                }
                let now = self.clock.monotonic_ms();
                let ready = |q: &&Waiting| {
                    !q.context.expired()
                        && self.check_state(&s, q.kind, &q.context).is_ok()
                        && (q.kind != Kind::Contract
                            || s.last_contract.is_none_or(|t| {
                                now.saturating_sub(t) >= s.status.restrictions.contract_spacing_ms
                            }))
                };
                let background = s.queue.iter().filter(ready).find(|q| q.context.background);
                let foreground = s.queue.iter().filter(ready).find(|q| !q.context.background);
                let selected = if s.foreground_streak >= 4 {
                    background.or(foreground)
                } else {
                    foreground.or(background)
                }
                .map(|q| q.id);
                if selected == Some(id)
                    && s.status.outstanding < s.status.restrictions.concurrency
                    && s.last_start
                        .is_none_or(|t| now.saturating_sub(t) >= s.status.restrictions.spacing_ms)
                {
                    s.queue.retain(|q| q.id != id);
                    s.last_start = Some(now);
                    if kind == Kind::Contract {
                        s.last_contract = Some(now);
                    }
                    s.foreground_streak = if ctx.background {
                        0
                    } else {
                        s.foreground_streak.saturating_add(1)
                    };
                    s.status.outstanding += 1;
                    s.status.requests = s.status.requests.saturating_add(1);
                    return Ok(Permit(self.clone()));
                }
            }
            self.clock.sleep(Duration::from_millis(10));
        }
    }
    pub fn throttled(&self, retry_after: Option<&str>) -> Result<(), AccessError> {
        let mut s = lock(&self.state);
        let fallback = 30_000u64
            .saturating_mul(
                1u64.checked_shl(s.consecutive_throttles.min(30))
                    .unwrap_or(u64::MAX),
            )
            .min(900_000);
        s.consecutive_throttles = s.consecutive_throttles.saturating_add(1);
        let now = self.clock.wall_ms();
        let deadline =
            retry_deadline(retry_after, now).unwrap_or_else(|| now.saturating_add(fallback));
        s.status.cooldown_until_ms = s
            .status
            .cooldown_until_ms
            .max(deadline.saturating_add(jitter(1000).as_millis() as u64));
        s.status.throttles = s.status.throttles.saturating_add(1);
        if let Some(path) = &s.persistence {
            if crate::policy::atomic_write(path, s.status.cooldown_until_ms.to_string().as_bytes())
                .is_err()
            {
                s.persistence_failed = true;
                return Err(AccessError::Persistence);
            }
        }
        Ok(())
    }
    pub(crate) fn persistence_failed(&self) {
        lock(&self.state).persistence_failed = true;
    }
    pub fn cache_observed(&self, hit: bool) {
        let mut s = lock(&self.state);
        if hit {
            s.status.cache_hits = s.status.cache_hits.saturating_add(1);
        } else {
            s.status.cache_misses = s.status.cache_misses.saturating_add(1);
        }
    }
    pub fn successful(&self) {
        let mut s = lock(&self.state);
        if self.clock.wall_ms() >= s.status.cooldown_until_ms {
            s.consecutive_throttles = 0;
        }
    }
}
pub struct Permit(Arc<Governor>);
impl Drop for Permit {
    fn drop(&mut self) {
        let mut s = lock(&self.0.state);
        s.status.outstanding = s.status.outstanding.saturating_sub(1);
    }
}

pub fn retry_deadline(header: Option<&str>, now: u64) -> Option<u64> {
    let value = header?.trim();
    if !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()) {
        return Some(
            now.saturating_add(
                value
                    .parse::<u64>()
                    .unwrap_or(u64::MAX)
                    .saturating_mul(1000),
            ),
        );
    }
    httpdate::parse_http_date(value).ok().map(|date| {
        date.duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0)
            .max(now)
    })
}
pub fn process() -> &'static Arc<Governor> {
    static GOVERNOR: OnceLock<Arc<Governor>> = OnceLock::new();
    GOVERNOR.get_or_init(|| Arc::new(Governor::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    struct TestClock(AtomicU64);
    impl Clock for TestClock {
        fn monotonic_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
        fn wall_ms(&self) -> u64 {
            1_000_000 + self.monotonic_ms()
        }
        fn sleep(&self, duration: Duration) {
            self.0
                .fetch_add(duration.as_millis() as u64, Ordering::SeqCst);
        }
    }
    #[test]
    fn mixed_requests_share_spacing_without_accumulated_bursts() {
        let clock = Arc::new(TestClock(AtomicU64::new(0)));
        let governor = Arc::new(Governor::new(clock.clone()));
        let mut times = vec![];
        for kind in [Kind::Read, Kind::Mutation, Kind::Authentication, Kind::Read] {
            let _permit = governor.acquire(kind, Context::default()).unwrap();
            times.push(clock.monotonic_ms());
        }
        assert_eq!(times, [0, 500, 1000, 1500]);
        clock.0.store(100_000, Ordering::SeqCst);
        drop(governor.acquire(Kind::Read, Context::default()).unwrap());
        drop(governor.acquire(Kind::Read, Context::default()).unwrap());
        assert_eq!(clock.monotonic_ms(), 100_500);
    }
    #[test]
    fn contracts_obey_both_budgets() {
        let clock = Arc::new(TestClock(AtomicU64::new(0)));
        let governor = Arc::new(Governor::new(clock.clone()));
        drop(
            governor
                .acquire(Kind::Contract, Context::default())
                .unwrap(),
        );
        drop(governor.acquire(Kind::Read, Context::default()).unwrap());
        assert_eq!(clock.monotonic_ms(), 500);
        drop(
            governor
                .acquire(Kind::Contract, Context::default())
                .unwrap(),
        );
        assert_eq!(clock.monotonic_ms(), 6100);
    }
    #[test]
    fn retry_after_never_shortens_or_wraps() {
        assert_eq!(retry_deadline(Some("9999"), 1000), Some(10_000_000));
        assert_eq!(
            retry_deadline(Some("184467440737095516160000"), 1000),
            Some(u64::MAX)
        );
        assert_eq!(
            retry_deadline(Some("Wed, 21 Oct 2015 07:28:00 GMT"), 0),
            Some(1_445_412_480_000)
        );
        for date in [
            "Sun, 06 Nov 1994 08:49:37 GMT",
            "Sunday, 06-Nov-94 08:49:37 GMT",
            "Sun Nov  6 08:49:37 1994",
        ] {
            assert_eq!(retry_deadline(Some(date), 0), Some(784_111_777_000));
        }
        for raw in [None, Some("bad"), Some("-1"), Some("")] {
            assert_eq!(retry_deadline(raw, 1000), None);
        }
    }
    #[test]
    fn cooldown_survives_restart_and_repeated_throttles_grow() {
        let path = std::env::temp_dir().join(format!("wfm-cooldown-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let clock = Arc::new(TestClock(AtomicU64::new(0)));
        let governor = Governor::new(clock.clone());
        governor.configure_persistence(path.clone()).unwrap();
        governor.throttled(None).unwrap();
        assert!(governor.status().cooldown_until_ms > 1_030_000);
        governor.throttled(Some("bad")).unwrap();
        assert!(governor.status().cooldown_until_ms > 1_060_000);
        let restarted = Governor::new(clock);
        restarted.configure_persistence(path.clone()).unwrap();
        assert_eq!(
            restarted.status().cooldown_until_ms,
            governor.status().cooldown_until_ms
        );
        assert!(matches!(
            restarted.check(Kind::Read, &Context::default()),
            Err(AccessError::Cooldown(_))
        ));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn cancellation_and_policy_are_checked_before_admission() {
        let governor = Arc::new(Governor::default());
        let restrictions = Restrictions {
            pause_mutations: true,
            ..Default::default()
        };
        governor
            .apply_policy(1, "Maintenance".into(), restrictions)
            .unwrap();
        assert!(matches!(
            governor.acquire(Kind::Mutation, Context::default()),
            Err(AccessError::Paused(_))
        ));
        let context = Context {
            expires: Some(Instant::now()),
            ..Default::default()
        };
        assert!(matches!(
            governor.acquire(Kind::Read, context),
            Err(AccessError::Cancelled)
        ));
        assert_eq!(governor.status().requests, 0);
        governor
            .apply_policy(2, String::new(), Restrictions::default())
            .unwrap();
        assert!(governor.acquire(Kind::Read, Context::default()).is_ok());
    }
    #[test]
    fn defaults_match_the_shared_contract() {
        let fixture: Restrictions =
            serde_json::from_str(include_str!("../../../tests/fixtures/pacing.json")).unwrap();
        assert_eq!(fixture, Restrictions::default());
    }
    #[test]
    fn queue_overflow_is_bounded_and_cancellation_removes_unsent_work() {
        let governor = Arc::new(Governor::default());
        let first = governor.acquire(Kind::Read, Context::default()).unwrap();
        let second = governor.acquire(Kind::Read, Context::default()).unwrap();
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        std::thread::scope(|scope| {
            for _ in 0..MAX_QUEUE {
                let governor = governor.clone();
                let cancelled = cancelled.clone();
                scope.spawn(move || {
                    assert!(matches!(
                        governor.acquire(
                            Kind::Read,
                            Context {
                                cancelled: Some(cancelled),
                                ..Default::default()
                            }
                        ),
                        Err(AccessError::Cancelled)
                    ))
                });
            }
            let start = Instant::now();
            while governor.status().queue_count < MAX_QUEUE
                && start.elapsed() < Duration::from_secs(10)
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            let count = governor.status().queue_count;
            if count != MAX_QUEUE {
                cancelled.store(true, Ordering::Release);
            }
            assert_eq!(count, MAX_QUEUE);
            let overflow = governor.acquire(Kind::Read, Context::default());
            cancelled.store(true, Ordering::Release);
            assert!(matches!(overflow, Err(AccessError::Busy)));
        });
        assert_eq!(governor.status().queue_count, 0);
        assert_eq!(governor.status().requests, 2);
        drop(first);
        drop(second);
        assert_eq!(governor.status().outstanding, 0);
    }
    #[test]
    fn waiting_background_work_gets_a_turn_after_four_foreground_starts() {
        let governor = Arc::new(Governor::default());
        let first = governor.acquire(Kind::Read, Context::default()).unwrap();
        let second = governor.acquire(Kind::Read, Context::default()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            for background in [false, false, false, false, true] {
                let governor = governor.clone();
                let tx = tx.clone();
                scope.spawn(move || {
                    let _permit = governor
                        .acquire(
                            Kind::Read,
                            Context {
                                background,
                                ..Default::default()
                            },
                        )
                        .unwrap();
                    tx.send(background).unwrap();
                });
            }
            let start = Instant::now();
            while governor.status().queue_count < 5 && start.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(10));
            }
            drop(first);
            drop(second);
        });
        let starts: Vec<_> = rx.try_iter().collect();
        assert_eq!(starts.iter().position(|background| *background), Some(2));
    }
    #[test]
    fn a_new_pause_rejects_work_already_waiting() {
        let governor = Arc::new(Governor::default());
        let first = governor.acquire(Kind::Read, Context::default()).unwrap();
        let second = governor.acquire(Kind::Read, Context::default()).unwrap();
        std::thread::scope(|scope| {
            let waiting = governor.clone();
            let task = scope.spawn(move || {
                waiting
                    .acquire(Kind::Mutation, Context::default())
                    .map(|_| ())
            });
            let start = Instant::now();
            while governor.status().queue_count == 0 && start.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(10));
            }
            governor
                .apply_policy(
                    1,
                    "Maintenance".into(),
                    Restrictions {
                        pause_mutations: true,
                        ..Default::default()
                    },
                )
                .unwrap();
            assert!(matches!(task.join().unwrap(), Err(AccessError::Paused(_))));
        });
        drop(first);
        drop(second);
        assert_eq!(governor.status().requests, 2);
        assert_eq!(governor.status().queue_count, 0);
    }
    #[test]
    fn command_codes_match_the_frontend_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/wfm-access/outcomes.json"
        ))
        .unwrap();
        let actual: Vec<_> = [
            AccessError::Busy,
            AccessError::Cooldown(1),
            AccessError::Paused(String::new()),
            AccessError::Cancelled,
            AccessError::UncertainMutation,
            AccessError::Transport,
        ]
        .iter()
        .map(AccessError::code)
        .collect();
        assert_eq!(serde_json::to_value(actual).unwrap(), fixture["codes"]);
    }
}
