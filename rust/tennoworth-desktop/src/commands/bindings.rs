use market_domain::bindings::TypeScript;

#[test]
fn desktop_bindings_match_rust() {
    let mut types = TypeScript::default();
    types.add::<super::report::ScanReport>();
    types.add::<wfm_client::governor::AccessStatus>();
    types.add::<crate::services::wfm_session::CmdError>();
    types.add::<crate::services::watch::WatchOutcome>();
    let mut expected = types.finish().expect("unique wire types");
    expected.push_str(&format!(
        "\nexport const WATCH_FIRED_EVENT = {:?} as const;\n",
        crate::services::watch::EVENT_WATCH_FIRED
    ));
    expected.push_str(&format!("\nexport const WFM_ACCESS_EVENT = {:?} as const;\n", crate::services::wfm_session::WFM_ACCESS_EVENT));
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
    let error = crate::services::wfm_session::CmdError::needs_login();
    let json = serde_json::to_value(error).expect("error json");
    assert_eq!(json["code"], "needs_login");
    assert!(json["message"].is_string());
    assert_eq!(json.as_object().expect("error object").len(), 2);
}
