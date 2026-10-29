//! Read-only box status and the fixed list of jobs the dashboard may start.
//!
//! The publisher never runs a job itself. A request writes a file into that
//! job's own directory; a root-owned systemd path unit watching only that
//! directory starts the job's existing service, exactly as its timer would.
//! Only the names in [`JOBS`] exist, and nothing in a request reaches a command
//! line, so the dashboard cannot be made to run anything else.

use crate::contracts::{
    AlertEntry, BackupStatus, BoxStatus, Health, JobRequest, JobStatus, Readiness, ServiceStatus,
    UnitStatus,
};
use chrono::{DateTime, NaiveDateTime, SecondsFormat};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Job {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub unit: &'static str,
    /// Minimum seconds between two requests, on top of refusing while it runs.
    pub cooldown: i64,
}

/// Every job the dashboard can start. Each needs a matching
/// `deploy/tennoworth-job-<id>.path` unit; a test holds the two lists together.
pub const JOBS: &[Job] = &[
    Job {
        id: "scrape",
        label: "Run market sweep",
        description: "Starts a full sweep now. It takes about 40 minutes and uses the WFM request budget. A scheduled sweep that comes due while it runs is skipped.",
        unit: "wfm-scrape.service",
        cooldown: 15 * 60,
    },
    Job {
        id: "readiness",
        label: "Check readiness",
        description: "Runs the readiness check again and rewrites its report.",
        unit: "wfm-observations-check.service",
        cooldown: 60,
    },
    Job {
        id: "backup",
        label: "Snapshot announcements",
        description: "Takes a consistent copy of drafts and history now. The newest 30 copies are kept.",
        unit: "tennoworth-publisher-backup.service",
        cooldown: 60,
    },
    Job {
        id: "web-pull",
        label: "Pull website",
        description: "Refreshes the box's rollback copy of the website from main, if it changed. The live site is served from Cloudflare.",
        unit: "wfm-web-pull.service",
        cooldown: 120,
    },
    Job {
        id: "policy-pull",
        label: "Pull WFM policy",
        description: "Installs the latest signed WFM access policy, if it changed.",
        unit: "wfm-policy-pull.service",
        cooldown: 120,
    },
];

pub fn job(id: &str) -> Option<&'static Job> {
    JOBS.iter().find(|job| job.id == id)
}

struct Service {
    label: &'static str,
    unit: &'static str,
    timer: Option<&'static str>,
    health_port: Option<u16>,
}

const SERVICES: &[Service] = &[
    Service {
        label: "Market sweep",
        unit: "wfm-scrape.service",
        timer: Some("wfm-scrape.timer"),
        health_port: None,
    },
    Service {
        label: "Readiness check",
        unit: "wfm-observations-check.service",
        timer: Some("wfm-observations-check.timer"),
        health_port: None,
    },
    Service {
        label: "App checkout pull",
        unit: "wfm-app-pull.service",
        timer: Some("wfm-app-pull.timer"),
        health_port: None,
    },
    Service {
        label: "WFM policy pull",
        unit: "wfm-policy-pull.service",
        timer: Some("wfm-policy-pull.timer"),
        health_port: None,
    },
    Service {
        label: "Usage collector",
        unit: "tennoworth-usage.service",
        timer: None,
        health_port: Some(8082),
    },
    Service {
        label: "Price reports",
        unit: "tennoworth-reports.service",
        timer: None,
        health_port: Some(8083),
    },
    Service {
        label: "Announcement publisher",
        unit: "tennoworth-publisher.service",
        timer: None,
        health_port: None,
    },
    Service {
        label: "Announcement snapshot",
        unit: "tennoworth-publisher-backup.service",
        timer: Some("tennoworth-publisher-backup.timer"),
        health_port: None,
    },
    Service {
        label: "Web server",
        unit: "caddy.service",
        timer: None,
        health_port: None,
    },
];

