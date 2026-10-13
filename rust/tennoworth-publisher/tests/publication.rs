use std::fs;
use tennoworth_publisher::{
    contracts::{Change, Content, EventChange, EventContent, EventOperation, Operation},
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

#[test]
fn existing_state_resumes_without_a_seed_and_retries_name_their_message() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.sqlite");
    let export = dir.path().join("public/announcements.json");
    assert!(Store::resume(&db, export.clone()).is_err());
    assert!(
        !db.exists(),
        "a refused start must not leave an empty database behind"
    );
    let mut store = Store::open(&db, export.clone(), SEED).unwrap();
    let save = change(0, Operation::Save, None, Some(content()));
    store
        .apply(&save, "publisher@example.invalid", NOW)
        .unwrap();
    let id = store
        .request_message(&save.request_id, "publisher@example.invalid")
        .unwrap()
        .unwrap();
    assert!(id.starts_with("announcement-"));
    drop(store);
    let mut store = Store::resume(&db, export).unwrap();
    store
        .apply(&save, "publisher@example.invalid", NOW)
        .unwrap();
    assert_eq!(store.snapshot().unwrap().revision, 1);
    assert_eq!(
        store
            .request_message(&save.request_id, "publisher@example.invalid")
            .unwrap(),
        Some(id)
    );
    assert_eq!(
        store
            .request_message(&save.request_id, "someone-else@example.invalid")
            .unwrap(),
        None
    );
}

#[test]
fn rotating_snapshots_restore_and_prune_only_their_own_files() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state/announcements.sqlite");
    let export = dir.path().join("public/announcements.json");
    let backups = dir.path().join("backups");
    fs::create_dir_all(&backups).unwrap();
    fs::write(backups.join("notes.txt"), "kept").unwrap();
    fs::write(backups.join("announcements-manual.sqlite"), "kept").unwrap();
    // What a killed earlier run leaves behind: never counted, and cleared afterwards.
    fs::write(
        backups.join("announcements-20261011T000000Z.sqlite.partial"),
        "torn",
    )
    .unwrap();
    let mut store = Store::open(&db, export, SEED).unwrap();
    let save = change(0, Operation::Save, None, Some(content()));
    store
        .apply(&save, "publisher@example.invalid", NOW)
        .unwrap();
    // The service keeps the database open while the timer takes snapshots.
    let written: Vec<_> = (0..3)
        .map(|day| {
            tennoworth_publisher::store::snapshot(&db, &backups, NOW + day * 86400, 2).unwrap()
        })
        .collect();
    let mut names: Vec<_> = fs::read_dir(&backups)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "announcements-20261013T000000Z.sqlite",
            "announcements-20261014T000000Z.sqlite",
            "announcements-manual.sqlite",
            "notes.txt"
        ]
    );
    assert!(!written[0].exists());
    assert!(tennoworth_publisher::store::snapshot(&db, &backups, NOW + 3 * 86400, 0).is_err());
    drop(store);
    let restored = dir.path().join("restored/announcements.sqlite");
    fs::create_dir_all(restored.parent().unwrap()).unwrap();
    fs::copy(&written[2], &restored).unwrap();
    let store = Store::resume(
        &restored,
        dir.path().join("restored-public/announcements.json"),
    );
    // A restored database without its export is a fresh host: it must start and keep the draft.
    let snapshot = store.unwrap().snapshot().unwrap();
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.messages.len(), 1);
}

fn stream() -> EventContent {
    EventContent {
        kind: "devstream".into(),
        title: "Devstream 200".into(),
        starts_at: "2026-10-23T19:00:00Z".into(),
        ends_at: None,
        source_url: "https://forums.warframe.com/topic/devstream-200/".into(),
        note: Some("".into()),
    }
}
fn event_change(
    revision: i64,
    operation: EventOperation,
    id: Option<String>,
    event: Option<EventContent>,
) -> EventChange {
    EventChange {
        request_id: format!("event-request-{revision:016}"),
        expected_revision: revision,
        operation,
        id,
        event,
    }
}

