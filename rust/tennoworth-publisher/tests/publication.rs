use std::fs;
use tennoworth_publisher::{
    contracts::{Change, Content, Operation},
    store::{Error, Store},
};

const SEED: &str = r#"{"version":1,"updated_at":"2026-01-01T00:00:00Z","messages":[]}"#;
const NOW: i64 = 1791763200;
fn content() -> Content {
    Content {
        title: "Useful market change".into(),
        body: "Keep crafting copies. Check the verified change before selling.".into(),
        source_url: "https://forums.warframe.com/topic/123-test/".into(),
        starts_at: "2026-10-12T00:00:00Z".into(),
        ends_at: "2026-10-15T15:00:00Z".into(),
    }
}
fn change(
    revision: i64,
    operation: Operation,
    id: Option<String>,
    content: Option<Content>,
) -> Change {
    Change {
        request_id: format!("test-request-{revision:016}"),
        expected_revision: revision,
        operation,
        id,
        content,
    }
}

#[test]
fn draft_publication_withdrawal_and_retries_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.sqlite");
    let export = dir.path().join("public/announcements.json");
    let mut store = Store::open(&db, export.clone(), SEED).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&export).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert_eq!(
            fs::metadata(&db).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let save = change(0, Operation::Save, None, Some(content()));
    let saved = store
        .apply(&save, "publisher@example.invalid", NOW)
        .unwrap();
    assert_eq!(fs::read_to_string(&export).unwrap(), SEED);
    let id = saved.messages[0].id.clone();
    let publish = change(1, Operation::Publish, Some(id.clone()), None);
    let live = store
        .apply(&publish, "publisher@example.invalid", NOW)
        .unwrap();
    assert_eq!(live.feed.messages[0].id, id);
    assert_eq!(live.feed_revision, live.exported_revision);
    assert_eq!(live.history[0].actor, "publisher@example.invalid");
    drop(store);
    let mut store = Store::open(&db, export.clone(), SEED).unwrap();
    assert_eq!(
        store
            .apply(&publish, "publisher@example.invalid", NOW)
            .unwrap()
            .revision,
        2
    );
    let withdrawn = store
        .apply(
            &change(2, Operation::Withdraw, Some(id.clone()), None),
            "publisher@example.invalid",
            NOW - 100,
        )
        .unwrap();
    assert!(withdrawn.feed.messages.is_empty());
    assert!(withdrawn.feed.updated_at > live.feed.updated_at);
    assert!(withdrawn.messages[0].ever_published);
    let republished = store
        .apply(
            &change(3, Operation::Publish, Some(id.clone()), None),
            "publisher@example.invalid",
            NOW,
        )
        .unwrap();
    assert_eq!(republished.feed.messages[0].id, id);
    assert_eq!(republished.history.len(), 4);
}

#[test]
fn stale_revisions_and_reused_request_ids_cannot_change_publication() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(
        &dir.path().join("state.sqlite"),
        dir.path().join("announcements.json"),
        SEED,
    )
    .unwrap();
    let save = change(0, Operation::Save, None, Some(content()));
    store
        .apply(&save, "publisher@example.invalid", NOW)
        .unwrap();
    let mut altered = save.clone();
    altered.content.as_mut().unwrap().title = "Different content".into();
    assert!(matches!(
        store.apply(&altered, "publisher@example.invalid", NOW),
        Err(Error::Conflict)
    ));
    altered.request_id = "different-request-0000".into();
    assert!(matches!(
        store.apply(&altered, "publisher@example.invalid", NOW),
        Err(Error::Conflict)
    ));
    assert_eq!(store.snapshot().unwrap().revision, 1);
    assert!(store.snapshot().unwrap().feed.messages.is_empty());
}