/// Where unit state comes from.
pub enum Units {
    /// The host's systemd, through this `systemctl` program.
    Systemctl(PathBuf),
    /// The local dashboard has no box: each requested job "runs" for a few
    /// seconds after its request file is written, and everything else is idle.
    Simulated,
}

const SIMULATED_RUN: i64 = 8;

pub struct Ops {
    pub units: Units,
    /// Holds one directory per job id; created by the deployment, never by us.
    pub jobs_dir: PathBuf,
    pub readiness_report: PathBuf,
    pub alerts_log: PathBuf,
    pub backups_dir: PathBuf,
    pub web_stamp: PathBuf,
    pub health_host: String,
}

impl Ops {
    /// The layout the deploy kit installs on the box.
    pub fn box_defaults() -> Self {
        Self {
            units: Units::Systemctl("systemctl".into()),
            jobs_dir: "/srv/wfm/jobs".into(),
            readiness_report: "/srv/wfm/data/observations-check/report.json".into(),
            alerts_log: "/srv/wfm/alerts.log".into(),
            backups_dir: "/var/lib/tennoworth-publisher-backups".into(),
            web_stamp: "/srv/wfm/web-latest.stamp".into(),
            health_host: "127.0.0.1".into(),
        }
    }

    /// A scratch layout for the local dashboard; every source is optional there.
    pub fn simulated(root: &Path) -> Self {
        Self {
            units: Units::Simulated,
            jobs_dir: root.join("jobs"),
            readiness_report: root.join("readiness.json"),
            alerts_log: root.join("alerts.log"),
            backups_dir: root.join("backups"),
            web_stamp: root.join("web-latest.stamp"),
            health_host: String::new(),
        }
    }

    fn request_file(&self, job: &Job) -> PathBuf {
        self.jobs_dir.join(job.id).join("request")
    }

    /// Asks the box to start `job`. Writing inside the job's own directory is
    /// what wakes its path unit; the directory must already exist, because
    /// creating it here could wake a unit still watching the parent.
    pub fn trigger(&self, job: &Job, now: i64) -> Result<(), String> {
        let dir = self.jobs_dir.join(job.id);
        if !dir.is_dir() {
            return Err(format!(
                "{} is not installed on the box yet. Install the job units, then retry.",
                job.label
            ));
        }
        fs::write(self.request_file(job), format!("{now}\n")).map_err(|_| {
            format!(
                "Could not ask the box to start {}. Refresh and retry.",
                job.label
            )
        })
    }

    pub async fn units(
        &self,
        names: &[&str],
        now: i64,
    ) -> Result<BTreeMap<String, UnitStatus>, String> {
        match &self.units {
            Units::Systemctl(program) => systemctl_show(program, names).await,
            Units::Simulated => Ok(names
                .iter()
                .map(|name| ((*name).to_owned(), self.simulated_unit(name, now)))
                .collect()),
        }
    }

