//! Budget admission is shared; authentication and mutation reconciliation stay in callers.
use crate::governor::{context, process, AccessError, Kind, Permit};
use reqwest::blocking::{RequestBuilder, Response};

pub struct GovernedResponse {
    response: Response,
    _permit: Permit,
}
impl GovernedResponse {
    pub fn status(&self) -> reqwest::StatusCode {
        self.response.status()
    }
    pub fn headers(&self) -> &reqwest::header::HeaderMap {
        self.response.headers()
    }
    pub fn json<T: serde::de::DeserializeOwned>(self) -> reqwest::Result<T> {
        self.response.json()
    }
    pub fn text(self) -> reqwest::Result<String> {
        self.response.text()
    }
    pub fn bytes(self) -> Result<Vec<u8>, reqwest::Error> {
        self.response.bytes().map(|b| b.to_vec())
    }
}

pub fn send(builder: RequestBuilder, kind: Kind) -> Result<GovernedResponse, AccessError> {
    send_on(process(), builder, kind)
}
fn send_on(
    governor: &std::sync::Arc<crate::governor::Governor>,
    builder: RequestBuilder,
    kind: Kind,
) -> Result<GovernedResponse, AccessError> {
    struct InvalidateMutation(bool);
    impl Drop for InvalidateMutation {
        fn drop(&mut self) {
            if self.0 {
                invalidate_reads();
            }
        }
    }
    let _invalidate = InvalidateMutation(kind == Kind::Mutation);
    if kind == Kind::Mutation {
        invalidate_reads();
    }
    let attempts = if matches!(kind, Kind::Read | Kind::Contract) {
        3
    } else if kind == Kind::Mutation {
        2
    } else {
        1
    };
    let mut original = Some(builder);
    for attempt in 0..attempts {
        let builder = original
            .as_ref()
            .and_then(RequestBuilder::try_clone)
            .or_else(|| original.take())
            .ok_or(AccessError::InvalidRequest)?;
        let permit = governor.acquire(kind, context())?;
        match builder.send() {
            Ok(response) => {
                let status = response.status();
                if matches!(status.as_u16(), 429 | 509) {
                    governor.throttled(
                        response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|v| v.to_str().ok()),
                    )?;
                    let deadline = governor.status().cooldown_until_ms;
                    let wait = deadline.saturating_sub(crate::governor::unix_ms());
                    if kind == Kind::Mutation
                        && status.as_u16() == 429
                        && attempt + 1 < attempts
                        && wait <= 5000
                    {
                        drop(response);
                        drop(permit);
                        std::thread::sleep(std::time::Duration::from_millis(wait));
                        continue;
                    }
                    return Err(AccessError::Cooldown(deadline));
                }
                if kind == Kind::Mutation && status.is_server_error() {
                    return Err(AccessError::UncertainMutation);
                }
                if !status.is_server_error() || attempt + 1 == attempts {
                    if status.is_success() {
                        governor.successful();
                    }
                    return Ok(GovernedResponse {
                        response,
                        _permit: permit,
                    });
                }
            }
            Err(_) if kind == Kind::Mutation => return Err(AccessError::UncertainMutation),
            Err(_) if attempt + 1 == attempts => return Err(AccessError::Transport),
            Err(_) => {}
        }
        drop(permit);
        std::thread::sleep(crate::retry_backoff(attempt));
    }
    Err(AccessError::Transport)
}