#[test]
fn export_failure_keeps_the_previous_file_and_recovers_the_latest_revision() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.sqlite");
    let export = dir.path().join("public/announcements.json");
    let mut store = Store::open(&db, export.clone(), SEED).unwrap();
    let saved = store
        .apply(
            &change(0, Operation::Save, None, Some(content())),
            "publisher@example.invalid",
            NOW,
        )
        .unwrap();
    // A directory at the temporary filename prevents a replacement without changing the old feed.
    fs::create_dir(export.parent().unwrap().join(".announcements.tmp")).unwrap();
    let queued = store
        .apply(
            &change(
                1,
                Operation::Publish,
                Some(saved.messages[0].id.clone()),
                None,
            ),
            "publisher@example.invalid",
            NOW,
        )
        .unwrap();
    assert_ne!(queued.feed_revision, queued.exported_revision);
    assert_eq!(fs::read_to_string(&export).unwrap(), SEED);
    store
        .apply(
            &change(
                2,
                Operation::Withdraw,
                Some(saved.messages[0].id.clone()),
                None,
            ),
            "publisher@example.invalid",
            NOW,
        )
        .unwrap();
    fs::remove_dir(export.parent().unwrap().join(".announcements.tmp")).unwrap();
    drop(store);
    let store = Store::open(&db, export.clone(), SEED).unwrap();
    assert!(
        tennoworth_announcements::parse(&fs::read_to_string(export).unwrap())
            .unwrap()
            .messages
            .is_empty()
    );
    assert_eq!(
        store.snapshot().unwrap().feed_revision,
        store.snapshot().unwrap().exported_revision
    );
}

#[test]
fn malformed_expired_and_oversized_publications_are_transactional() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(
        &dir.path().join("state.sqlite"),
        dir.path().join("announcements.json"),
        SEED,
    )
    .unwrap();
    let mut invalid = content();
    invalid.source_url = "https://forums.warframe.com.attacker.invalid/".into();
    assert!(matches!(
        store.apply(
            &change(0, Operation::Save, None, Some(invalid)),
            "publisher@example.invalid",
            NOW
        ),
        Err(Error::Invalid(_))
    ));
    assert_eq!(store.snapshot().unwrap().revision, 0);
    let mut revision = 0;
    for _ in 0..11 {
        let saved = store
            .apply(
                &change(revision, Operation::Save, None, Some(content())),
                "publisher@example.invalid",
                NOW,
            )
            .unwrap();
        revision += 1;
        let publish = change(
            revision,
            Operation::Publish,
            Some(saved.messages[0].id.clone()),
            None,
        );
        if revision == 21 {
            assert!(matches!(
                store.apply(&publish, "publisher@example.invalid", NOW),
                Err(Error::Invalid(_))
            ));
        } else {
            store
                .apply(&publish, "publisher@example.invalid", NOW)
                .unwrap();
            revision += 1;
        }
    }
    assert_eq!(store.snapshot().unwrap().feed.messages.len(), 10);
    let expired = change(
        revision,
        Operation::Publish,
        Some(store.snapshot().unwrap().messages[0].id.clone()),
        None,
    );
    assert!(matches!(
        store.apply(&expired, "publisher@example.invalid", NOW + 31 * 86400),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn exclusive_ownership_backup_restore_and_missing_exports_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.sqlite");
    let export = dir.path().join("public/announcements.json");
    let mut store = Store::open(&db, export.clone(), SEED).unwrap();
    assert!(Store::open(&db, export.clone(), SEED).is_err());
    store
        .apply(
            &change(0, Operation::Save, None, Some(content())),
            "publisher@example.invalid",
            NOW,
        )
        .unwrap();
    let backup = dir.path().join("backup.sqlite");
    tennoworth_publisher::store::backup_file(&db, &backup).unwrap();
    assert!(tennoworth_publisher::store::backup_file(&db, &backup).is_err());
    let online = dir.path().join("online-backup.sqlite");
    tennoworth_publisher::store::backup_file(&db, &online).unwrap();
    let restored =
        Store::open(&backup, dir.path().join("restore/announcements.json"), SEED).unwrap();
    assert_eq!(restored.snapshot().unwrap().messages.len(), 1);
    drop(restored);
    drop(store);
    fs::remove_file(&export).unwrap();
    let store = Store::open(&db, export.clone(), SEED).unwrap();
    assert!(export.exists());
    drop(store);
    fs::write(
        &export,
        r#"{"version":1,"updated_at":"2099-01-01T00:00:00Z","messages":[]}"#,
    )
    .unwrap();
    assert!(Store::open(&db, export.clone(), SEED).is_err());
    tennoworth_publisher::store::recover_publication(&db, &export).unwrap();
    let recovered = Store::open(&db, export.clone(), SEED).unwrap();
    assert!(recovered.snapshot().unwrap().feed.updated_at.as_str() > "2099-01-01T00:00:00Z");
    assert_eq!(
        recovered.snapshot().unwrap().history[0].operation,
        "recover"
    );
    assert!(tennoworth_publisher::store::recover_publication(&db, &export).is_err());
    assert!(Store::open(&dir.path().join("lost.sqlite"), export, SEED).is_err());
}
