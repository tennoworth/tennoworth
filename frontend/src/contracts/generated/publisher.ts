// Derived from Rust wire contracts. Run the binding export test to update.

export type AlertEntry = { at: string, unit: string, result: string, context: Array<string>, };

export type Announcement = { id: string, starts_at: string, ends_at: string, title: string, body: string, source_url: string, };

export type AnnouncementFeed = { version: number, updated_at: string, messages: Array<Announcement>, };

export type BackupStatus = { latest_at: string | null, count: number, error: string | null, };

export type BoxStatus = { collected_at: string,
/**
 * Sources the publisher could not read; the rest of the page still loads.
 */
problems: Array<string>, readiness: Readiness | null, services: Array<ServiceStatus>, failed_units: Array<string>, alerts: Array<AlertEntry>, backup: BackupStatus, web_bundle: string | null, jobs: Array<JobStatus>, };

export type Change = { request_id: string, expected_revision: number, operation: Operation, id: string | null, content: Content | null, };

export type Content = { title: string, body: string, source_url: string, starts_at: string, ends_at: string, };

export type DashboardState = { actor: string, snapshot: Snapshot,
/**
 * The announcement the answered change created or edited; absent on a plain read.
 */
message_id: string | null, };

export type Health = "ok" | "failing";

export type History = { revision: number, actor: string, at: string, operation: string, id: string | null, content: Content | null, };

export type JobRequest = { at: string, actor: string, };

export type JobStatus = { id: string, label: string, description: string, unit: UnitStatus,
/**
 * Why the job cannot be requested right now; absent when it can.
 */
blocked: string | null, last_request: JobRequest | null, cooldown_until: string | null, };

export type Message = { id: string, draft: Content, published: Content | null, ever_published: boolean, };

export type Operation = "save" | "publish" | "withdraw";

export type Readiness = { ready: boolean, checked_at: string, errors: Array<string>, warnings: Array<string>, csv_rows: number | null, newest_sweep_age_seconds: number | null, sweep_wall_seconds: number | null, deploy_revision: string | null, checkout_head: string | null, deploy_lag: string | null, pending_scraper_commits: number | null, disk_free_bytes: number | null, disk_floor_bytes: number | null, };

export type RunJob = { job: string, };

export type ServiceStatus = { label: string, service: UnitStatus,
/**
 * Started by a timer and expected to be idle between runs, rather than always running.
 */
scheduled: boolean,
/**
 * When its timer fires next; absent for long-running services.
 */
next_run: string | null,
/**
 * The loopback health probe, for services that answer one.
 */
health: Health | null, };

export type Snapshot = { revision: number, feed_revision: number, exported_revision: number, feed: AnnouncementFeed, messages: Array<Message>, history: Array<History>, };

export type StatusState = { actor: string, status: BoxStatus, };

export type UnitStatus = { unit: string,
/**
 * False when systemd does not know the unit, i.e. it is not installed.
 */
installed: boolean, active_state: string, sub_state: string, result: string, started_at: string | null, finished_at: string | null, };