    fn simulated_unit(&self, name: &str, now: i64) -> UnitStatus {
        let requested = JOBS
            .iter()
            .find(|job| job.unit == name)
            .and_then(|job| fs::metadata(self.request_file(job)).ok())
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok());
        let long_running = SERVICES.iter().any(|s| s.unit == name && s.timer.is_none());
        let running = long_running || requested.is_some_and(|at| now - at < SIMULATED_RUN);
        UnitStatus {
            unit: name.into(),
            installed: true,
            active_state: if running { "active" } else { "inactive" }.into(),
            sub_state: if running { "running" } else { "dead" }.into(),
            result: "success".into(),
            started_at: requested.and_then(rfc3339),
            finished_at: requested
                .filter(|at| now - at >= SIMULATED_RUN)
                .and_then(|at| rfc3339(at + SIMULATED_RUN)),
        }
    }

    pub async fn failed_units(&self) -> Result<Vec<String>, String> {
        match &self.units {
            Units::Systemctl(program) => {
                let output = run(
                    program,
                    &[
                        "list-units",
                        "--state=failed",
                        "--plain",
                        "--no-legend",
                        "--no-pager",
                    ],
                )
                .await?;
                Ok(output
                    .lines()
                    .filter_map(|line| line.split_whitespace().next())
                    .map(str::to_owned)
                    .collect())
            }
            Units::Simulated => Ok(Vec::new()),
        }
    }

    async fn health(&self, port: u16) -> Health {
        if matches!(self.units, Units::Simulated) {
            return Health::Ok;
        }
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
        {
            Ok(client) => client,
            Err(_) => return Health::Failing,
        };
        match client
            .get(format!("http://{}:{port}/health", self.health_host))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => Health::Ok,
            _ => Health::Failing,
        }
    }

    /// Everything the status page shows except the per-job request history,
    /// which lives in the publication store and is joined in by the caller.
    pub async fn collect(&self, now: i64, requests: &[(String, String, i64)]) -> BoxStatus {
        let mut problems = Vec::new();
        let mut names: BTreeSet<&str> = BTreeSet::new();
        for service in SERVICES {
            names.insert(service.unit);
            if let Some(timer) = service.timer {
                names.insert(timer);
            }
        }
        for job in JOBS {
            names.insert(job.unit);
        }
        let names: Vec<&str> = names.into_iter().collect();
        let units = match self.units(&names, now).await {
            Ok(units) => units,
            Err(problem) => {
                problems.push(problem);
                BTreeMap::new()
            }
        };
        let failed_units = self.failed_units().await.unwrap_or_else(|problem| {
            problems.push(problem);
            Vec::new()
        });
        let mut services = Vec::new();
        for service in SERVICES {
            let health = match service.health_port {
                Some(port) => Some(self.health(port).await),
                None => None,
            };
            services.push(ServiceStatus {
                label: service.label.into(),
                service: unit_or_unknown(&units, service.unit),
                scheduled: service.timer.is_some(),
                next_run: service
                    .timer
                    .and_then(|timer| units.get(timer))
                    .filter(|timer| timer.installed)
                    .and_then(|timer| timer.started_at.clone()),
                health,
            });
        }
        let readiness = match fs::read_to_string(&self.readiness_report) {
            Ok(raw) => match readiness(&raw) {
                Some(report) => Some(report),
                None => {
                    problems.push("The readiness report is not in the expected format.".into());
                    None
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                problems.push("The readiness check has not written a report yet.".into());
                None
            }
            Err(_) => {
                problems.push("The readiness report could not be read.".into());
                None
            }
        };
        let alerts = match read_tail(&self.alerts_log, 64 * 1024) {
            Ok(text) => alerts(&text, now - 7 * 86_400),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => {
                problems.push("The alert log could not be read.".into());
                Vec::new()
            }
        };
        let backup = match crate::store::latest_snapshot(&self.backups_dir) {
            Ok((latest, count)) => BackupStatus {
                latest_at: latest.and_then(rfc3339),
                count: i64::try_from(count).unwrap_or(i64::MAX),
                error: None,
            },
            Err(error) => BackupStatus {
                latest_at: None,
                count: 0,
                error: Some(if error.kind() == std::io::ErrorKind::NotFound {
                    "No snapshot folder yet: the snapshot timer has never run.".into()
                } else {
                    "The snapshot folder could not be read.".into()
                }),
            },
        };
        let web_bundle = fs::read_to_string(&self.web_stamp)
            .ok()
            .map(|stamp| stamp.trim().to_owned())
            .filter(|stamp| !stamp.is_empty());
        let jobs = JOBS
            .iter()
            .map(|job| {
                let unit = unit_or_unknown(&units, job.unit);
                let last = requests.iter().find(|(id, _, _)| id == job.id);
                let cooldown_until = last
                    .map(|(_, _, at)| at + job.cooldown)
                    .filter(|until| *until > now);
                JobStatus {
                    id: job.id.into(),
                    label: job.label.into(),
                    description: job.description.into(),
                    blocked: self.blocked(job, &unit, cooldown_until, now),
                    unit,
                    last_request: last.and_then(|(_, actor, at)| {
                        Some(JobRequest {
                            at: rfc3339(*at)?,
                            actor: actor.clone(),
                        })
                    }),
                    cooldown_until: cooldown_until.and_then(rfc3339),
                }
            })
            .collect();
        BoxStatus {
            collected_at: rfc3339(now).unwrap_or_default(),
            problems,
            readiness,
            services,
            failed_units,
            alerts,
            backup,
            web_bundle,
            jobs,
        }
    }

    /// Why `job` cannot be requested now. Running and pacing are checked again
    /// when a request arrives; this only tells the page what to offer.
    pub fn blocked(
        &self,
        job: &Job,
        unit: &UnitStatus,
        cooldown_until: Option<i64>,
        now: i64,
    ) -> Option<String> {
        if !self.jobs_dir.join(job.id).is_dir() {
            return Some("Not installed on the box yet.".into());
        }
        if !unit.installed {
            return Some(format!("{} is not installed on the box.", unit.unit));
        }
        if is_running(unit) {
            return Some("Running now.".into());
        }
        cooldown_until.map(|until| {
            let minutes = (until - now + 59) / 60;
            format!("Requested recently; available again in {minutes} min.")
        })
    }
}