#[test]
fn official_events_publish_correct_and_remove_through_the_public_feed() {
    let dir = tempfile::tempdir().unwrap();
    let export = dir.path().join("public/announcements.json");
    let mut store = Store::open(&dir.path().join("state.sqlite"), export.clone(), SEED).unwrap();
    let actor = "publisher@example.invalid";
    let publish = event_change(0, EventOperation::Publish, None, Some(stream()));
    let live = store.apply_event(&publish, actor, NOW).unwrap();
    let events = live.feed.events.clone().unwrap();
    let id = events[0].id.clone();
    assert!(id.starts_with("event-"));
    // An empty note is not published as one.
    assert!(events[0].note.is_none());
    assert_eq!(live.feed_revision, live.exported_revision);
    let disk = tennoworth_announcements::parse(&fs::read_to_string(&export).unwrap()).unwrap();
    assert_eq!(disk.events.unwrap()[0].title, "Devstream 200");
    assert_eq!(live.history[0].operation, "event-publish");
    assert_eq!(live.history[0].event.as_ref().unwrap().kind, "devstream");
    assert!(live.history[0].content.is_none());

    // A retried request changes nothing; a stale one conflicts.
    assert_eq!(store.apply_event(&publish, actor, NOW).unwrap().revision, 1);
    assert!(matches!(
        store.apply_event(
            &event_change(0, EventOperation::Publish, None, Some(stream())),
            "other@example.invalid",
            NOW
        ),
        Err(Error::Conflict)
    ));

    // A correction keeps the ID.
    let corrected = EventContent {
        title: "Devstream 200: Update 41".into(),
        ..stream()
    };
    let live = store
        .apply_event(
            &event_change(
                1,
                EventOperation::Publish,
                Some(id.clone()),
                Some(corrected),
            ),
            actor,
            NOW,
        )
        .unwrap();
    let events = live.feed.events.unwrap();
    assert_eq!(
        (
            events.len(),
            events[0].id.as_str(),
            events[0].title.as_str()
        ),
        (1, id.as_str(), "Devstream 200: Update 41")
    );

    // Publishing a message keeps the events in the feed.
    let saved = store
        .apply(
            &change(2, Operation::Save, None, Some(content())),
            actor,
            NOW,
        )
        .unwrap();
    let message = saved.messages[0].id.clone();
    let live = store
        .apply(
            &change(3, Operation::Publish, Some(message), None),
            actor,
            NOW,
        )
        .unwrap();
    assert_eq!(live.feed.events.as_ref().map(Vec::len), Some(1));

    let removed = store
        .apply_event(
            &event_change(4, EventOperation::Remove, Some(id.clone()), None),
            actor,
            NOW,
        )
        .unwrap();
    assert_eq!(removed.feed.events.as_ref().map(Vec::len), Some(0));
    assert_eq!(removed.history[0].operation, "event-remove");
    assert!(matches!(
        store.apply_event(
            &event_change(5, EventOperation::Remove, Some(id), None),
            actor,
            NOW
        ),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn official_events_are_validated_and_old_ones_drop_out() {
    let dir = tempfile::tempdir().unwrap();
    let export = dir.path().join("public/announcements.json");
    let mut store = Store::open(&dir.path().join("state.sqlite"), export.clone(), SEED).unwrap();
    let actor = "publisher@example.invalid";
    for bad in [
        EventContent {
            kind: "rumour".into(),
            ..stream()
        },
        EventContent {
            source_url: "https://www.reddit.com/r/Warframe/".into(),
            ..stream()
        },
        EventContent {
            ends_at: Some("2027-03-01T00:00:00Z".into()),
            ..stream()
        },
        EventContent {
            starts_at: "2026-10-01T00:00:00Z".into(),
            ..stream()
        },
    ] {
        assert!(matches!(
            store.apply_event(
                &event_change(0, EventOperation::Publish, None, Some(bad)),
                actor,
                NOW
            ),
            Err(Error::Invalid(_))
        ));
    }
    assert_eq!(fs::read_to_string(&export).unwrap(), SEED);
    store
        .apply_event(
            &event_change(0, EventOperation::Publish, None, Some(stream())),
            actor,
            NOW,
        )
        .unwrap();
    // Eight days after the stream it is pruned on the next publication.
    let later = NOW + 20 * 86_400;
    let next = EventContent {
        starts_at: "2026-11-18T15:00:00Z".into(),
        kind: "update".into(),
        title: "Update 41".into(),
        ..stream()
    };
    let live = store
        .apply_event(
            &event_change(1, EventOperation::Publish, None, Some(next)),
            actor,
            later,
        )
        .unwrap();
    let titles: Vec<_> = live
        .feed
        .events
        .unwrap()
        .into_iter()
        .map(|e| e.title)
        .collect();
    assert_eq!(titles, ["Update 41"]);
}
