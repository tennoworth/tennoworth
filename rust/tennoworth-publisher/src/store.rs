use crate::contracts::{Change, Content, History, Message, Operation, Snapshot};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tennoworth_announcements::{parse, stamp, AnnouncementFeed};

#[derive(Debug)]
pub enum Error {
    Conflict,
    Invalid(String),
    Unavailable,
}
impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self::Unavailable
    }
}
impl From<serde_json::Error> for Error {
    fn from(_: serde_json::Error) -> Self {
        Self::Unavailable
    }
}

pub struct Store {
    db: Connection,
    export: PathBuf,
    _lock: fs::File,
}

fn encode<T: serde::Serialize>(value: &T) -> Result<String, Error> {
    Ok(serde_json::to_string(value)?)
}
fn decode<T: serde::de::DeserializeOwned>(value: String) -> Result<T, Error> {
    Ok(serde_json::from_str(&value)?)
}
fn utc(now: i64) -> Result<String, Error> {
    chrono::DateTime::from_timestamp(now, 0)
        .map(|v| v.to_rfc3339_opts(SecondsFormat::Secs, true))
        .ok_or_else(|| Error::Invalid("Date is outside the supported range.".into()))
}

impl Store {
    /// Creates publication state from `seed` on first use; an existing database ignores it.
    pub fn open(path: &Path, export: PathBuf, seed: &str) -> anyhow::Result<Self> {
        Self::open_with(path, export, Some(seed))
    }

    /// Opens existing publication state, so the seed file is only needed once.
    pub fn resume(path: &Path, export: PathBuf) -> anyhow::Result<Self> {
        Self::open_with(path, export, None)
    }