pub fn is_running(unit: &UnitStatus) -> bool {
    matches!(
        unit.active_state.as_str(),
        "active" | "activating" | "reloading" | "deactivating"
    ) && unit.sub_state != "exited"
}

fn unit_or_unknown(units: &BTreeMap<String, UnitStatus>, name: &str) -> UnitStatus {
    units.get(name).cloned().unwrap_or_else(|| UnitStatus {
        unit: name.into(),
        installed: false,
        active_state: "unknown".into(),
        sub_state: "unknown".into(),
        result: "unknown".into(),
        started_at: None,
        finished_at: None,
    })
}

pub fn rfc3339(at: i64) -> Option<String> {
    DateTime::from_timestamp(at, 0).map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true))
}

async fn run(program: &Path, args: &[&str]) -> Result<String, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(program)
            .args(args)
            .env("SYSTEMD_COLORS", "0")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "systemd did not answer in time.".to_owned())?
    .map_err(|_| "systemd could not be asked for unit state.".to_owned())?;
    if !output.status.success() {
        return Err("systemd refused the unit state query.".into());
    }
    String::from_utf8(output.stdout).map_err(|_| "systemd answered in an unreadable form.".into())
}

const SHOW_PROPERTIES: &str = "Id,LoadState,ActiveState,SubState,Result,ExecMainStartTimestamp,ExecMainExitTimestamp,NextElapseUSecRealtime";

async fn systemctl_show(
    program: &Path,
    names: &[&str],
) -> Result<BTreeMap<String, UnitStatus>, String> {
    let mut args = vec![
        "show",
        "--timestamp=utc",
        "--property",
        SHOW_PROPERTIES,
        "--",
    ];
    args.extend_from_slice(names);
    Ok(parse_show(&run(program, &args).await?))
}

/// Parses `systemctl show` output: one `Key=Value` block per unit, separated by
/// blank lines. For a timer, `started_at` carries its next elapse instead.
pub fn parse_show(output: &str) -> BTreeMap<String, UnitStatus> {
    let mut units = BTreeMap::new();
    for block in output.split("\n\n") {
        let fields: BTreeMap<&str, &str> = block
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect();
        let Some(id) = fields.get("Id").filter(|id| !id.is_empty()) else {
            continue;
        };
        let get = |key: &str| fields.get(key).copied().unwrap_or_default();
        let timer = id.ends_with(".timer");
        units.insert(
            (*id).to_owned(),
            UnitStatus {
                unit: (*id).to_owned(),
                installed: get("LoadState") == "loaded",
                active_state: get("ActiveState").into(),
                sub_state: get("SubState").into(),
                result: get("Result").into(),
                started_at: systemd_time(get(if timer {
                    "NextElapseUSecRealtime"
                } else {
                    "ExecMainStartTimestamp"
                })),
                finished_at: if timer {
                    None
                } else {
                    systemd_time(get("ExecMainExitTimestamp"))
                },
            },
        );
    }
    units
}

