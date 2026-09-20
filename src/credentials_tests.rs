use super::*;

fn row() -> crate::config::OAuth {
    crate::config::OAuth {
        token_url: "https://example.invalid/token".to_string(),
        client_id: "id".to_string(),
        client_secret: None,
        refresh_token: None,
        scope: None,
        grant: Some("device_code".to_string()),
        device_url: Some("https://example.invalid/device".to_string()),
        exchange_url: None,
        ..crate::config::OAuth::default()
    }
}

#[test]
fn each_wire_carries_its_credential_where_that_wire_wants_it() {
    assert_eq!(scheme(Wire::Anthropic), Scheme::ApiKey);
    assert_eq!(scheme(Wire::Gemini), Scheme::GoogleKey);
    assert_eq!(scheme(Wire::Completions), Scheme::Bearer);
    assert_eq!(scheme(Wire::Decisions), Scheme::Bearer);
}

#[test]
fn a_login_row_becomes_a_spec_with_its_grant() {
    let built = spec(&row()).unwrap();
    assert_eq!(built.grant, Grant::DeviceCode);
    assert_eq!(
        built.device_url.as_deref(),
        Some("https://example.invalid/device")
    );
}

#[test]
fn a_login_row_missing_its_url_is_refused_before_any_request() {
    let mut bad = row();
    bad.device_url = None;
    let error = spec(&bad).unwrap_err().to_string();
    assert!(error.contains("device_url"), "got {error}");
    let mut empty = row();
    empty.client_id = String::new();
    assert!(
        spec(&empty).is_err(),
        "a login without client_id is refused"
    );
}

#[test]
fn a_row_with_no_grant_named_is_a_refresh_login() {
    let mut plain = row();
    plain.grant = None;
    plain.device_url = None;
    plain.refresh_token = Some("r".to_string());
    assert_eq!(spec(&plain).unwrap().grant, Grant::RefreshToken);
    assert_eq!(flow(&plain), LoginFlow::Key);
    assert_eq!(flow(&row()), LoginFlow::Device);
}
