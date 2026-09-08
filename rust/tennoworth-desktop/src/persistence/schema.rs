pub(super) const MIGRATIONS: &[&str] = &[
    // v1 - initial schema.
    r#"
CREATE TABLE snapshot (
  id INTEGER PRIMARY KEY,
  taken_at TEXT NOT NULL,            -- ISO8601 UTC
  source TEXT NOT NULL CHECK(source IN ('memory','import')),
  game_version TEXT
);
CREATE TABLE snapshot_item (
  snapshot_id INTEGER NOT NULL REFERENCES snapshot(id),
  slug TEXT NOT NULL,                -- resolved item slug
  count INTEGER NOT NULL,
  leveled INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (snapshot_id, slug)
);
CREATE TABLE setting (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE reserve (slug TEXT PRIMARY KEY, keep INTEGER NOT NULL);
CREATE TABLE listing_log (            -- what we listed, when, at what price
  id INTEGER PRIMARY KEY,
  slug TEXT NOT NULL, listed_at TEXT NOT NULL,
  price INTEGER NOT NULL, qty INTEGER NOT NULL,
  outcome TEXT                        -- NULL until sold/cancelled observed
);
"#,
    // v2 - make listing_log actually recordable.
    //
    // v1 shipped the table and nothing ever wrote a row, so every plan's
    // evidence died with the modal that displayed it. Writing it needs four
    // things v1 has no column for:
    //   plan_id  - groups the items of one batch, so a partial run reads as
    //              one event instead of N unrelated rows.
    //   status   - the point of the log. An 'error' row IS the record of what
    //              went wrong; without it only successes are representable.
    //   action   - created vs updated (the duplicate-listing reconcile path).
    //   order_id - the join key for observing `outcome` later: diff these
    //              against a later GET /orders and a vanished id means sold or
    //              cancelled. Nothing does that yet; this is what makes it
    //              possible without a second migration.
    //
    // status defaults to 'ok' only to satisfy NOT NULL on the zero pre-existing
    // rows; every insert passes it explicitly.
    r#"
ALTER TABLE listing_log ADD COLUMN plan_id TEXT;
ALTER TABLE listing_log ADD COLUMN status TEXT NOT NULL DEFAULT 'ok';
ALTER TABLE listing_log ADD COLUMN action TEXT;
ALTER TABLE listing_log ADD COLUMN order_id TEXT;
ALTER TABLE listing_log ADD COLUMN message TEXT;
CREATE INDEX listing_log_slug_at ON listing_log(slug, listed_at);
CREATE INDEX listing_log_order ON listing_log(order_id);
"#,
    // v3 - price watches. One row per "tell me when": `side` names which side
    // of the book is watched - 'sell' fires when the lowest online ASK drops
    // to `threshold` or below (a buying opportunity), 'buy' fires when the
    // highest online BID reaches `threshold` or above (a selling opportunity).
    // `last_*` are the checker's evidence trail (what it saw, when, when it
    // last notified) so the UI can show "12p as of 3 min ago" without a
    // network call, and re-arm rather than nag.
    r#"
CREATE TABLE watch (
  id INTEGER PRIMARY KEY,
  slug TEXT NOT NULL,
  name TEXT NOT NULL,
  subtype TEXT,
  rank INTEGER,
  side TEXT NOT NULL CHECK(side IN ('sell','buy')),
  threshold INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  last_price INTEGER,
  last_checked_at INTEGER,            -- unix seconds
  last_fired_at INTEGER               -- unix seconds
);
CREATE INDEX watch_slug ON watch(slug);
"#,
    // v4 - the trade ledger. One row per trade the game confirmed in EE.log
    // (see eelog.rs), items as JSON - the ground truth for realised P&L and
    // for auto-closing sold WFM listings. `log_stamp` + `at` dedupe a tailer
    // restart re-reading the same trade.
    r#"
CREATE TABLE trade (
  id INTEGER PRIMARY KEY,
  at INTEGER NOT NULL,                -- unix seconds, when we saw it
  partner TEXT NOT NULL,
  kind TEXT NOT NULL CHECK(kind IN ('sale','purchase','trade')),
  plat INTEGER NOT NULL,
  items TEXT NOT NULL,                -- JSON [{name, qty, direction}]
  log_stamp TEXT,
  wfm_closed INTEGER NOT NULL DEFAULT 0  -- listings adjusted after this trade
);
CREATE INDEX trade_at ON trade(at);
"#,
    r#"
CREATE TABLE notification (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  category TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL,
  target TEXT NOT NULL, created_at INTEGER NOT NULL,
  read INTEGER NOT NULL DEFAULT 0, delivery TEXT NOT NULL
);
CREATE TABLE notification_checkpoint (
  key TEXT PRIMARY KEY, stage INTEGER NOT NULL, at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL
);
"#,
    // Version 5 shipped the notification tables; allowance tracking follows it.
    r#"
CREATE TABLE trade_log_event (
  session TEXT NOT NULL,
  end_offset INTEGER NOT NULL,
  position TEXT NOT NULL,
  trade_id INTEGER NOT NULL REFERENCES trade(id),
  PRIMARY KEY(session, end_offset)
);
CREATE TABLE trade_allowance (
  id INTEGER PRIMARY KEY CHECK(id = 1),
  observation TEXT NOT NULL
);
"#,
];