/// `Mon 2026-10-12 14:02:03 UTC` (from `--timestamp=utc`) to RFC 3339.
fn systemd_time(value: &str) -> Option<String> {
    let value = value.trim();
    let (_, rest) = value.split_once(' ')?;
    let rest = rest.strip_suffix(" UTC")?;
    let time = NaiveDateTime::parse_from_str(rest, "%Y-%m-%d %H:%M:%S").ok()?;
    rfc3339(time.and_utc().timestamp())
}

fn int(value: &serde_json::Value, pointer: &str) -> Option<i64> {
    value.pointer(pointer).and_then(serde_json::Value::as_i64)
}

fn text(value: &serde_json::Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn strings(value: &serde_json::Value, pointer: &str) -> Option<Vec<String>> {
    value.pointer(pointer)?.as_array().map(|items| {
        items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect()
    })
}

/// Reads the report `deploy/observations-check.sh` writes. The verdict and its
/// reasons are required; every measurement is optional, because the check
/// itself writes null where it has no evidence.
pub fn readiness(raw: &str) -> Option<Readiness> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    Some(Readiness {
        ready: value.get("ready")?.as_bool()?,
        checked_at: text(&value, "/checked_at")?,
        errors: strings(&value, "/errors")?,
        warnings: strings(&value, "/warnings")?,
        csv_rows: int(&value, "/csv/rows"),
        newest_sweep_age_seconds: int(&value, "/sweeps/newest_age_seconds"),
        sweep_wall_seconds: int(&value, "/service/wall_seconds"),
        deploy_revision: text(&value, "/deployment/revision"),
        checkout_head: text(&value, "/deployment/checkout_head"),
        deploy_lag: text(&value, "/deployment/lag"),
        pending_scraper_commits: int(&value, "/deployment/pending_scraper_commits"),
        disk_free_bytes: int(&value, "/disk/free_bytes"),
        disk_floor_bytes: int(&value, "/disk/floor_bytes"),
    })
}