    fn open_with(path: &Path, export: PathBuf, seed: Option<&str>) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(path.with_extension("lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock)?;
        // Losing the publication database must not silently erase the live feed.
        if !path.exists() && export.exists() {
            anyhow::bail!("existing feed requires database restoration before startup");
        }
        if !path.exists() && seed.is_none() {
            anyhow::bail!("publication database is missing; start with a seed feed");
        }
        if !path.exists() {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(path)?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS state (id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL, feed_revision INTEGER NOT NULL, exported_revision INTEGER NOT NULL, feed TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS messages (id TEXT PRIMARY KEY, draft TEXT NOT NULL, published TEXT, ever_published INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS history (revision INTEGER PRIMARY KEY, actor TEXT NOT NULL, at TEXT NOT NULL, operation TEXT NOT NULL, message_id TEXT, content TEXT);
            CREATE TABLE IF NOT EXISTS requests (request_id TEXT NOT NULL, actor TEXT NOT NULL, digest TEXT NOT NULL, message_id TEXT NOT NULL, PRIMARY KEY(request_id, actor));")?;
        let mut store = Self {
            db,
            export,
            _lock: lock,
        };
        let existing: bool = store
            .db
            .query_row("SELECT EXISTS(SELECT 1 FROM state)", [], |r| r.get(0))?;
        if !existing {
            let seed = seed.ok_or_else(|| {
                anyhow::anyhow!("publication database has no state; start with a seed feed")
            })?;
            let seed = parse(seed).ok_or_else(|| anyhow::anyhow!("invalid seed feed"))?;
            let tx = store.db.transaction()?;
            tx.execute(
                "INSERT INTO state VALUES (1,0,0,-1,?1)",
                [serde_json::to_string(&seed)?],
            )?;
            for message in seed.messages {
                let content = Content {
                    title: message.title,
                    body: message.body,
                    source_url: message.source_url,
                    starts_at: message.starts_at,
                    ends_at: message.ends_at,
                };
                let raw = serde_json::to_string(&content)?;
                tx.execute(
                    "INSERT INTO messages VALUES (?1,?2,?2,1)",
                    params![message.id, raw],
                )?;
            }
            tx.commit()?;
        }
        if store.export.exists() {
            let disk = fs::read_to_string(&store.export)?;
            let disk =
                parse(&disk).ok_or_else(|| anyhow::anyhow!("invalid existing public feed"))?;
            let stored: String = store
                .db
                .query_row("SELECT feed FROM state", [], |r| r.get(0))?;
            let stored = parse(&stored).ok_or_else(|| anyhow::anyhow!("invalid stored feed"))?;
            if disk.updated_at > stored.updated_at
                || (disk.updated_at == stored.updated_at
                    && serde_json::to_value(&disk)? != serde_json::to_value(&stored)?)
            {
                anyhow::bail!("public feed is ahead of or conflicts with database; restore publication state before startup");
            }
        }
        // Restart also repairs a missing export, even if the database says it was written.
        store
            .export_current()
            .map_err(|_| anyhow::anyhow!("initial feed export unavailable"))?;
        Ok(store)
    }

    pub fn snapshot(&self) -> Result<Snapshot, Error> {
        let (revision, feed_revision, exported_revision, raw): (i64, i64, i64, String) =
            self.db.query_row(
                "SELECT revision,feed_revision,exported_revision,feed FROM state",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
        let mut stmt = self.db.prepare(
            "SELECT id,draft,published,ever_published FROM messages ORDER BY rowid DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })?;
        let mut messages = Vec::new();
        for row in rows {
            let (id, draft, published, ever_published) = row?;
            messages.push(Message {
                id,
                draft: decode(draft)?,
                published: published.map(decode).transpose()?,
                ever_published,
            });
        }
        let mut stmt = self.db.prepare("SELECT revision,actor,at,operation,message_id,content FROM history ORDER BY revision DESC LIMIT 100")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        let mut history = Vec::new();
        for row in rows {
            let (revision, actor, at, operation, id, content) = row?;
            history.push(History {
                revision,
                actor,
                at,
                operation,
                id,
                content: content.map(decode).transpose()?,
            });
        }
        Ok(Snapshot {
            revision,
            feed_revision,
            exported_revision,
            feed: decode(raw)?,
            messages,
            history,
        })
    }

    pub fn apply(&mut self, change: &Change, actor: &str, now: i64) -> Result<Snapshot, Error> {
        if change.request_id.len() < 16
            || change.request_id.len() > 80
            || !change
                .request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(Error::Invalid("Invalid request ID.".into()));
        }
        let digest = format!("{:x}", Sha256::digest(encode(change)?.as_bytes()));
        let tx = self.db.transaction()?;
        let old: Option<String> = tx
            .query_row(
                "SELECT digest FROM requests WHERE request_id=?1 AND actor=?2",
                params![change.request_id, actor],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = old {
            if old != digest {
                return Err(Error::Conflict);
            }
            drop(tx);
            let _ = self.retry_export();
            return self.snapshot();
        }
        let (revision, raw): (i64, String) =
            tx.query_row("SELECT revision,feed FROM state", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        if change.expected_revision != revision {
            return Err(Error::Conflict);
        }
        let revision = revision.checked_add(1).ok_or(Error::Unavailable)?;
        let mut feed: AnnouncementFeed = decode(raw)?;
        let id = match &change.id {
            Some(id) => id.clone(),
            None if matches!(change.operation, Operation::Save) => tx.query_row(
                "SELECT 'announcement-' || lower(hex(randomblob(16)))",
                [],
                |r| r.get(0),
            )?,
            None => return Err(Error::Invalid("Save a draft first.".into())),
        };
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE id=?1)",
            [&id],
            |r| r.get(0),
        )?;
        if change.id.is_some() && !exists {
            return Err(Error::Invalid("Announcement no longer exists.".into()));
        }
        let at = utc(now)?;
        let operation;
        let saved_content;
        match change.operation {
            Operation::Save => {
                let content = change
                    .content
                    .as_ref()
                    .ok_or_else(|| Error::Invalid("Draft content is missing.".into()))?;
                let draft_feed = AnnouncementFeed {
                    version: 1,
                    updated_at: at.clone(),
                    messages: vec![content.message(&id)],
                };
                if parse(&encode(&draft_feed)?).is_none() {
                    return Err(Error::Invalid("Check the text, source and dates. The window must be between 1 second and 31 days.".into()));
                }
                let raw = encode(content)?;
                tx.execute("INSERT INTO messages VALUES (?1,?2,NULL,0) ON CONFLICT(id) DO UPDATE SET draft=excluded.draft", params![id,raw])?;
                saved_content = Some(content.clone());
                operation = "save";
            }
            Operation::Publish => {
                if change.content.is_some() {
                    return Err(Error::Invalid("Save changes before publishing.".into()));
                }
                let raw: String =
                    tx.query_row("SELECT draft FROM messages WHERE id=?1", [&id], |r| {
                        r.get(0)
                    })?;
                let content: Content = decode(raw.clone())?;
                if stamp(&content.ends_at).is_none_or(|end| end <= now) {
                    return Err(Error::Invalid(
                        "The announcement has already ended. Set a future expiry.".into(),
                    ));
                }
                tx.execute(
                    "UPDATE messages SET published=?1,ever_published=1 WHERE id=?2",
                    params![raw, id],
                )?;
                saved_content = Some(content);
                operation = "publish";
            }
            Operation::Withdraw => {
                if change.content.is_some() {
                    return Err(Error::Invalid("Withdrawal cannot change content.".into()));
                }
                let published: Option<String> =
                    tx.query_row("SELECT published FROM messages WHERE id=?1", [&id], |r| {
                        r.get(0)
                    })?;
                let published = published
                    .ok_or_else(|| Error::Invalid("This announcement is not published.".into()))?;
                saved_content = Some(decode(published)?);
                tx.execute("UPDATE messages SET published=NULL WHERE id=?1", [&id])?;
                operation = "withdraw";
            }
        }
        if !matches!(change.operation, Operation::Save) {
            let previous = stamp(&feed.updated_at).ok_or(Error::Unavailable)?;
            feed.updated_at = utc(now.max(previous.checked_add(1).ok_or(Error::Unavailable)?))?;
            let mut stmt = tx.prepare(
                "SELECT id,published FROM messages WHERE published IS NOT NULL ORDER BY rowid",
            )?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            feed.messages.clear();
            for row in rows {
                let (id, raw) = row?;
                let content: Content = decode(raw)?;
                if stamp(&content.ends_at).is_some_and(|end| end > now) {
                    feed.messages.push(content.message(&id));
                }
            }
            let raw = encode(&feed)?;
            if parse(&raw).is_none() {
                return Err(Error::Invalid(
                    "The resulting feed exceeds its message or size limit.".into(),
                ));
            }
            tx.execute(
                "UPDATE state SET feed=?1,feed_revision=?2 WHERE id=1",
                params![raw, revision],
            )?;
        }
        tx.execute("UPDATE state SET revision=?1 WHERE id=1", [revision])?;
        tx.execute(
            "INSERT INTO history VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                revision,
                actor,
                at,
                operation,
                id,
                saved_content.as_ref().map(encode).transpose()?
            ],
        )?;
        tx.execute(
            "INSERT INTO requests VALUES (?1,?2,?3,?4)",
            params![change.request_id, actor, digest, id],
        )?;
        tx.commit()?;
        let _ = self.retry_export();
        self.snapshot()
    }

    /// The announcement a completed request created or changed, including on a retried request.
    pub fn request_message(&self, request_id: &str, actor: &str) -> Result<Option<String>, Error> {
        Ok(self
            .db
            .query_row(
                "SELECT message_id FROM requests WHERE request_id=?1 AND actor=?2",
                params![request_id, actor],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn retry_export(&mut self) -> Result<(), Error> {
        let pending: bool = self.db.query_row(
            "SELECT feed_revision != exported_revision FROM state",
            [],
            |r| r.get(0),
        )?;
        if pending {
            self.export_current()?;
        }
        Ok(())
    }

    fn export_current(&mut self) -> Result<(), Error> {
        let (revision, raw): (i64, String) =
            self.db
                .query_row("SELECT feed_revision,feed FROM state", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        if parse(&raw).is_none() {
            return Err(Error::Unavailable);
        }
        let parent = self.export.parent().ok_or(Error::Unavailable)?;
        fs::create_dir_all(parent).map_err(|_| Error::Unavailable)?;
        let temp = parent.join(".announcements.tmp");
        let _ = fs::remove_file(&temp);
        let result = (|| -> std::io::Result<()> {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o644);
            }
            let mut file = options.open(&temp)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                // The service's private umask must not hide the public export from Caddy.
                file.set_permissions(fs::Permissions::from_mode(0o644))?;
            }
            file.write_all(raw.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, &self.export)?;
            #[cfg(unix)]
            {
                fs::File::open(parent)?.sync_all()?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
            return Err(Error::Unavailable);
        }
        self.db.execute(
            "UPDATE state SET exported_revision=?1 WHERE id=1",
            [revision],
        )?;
        Ok(())
    }
}

pub fn now() -> i64 {
    Utc::now().timestamp()
}

pub fn backup_file(path: &Path, destination: &Path) -> anyhow::Result<()> {
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    if destination.exists() {
        anyhow::bail!("backup destination already exists");
    }
    // An interrupted VACUUM leaves only this name behind, never a complete-looking backup.
    let partial = partial_path(destination);
    let _ = fs::remove_file(&partial);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(options.open(&partial)?);
    let result = db
        .execute("VACUUM INTO ?1", [partial.to_string_lossy().as_ref()])
        .map_err(anyhow::Error::from)
        .and_then(|_| {
            fs::File::open(&partial)?.sync_all()?;
            verify_backup(&partial)
        });
    if let Err(error) = result {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    fs::rename(&partial, destination)?;
    #[cfg(unix)]
    if let Some(parent) = destination.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn partial_path(destination: &Path) -> PathBuf {
    let mut name = destination.as_os_str().to_owned();
    name.push(".partial");
    PathBuf::from(name)
}

fn verify_backup(path: &Path) -> anyhow::Result<()> {
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let check: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if check != "ok" {
        anyhow::bail!("backup failed its integrity check: {check}");
    }
    let stored: String = db.query_row("SELECT feed FROM state", [], |r| r.get(0))?;
    parse(&stored).ok_or_else(|| anyhow::anyhow!("backup holds no valid publication state"))?;
    Ok(())
}

/// Writes a timestamped snapshot into `dir`, then keeps only the newest `keep`
/// snapshots of this naming scheme there; anything else in the directory is left alone.
pub fn snapshot(path: &Path, dir: &Path, now: i64, keep: usize) -> anyhow::Result<PathBuf> {
    if keep == 0 {
        anyhow::bail!("keep at least one snapshot");
    }
    let stamp = chrono::DateTime::from_timestamp(now, 0)
        .ok_or_else(|| anyhow::anyhow!("snapshot time is out of range"))?
        .format("%Y%m%dT%H%M%SZ");
    let destination = dir.join(format!("announcements-{stamp}.sqlite"));
    backup_file(path, &destination)?;
    // Partials from interrupted runs are never snapshots; clear them once one succeeds.
    for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
        let name = entry.file_name();
        if name
            .to_str()
            .and_then(|name| name.strip_suffix(".partial"))
            .is_some_and(snapshot_name)
        {
            fs::remove_file(entry.path())?;
        }
    }
    let mut snapshots: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(snapshot_name)
        })
        .collect();
    // The fixed-width UTC stamp makes name order chronological order.
    snapshots.sort();
    let excess = snapshots.len().saturating_sub(keep);
    for old in snapshots.iter().take(excess) {
        fs::remove_file(old)?;
    }
    Ok(destination)
}

fn snapshot_name(name: &str) -> bool {
    name.strip_prefix("announcements-")
        .and_then(|rest| rest.strip_suffix(".sqlite"))
        .is_some_and(|stamp| {
            stamp.len() == 16
                && stamp.bytes().enumerate().all(|(i, b)| match i {
                    8 => b == b'T',
                    15 => b == b'Z',
                    _ => b.is_ascii_digit(),
                })
        })
}

pub fn recover_publication(path: &Path, export: &Path) -> anyhow::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(path.with_extension("lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)?;
    let mut db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let disk = parse(&fs::read_to_string(export)?)
        .ok_or_else(|| anyhow::anyhow!("invalid public feed"))?;
    let tx = db.transaction()?;
    let (revision, raw): (i64, String) =
        tx.query_row("SELECT revision,feed FROM state", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    let mut feed = parse(&raw).ok_or_else(|| anyhow::anyhow!("invalid saved publication"))?;
    let previous = stamp(&feed.updated_at)
        .zip(stamp(&disk.updated_at))
        .map(|(a, b)| a.max(b))
        .ok_or_else(|| anyhow::anyhow!("invalid publication time"))?;
    let next = previous
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("publication time overflow"))?;
    feed.updated_at =
        utc(now().max(next)).map_err(|_| anyhow::anyhow!("publication date unavailable"))?;
    let revision = revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("revision overflow"))?;
    let raw = serde_json::to_string(&feed)?;
    if parse(&raw).is_none() {
        anyhow::bail!("restored publication is invalid");
    }
    tx.execute(
        "UPDATE state SET revision=?1,feed_revision=?1,exported_revision=-1,feed=?2 WHERE id=1",
        params![revision, raw],
    )?;
    tx.execute(
        "INSERT INTO history VALUES (?1,'host recovery',?2,'recover',NULL,NULL)",
        params![revision, feed.updated_at],
    )?;
    tx.commit()?;
    Ok(())
}