pub trait GovernedRequest {
    fn send_governed(self, kind: Kind) -> Result<GovernedResponse, AccessError>;
}
impl GovernedRequest for RequestBuilder {
    fn send_governed(self, kind: Kind) -> Result<GovernedResponse, AccessError> {
        send(self, kind)
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct ReadKey {
    pub url: String,
    pub platform: String,
    pub account: Option<String>,
}
struct Cached {
    result: std::sync::Mutex<Option<Result<serde_json::Value, AccessError>>>,
    completed: std::sync::Mutex<Option<std::time::Instant>>,
}
type Cache = std::collections::HashMap<ReadKey, std::sync::Arc<Cached>>;
fn cache() -> &'static std::sync::Mutex<Cache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Cache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}
pub fn invalidate_reads() {
    crate::governor::lock(cache()).clear();
}

/// Fresh validation bypasses both the TTL and an earlier in-flight observation.
pub fn read_json(
    builder: RequestBuilder,
    kind: Kind,
    key: ReadKey,
    ttl: std::time::Duration,
    fresh: bool,
) -> Result<serde_json::Value, AccessError> {
    let load = || {
        let response = send(builder, kind)?;
        if !response.status().is_success() {
            return Err(AccessError::Http(response.status().as_u16()));
        }
        response.json().map_err(|_| AccessError::Transport)
    };
    if fresh {
        process().cache_observed(false);
        return load();
    }
    let (entry, owner) = {
        let mut cache = crate::governor::lock(cache());
        if cache.get(&key).is_some_and(|entry| {
            crate::governor::lock(&entry.completed).is_some_and(|at| at.elapsed() >= ttl)
        }) {
            cache.remove(&key);
        }
        if let Some(entry) = cache.get(&key) {
            (entry.clone(), false)
        } else {
            if cache.len() >= 256 {
                cache.retain(|_, entry| crate::governor::lock(&entry.completed).is_none());
                if cache.len() >= 256 {
                    return Err(AccessError::Busy);
                }
            }
            let entry = std::sync::Arc::new(Cached {
                result: std::sync::Mutex::new(None),
                completed: std::sync::Mutex::new(None),
            });
            cache.insert(key.clone(), entry.clone());
            (entry, true)
        }
    };
    process().cache_observed(!owner);
    if owner {
        let result = load();
        *crate::governor::lock(&entry.result) = Some(result.clone());
        *crate::governor::lock(&entry.completed) = Some(std::time::Instant::now());
        if result.is_err() {
            let mut cache = crate::governor::lock(cache());
            if cache
                .get(&key)
                .is_some_and(|value| std::sync::Arc::ptr_eq(value, &entry))
            {
                cache.remove(&key);
            }
        }
        result
    } else {
        loop {
            if let Some(result) = crate::governor::lock(&entry.result).clone() {
                return result;
            }
            process().check(kind, &context())?;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn server(
        replies: Vec<&'static str>,
    ) -> (
        String,
        Arc<Mutex<Vec<Instant>>>,
        std::thread::JoinHandle<()>,
    ) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let starts = Arc::new(Mutex::new(Vec::new()));
        let observed = starts.clone();
        let task = std::thread::spawn(move || {
            for response in replies {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = [0; 4096];
                let _ = socket.read(&mut bytes).unwrap();
                observed.lock().unwrap().push(Instant::now());
                if !response.is_empty() {
                    socket.write_all(response.as_bytes()).unwrap();
                }
            }
        });
        (url, starts, task)
    }
    static SERIAL: Mutex<()> = Mutex::new(());
    const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
    #[test]
    fn actual_http_starts_share_a_budget_across_request_kinds() {
        let _serial = SERIAL.lock().unwrap();
        let (url, starts, task) = server(vec![OK, OK, OK]);
        let governor = Arc::new(crate::governor::Governor::default());
        let client = crate::build_client(5).unwrap();
        std::thread::scope(|scope| {
            for kind in [Kind::Read, Kind::Mutation, Kind::Authentication] {
                let governor = &governor;
                let client = &client;
                let url = &url;
                scope.spawn(move || {
                    assert_eq!(
                        send_on(governor, client.get(url), kind)
                            .unwrap()
                            .text()
                            .unwrap(),
                        "{}"
                    );
                });
            }
        });
        task.join().unwrap();
        let starts = starts.lock().unwrap();
        for pair in starts.windows(2) {
            assert!(pair[1].duration_since(pair[0]) >= Duration::from_millis(490));
        }
        assert_eq!(governor.status().outstanding, 0);
        assert_eq!(governor.status().requests, 3);
    }
    #[test]
    fn ambiguous_creates_are_never_automatically_replayed() {
        let _serial = SERIAL.lock().unwrap();
        for response in [
            "",
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ] {
            let (url, starts, task) = server(vec![response]);
            let governor = Arc::new(crate::governor::Governor::default());
            let client = crate::build_client(2).unwrap();
            assert!(matches!(
                send_on(
                    &governor,
                    client
                        .post(url)
                        .json(&serde_json::json!({"itemId":"fixture"})),
                    Kind::Mutation
                ),
                Err(AccessError::UncertainMutation)
            ));
            task.join().unwrap();
            assert_eq!(starts.lock().unwrap().len(), 1);
            assert_eq!(governor.status().requests, 1);
        }
    }
    #[test]
    fn challenge_is_not_retried_and_long_throttles_return_control() {
        let _serial = SERIAL.lock().unwrap();
        for status in [403, 429, 509] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/", listener.local_addr().unwrap());
            let task = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                let mut buf = [0; 4096];
                let _ = socket.read(&mut buf).unwrap();
                write!(socket, "HTTP/1.1 {status} Test\r\nRetry-After: 999999999999999999999\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            });
            let governor = Arc::new(crate::governor::Governor::default());
            let response = send_on(
                &governor,
                crate::build_client(2).unwrap().get(url),
                Kind::Read,
            );
            if status == 403 {
                assert_eq!(response.unwrap().status().as_u16(), 403);
            } else {
                assert!(matches!(response, Err(AccessError::Cooldown(u64::MAX))));
                assert!(matches!(
                    governor.check(Kind::Mutation, &crate::governor::Context::default()),
                    Err(AccessError::Cooldown(_))
                ));
            }
            assert_eq!(governor.status().requests, 1);
            task.join().unwrap();
        }
    }
    #[test]
    fn identical_reads_share_work_but_variants_accounts_and_fresh_validation_do_not() {
        let _serial = SERIAL.lock().unwrap();
        let (url, starts, task) = server(vec![OK; 6]);
        let client = crate::build_client(5).unwrap();
        let key = ReadKey {
            url: url.clone(),
            platform: "pc".into(),
            account: Some("first".into()),
        };
        let load = |key: ReadKey, fresh| {
            read_json(
                client.get(&key.url),
                Kind::Read,
                key,
                Duration::from_secs(15),
                fresh,
            )
            .unwrap()
        };
        std::thread::scope(|scope| {
            for _ in 0..3 {
                let key = key.clone();
                let load = &load;
                scope.spawn(move || load(key, false));
            }
        });
        load(key.clone(), false);
        assert_eq!(starts.lock().unwrap().len(), 1);
        load(
            ReadKey {
                platform: "ps4".into(),
                ..key.clone()
            },
            false,
        );
        load(
            ReadKey {
                account: Some("second".into()),
                ..key.clone()
            },
            false,
        );
        load(
            ReadKey {
                url: format!("{url}?rank=1"),
                ..key.clone()
            },
            false,
        );
        invalidate_reads();
        load(key.clone(), false);
        load(key, true);
        task.join().unwrap();
        assert_eq!(starts.lock().unwrap().len(), 6);
    }
}
