use market_domain::bindings::TypeScript;

#[test]
fn desktop_bindings_match_rust() {
    let mut types = TypeScript::default();
    types.add::<wfm_client::governor::AccessStatus>();
    types.add::<crate::command_error::CmdError>();
    types.add::<crate::services::watch::WatchOutcome>();
    types.add::<crate::services::auto_scan::AutoScanSettings>();
    types.add::<crate::services::auto_scan::AutoScanStatus>();
    types.add::<super::listing::OwnOrder>();
    types.add::<crate::notification_contract::NotificationContent>();
    types.add::<crate::shell::app_icon::AppIconOutcome>();
    let mut expected = types.finish().expect("unique wire types");
    expected.push_str(&format!(
        "\nexport const WATCH_FIRED_EVENT = {:?} as const;\n",
        crate::services::watch::EVENT_WATCH_FIRED
    ));
    expected.push_str(&format!("\nexport const WFM_ACCESS_EVENT = {:?} as const;\n", crate::commands::auth::WFM_ACCESS_EVENT));
    expected.push_str(&format!(
        "\nexport const INVENTORY_SCANNED_EVENT = {:?} as const;\n",
        crate::services::acquisition::EVENT_INVENTORY_SCANNED
    ));
    // Every other Rust-emitted event name the webview listens for. A name typed
    // on each side compiled and type-checked while the listener never fired.
    for (name, value) in [
        ("TRADE_DETECTED_EVENT", crate::services::trades::EVENT_TRADE_DETECTED),
        ("RECORDING_CHANGED_EVENT", crate::services::trades::EVENT_RECORDING_CHANGED),
        ("ALLOWANCE_CHANGED_EVENT", crate::services::allowance::EVENT_ALLOWANCE_CHANGED),
        ("NOTIFICATIONS_EVENT", crate::services::notifications::EVENT),
        ("MARKET_REFRESHED_EVENT", crate::services::reminders::MARKET_EVENT),
        ("LIVE_TOP_PROGRESS_EVENT", super::market::EVENT_LIVE_TOP_PROGRESS),
        ("TRAY_HINT_EVENT", crate::shell::tray::EVENT_TRAY_HINT),
        ("UPDATE_AVAILABLE_EVENT", crate::shell::update::EVENT_UPDATE_AVAILABLE),
        ("RELIC_OVERLAY_UPDATE_EVENT", crate::overlay::EVENT_UPDATE),
        ("RELIC_OVERLAY_HIDE_EVENT", crate::overlay::EVENT_HIDE),
    ] {
        expected.push_str(&format!("\nexport const {name} = {value:?} as const;\n"));
    }
    expected.push_str(&format!(
        "\nexport const RELIC_RECOMMENDATION_CONFIDENCE = {:?} as const;\n",
        crate::overlay::RECOMMENDATION_CONFIDENCE
    ));
    // The cadences the setting offers live in Rust; the selector reads them from
    // here rather than repeating the list, so the two cannot drift.
    expected.push_str(&format!(
        "\nexport const AUTO_SCAN_CADENCE_CHOICES = {:?} as const;\n",
        crate::services::auto_scan::CADENCE_CHOICES
    ));
    // The colours set_app_icon accepts; the setting resolves its choice to one.
    expected.push_str(&format!(
        "\nexport const APP_ICON_COLOURS = {:?} as const;\n",
        crate::shell::app_icon::COLOURS
    ));
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../frontend/src/contracts/generated/desktop.ts");
    if std::env::var_os("TENNOWORTH_UPDATE_BINDINGS").is_some() {
        std::fs::create_dir_all(file.parent().expect("parent")).expect("binding directory");
        std::fs::write(&file, &expected).expect("write bindings");
    }
    let actual = std::fs::read_to_string(&file).expect("export bindings with TENNOWORTH_UPDATE_BINDINGS=1 cargo test -p tennoworth-desktop desktop_bindings_match_rust");
    assert_eq!(
        actual, expected,
        "Rust wire contracts changed; regenerate TypeScript bindings"
    );
}

#[test]
fn auth_error_serialization_keeps_the_frontend_contract() {
    let error = crate::command_error::CmdError::needs_login();
    let json = serde_json::to_value(error).expect("error json");
    assert_eq!(json["code"], "needs_login");
    assert!(json["message"].is_string());
    assert_eq!(json.as_object().expect("error object").len(), 2);
}
