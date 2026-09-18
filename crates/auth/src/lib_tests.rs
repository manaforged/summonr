use super::*;

struct Memory(Mutex<std::collections::BTreeMap<String, String>>);

impl Store for Memory {
    fn get(&self, name: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(name).cloned())
    }
    fn set(&self, name: &str, secret: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(name.to_string(), secret.to_string());
        Ok(())
    }
    fn forget(&self, name: &str) -> Result<bool> {
        Ok(self.0.lock().unwrap().remove(name).is_some())
    }
}

fn store() -> Arc<dyn Store> {
    Arc::new(Memory(Mutex::new(std::collections::BTreeMap::new())))
}

#[test]
fn a_key_endpoint_needs_no_login_and_is_ready() {
    let auth = Auth::new(Method::Key("k".into()), Scheme::Bearer, "zai", store());
    assert!(!auth.is_login());
    assert!(auth.ready().unwrap());
}

#[test]
fn a_login_endpoint_is_not_ready_until_something_is_stored() {
    let kept = store();
    let spec = Spec {
        token_url: "https://example.invalid/token".into(),
        client_id: "id".into(),
        grant: Grant::DeviceCode,
        device_url: Some("https://example.invalid/device".into()),
        ..Spec::default()
    };
    let auth = Auth::new(Method::Login(spec), Scheme::Bearer, "copilot", kept.clone());
    assert!(auth.is_login());
    assert!(!auth.ready().unwrap());
    kept.set(
        "copilot",
        r#"{"access_token":"t","refresh_token":"r","expires_at":9999999999}"#,
    )
    .unwrap();
    assert!(auth.ready().unwrap());
}

#[test]
fn each_wire_puts_the_credential_where_that_wire_wants_it() {
    assert_eq!(Scheme::default(), Scheme::Bearer);
    for scheme in [Scheme::Bearer, Scheme::ApiKey, Scheme::GoogleKey] {
        let auth = Auth::new(Method::Key("k".into()), scheme, "x", store());
        assert_eq!(auth.scheme(), scheme);
    }
}

#[test]
fn a_login_spec_states_what_it_is_missing() {
    let device = Spec {
        grant: Grant::DeviceCode,
        ..Spec::default()
    };
    assert!(
        device
            .check()
            .unwrap_err()
            .to_string()
            .contains("device_url"),
        "a device login without device_url must say so"
    );
    let refreshing = Spec::default();
    assert!(
        refreshing
            .check()
            .unwrap_err()
            .to_string()
            .contains("refresh_token")
    );
}

#[test]
fn a_stored_token_round_trips_through_the_store_format() {
    let issued =
        oauth::parse_stored(r#"{"access_token":"a","refresh_token":"r","expires_at":9999999999}"#)
            .expect("a stored token parses");
    assert_eq!(issued.access, "a");
    assert_eq!(issued.refresh.as_deref(), Some("r"));
    assert!(issued.fresh(), "a token far in the future is usable");
    assert!(oauth::parse_stored("not json").is_none());
}

#[test]
fn a_token_that_expires_within_the_skew_is_not_fresh() {
    let soon = Issued {
        access: "a".into(),
        refresh: None,
        expires: Some(Instant::now() + Duration::from_secs(5)),
    };
    assert!(
        !soon.fresh(),
        "a token expiring inside the skew must be refreshed, not used"
    );
}