fn read_tail(path: &Path, limit: u64) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(limit)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Entries from `deploy/alert.sh`'s log newer than `since`, newest first. A
/// partial first entry from the tail cut has no header and is dropped.
pub fn alerts(log: &str, since: i64) -> Vec<AlertEntry> {
    let mut entries: Vec<AlertEntry> = Vec::new();
    let mut current: Option<AlertEntry> = None;
    for line in log.lines() {
        if let Some(header) = line.strip_prefix("=== ") {
            entries.extend(current.take());
            let mut parts = header.split_whitespace();
            let (Some(at), Some(unit), result) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            current = Some(AlertEntry {
                at: at.into(),
                unit: unit.into(),
                result: result
                    .and_then(|r| r.strip_prefix("result="))
                    .unwrap_or("unknown")
                    .into(),
                context: Vec::new(),
            });
        } else if let (Some(entry), Some(context)) = (current.as_mut(), line.strip_prefix("    ")) {
            entry.context.push(context.into());
        }
    }
    entries.extend(current);
    entries.retain(|entry| {
        DateTime::parse_from_rfc3339(&entry.at).is_ok_and(|at| at.timestamp() >= since)
    });
    entries.reverse();
    entries.truncate(20);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_output_maps_services_timers_and_missing_units() {
        let output = "Id=wfm-scrape.service\nLoadState=loaded\nActiveState=inactive\nSubState=dead\nResult=success\nExecMainStartTimestamp=Mon 2026-10-12 12:00:01 UTC\nExecMainExitTimestamp=Mon 2026-10-12 12:36:40 UTC\nNextElapseUSecRealtime=\n\nId=wfm-scrape.timer\nLoadState=loaded\nActiveState=active\nSubState=waiting\nResult=success\nExecMainStartTimestamp=\nExecMainExitTimestamp=\nNextElapseUSecRealtime=Mon 2026-10-12 14:00:00 UTC\n\nId=caddy.service\nLoadState=not-found\nActiveState=inactive\nSubState=dead\nResult=success\nExecMainStartTimestamp=\nExecMainExitTimestamp=\nNextElapseUSecRealtime=\n";
        let units = parse_show(output);
        let scrape = &units["wfm-scrape.service"];
        assert!(scrape.installed);
        assert_eq!(scrape.started_at.as_deref(), Some("2026-10-12T12:00:01Z"));
        assert_eq!(scrape.finished_at.as_deref(), Some("2026-10-12T12:36:40Z"));
        assert_eq!(
            units["wfm-scrape.timer"].started_at.as_deref(),
            Some("2026-10-12T14:00:00Z")
        );
        assert!(!units["caddy.service"].installed);
        assert_eq!(units["caddy.service"].started_at, None);
    }

    #[test]
    fn a_oneshot_that_finished_is_not_running_but_one_in_progress_is() {
        let mut unit = unit_or_unknown(&BTreeMap::new(), "x.service");
        unit.active_state = "activating".into();
        unit.sub_state = "start".into();
        assert!(is_running(&unit));
        unit.active_state = "active".into();
        unit.sub_state = "exited".into();
        assert!(!is_running(&unit));
        unit.active_state = "inactive".into();
        unit.sub_state = "dead".into();
        assert!(!is_running(&unit));
    }

    #[test]
    fn alert_log_keeps_recent_entries_newest_first_with_context() {
        let log = "    tail of a cut entry\n=== 2026-10-01T03:10:04Z  old.service  result=exit-code\n    old context\n=== 2026-10-09T03:10:04Z  wfm-app-pull.service  result=exit-code\n    fatal: unable to access\n    Failed with result 'exit-code'.\n=== 2026-10-10T01:00:00Z  wfm-scrape.service  result=timeout\n";
        let since = DateTime::parse_from_rfc3339("2026-10-05T00:00:00Z")
            .unwrap()
            .timestamp();
        let entries = alerts(log, since);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].unit, "wfm-scrape.service");
        assert_eq!(entries[0].result, "timeout");
        assert_eq!(
            entries[1].context,
            ["fatal: unable to access", "Failed with result 'exit-code'."]
        );
    }

    #[test]
    fn readiness_requires_the_verdict_but_not_every_measurement() {
        let report = r#"{"ready":false,"checked_at":"2026-10-12T03:30:00Z","errors":["no sweep in 3h"],"warnings":[],"csv":{"rows":null},"deployment":{"lag":"behind","pending_scraper_commits":2}}"#;
        let parsed = readiness(report).unwrap();
        assert!(!parsed.ready);
        assert_eq!(parsed.errors, ["no sweep in 3h"]);
        assert_eq!(parsed.csv_rows, None);
        assert_eq!(parsed.deploy_lag.as_deref(), Some("behind"));
        assert_eq!(parsed.pending_scraper_commits, Some(2));
        assert!(
            readiness(r#"{"checked_at":"2026-10-12T03:30:00Z","errors":[],"warnings":[]}"#)
                .is_none()
        );
        assert!(readiness("not json").is_none());
    }

    /// The report is written by a shell script; parse what the current script
    /// actually emits, not a hand-copied sample of it.
    #[cfg(unix)]
    #[test]
    fn readiness_parses_the_current_check_script_output() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("report.json");
        let status = std::process::Command::new("bash")
            .arg(root.join("deploy/observations-check.sh"))
            .args(["--observations"])
            .arg(dir.path().join("observations"))
            .args(["--csv"])
            .arg(dir.path().join("results.csv"))
            .args(["--snapshot"])
            .arg(dir.path().join("market.json"))
            .args(["--deployed"])
            .arg(dir.path().join("deployed.json"))
            .args(["--binary"])
            .arg(dir.path().join("wfm-scrape"))
            .args(["--app"])
            .arg(dir.path().join("app"))
            .args([
                "--preservation",
                "external-backup",
                "--now",
                "2026-10-12T03:30:00Z",
                "--out",
            ])
            .arg(&out)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    dir.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap();
        assert!(
            out.exists(),
            "check wrote no report: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        let parsed =
            readiness(&fs::read_to_string(out).unwrap()).expect("current report shape parses");
        assert!(!parsed.ready, "an empty corpus must not read as ready");
        assert!(!parsed.errors.is_empty());
        assert_eq!(parsed.checked_at, "2026-10-12T03:30:00Z");
        assert!(parsed.disk_floor_bytes.is_some());
        assert!(parsed.deploy_lag.is_some());
    }

    #[test]
    fn requests_only_wake_an_installed_job_directory() {
        let dir = tempfile::tempdir().unwrap();
        let ops = Ops::simulated(dir.path());
        let backup = job("backup").unwrap();
        assert!(ops
            .trigger(backup, 100)
            .unwrap_err()
            .contains("not installed"));
        assert!(
            !dir.path().join("jobs").exists(),
            "a request must not create job directories"
        );
        fs::create_dir_all(dir.path().join("jobs/backup")).unwrap();
        ops.trigger(backup, 100).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("jobs/backup/request")).unwrap(),
            "100\n"
        );
        assert!(job("rm -rf /").is_none());
    }

    /// Each job needs a path unit that watches exactly its own directory and
    /// starts exactly its own service, and no path unit may start anything else.
    #[test]
    fn every_job_has_exactly_one_matching_path_unit() {
        let deploy = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy");
        let mut found = BTreeSet::new();
        for entry in fs::read_dir(&deploy).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            let Some(id) = name
                .strip_prefix("tennoworth-job-")
                .and_then(|n| n.strip_suffix(".path"))
            else {
                continue;
            };
            let job = job(id)
                .unwrap_or_else(|| panic!("{name} starts a job the publisher does not offer"));
            let unit = fs::read_to_string(deploy.join(&name)).unwrap();
            let lines: Vec<&str> = unit.lines().map(str::trim).collect();
            assert!(
                lines.contains(&format!("PathChanged=/srv/wfm/jobs/{id}").as_str()),
                "{name} watches the wrong path"
            );
            assert!(
                lines.contains(&format!("Unit={}", job.unit).as_str()),
                "{name} starts the wrong unit"
            );
            assert_eq!(
                lines.iter().filter(|l| l.starts_with("Path")).count(),
                1,
                "{name} must watch one path"
            );
            found.insert(id.to_owned());
        }
        let expected: BTreeSet<String> = JOBS.iter().map(|job| job.id.to_owned()).collect();
        assert_eq!(found, expected);
        let tmpfiles = fs::read_to_string(deploy.join("tennoworth-dashboard.conf")).unwrap();
        let rule = |path: &str| {
            tmpfiles
                .lines()
                .map(|line| line.split_whitespace().collect::<Vec<_>>())
                .find(|fields| fields.get(1) == Some(&path))
        };
        for job in JOBS {
            let fields = rule(&format!("/srv/wfm/jobs/{}", job.id)).unwrap_or_else(|| {
                panic!(
                    "tennoworth-dashboard.conf does not create the {} directory",
                    job.id
                )
            });
            assert_eq!(
                fields[3], "tennoworth-publisher",
                "{} must be the publisher's to write",
                job.id
            );
        }
        // /srv/wfm/data is wfm's, so tmpfiles refuses any root-owned path below it
        // ("unsafe path transition") and fails the whole file at boot.
        for line in tmpfiles
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
        {
            assert!(
                !line.contains("/srv/wfm/data"),
                "tmpfiles cannot manage paths under /srv/wfm/data: {line}"
            );
        }
    }
}
