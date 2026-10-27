#[test]
fn a_fallback_that_does_not_resolve_is_reported_as_skipped() {
    let mut endpoint =
        super::super::resolve(Some("zai"), Some("glm-5.3-flash"), crate::Need::Config).unwrap();
    endpoint.fallback = vec!["no-such-fallback-seat".to_string()];
    let (ready, skipped) = super::fallbacks(&endpoint, |_| Ok(()));
    assert!(ready.is_empty());
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert!(
        skipped[0].starts_with("fallback no-such-fallback-seat skipped: "),
        "{skipped:?}"
    );
}

fn grok() -> crate::Endpoint {
    super::super::keys::builtin("grok").expect("grok preset")
}

#[test]
fn a_remote_endpoint_takes_the_catalog_window() {
    assert_eq!(
        super::with_catalog_window(grok()).context_window,
        Some(500_000)
    );
}

#[test]
fn a_remote_endpoint_without_a_key_still_takes_the_catalog_window() {
    let mut endpoint = grok();
    endpoint.api_key = None;
    assert_eq!(
        super::with_catalog_window(endpoint).context_window,
        Some(500_000)
    );
}

#[test]
fn a_local_url_stays_unset_so_the_probe_runs() {
    let mut endpoint = grok();
    endpoint.base_url = Some("http://127.0.0.1:8080/v1".to_string());
    endpoint.api_key = Some("present".to_string());
    assert_eq!(super::with_catalog_window(endpoint).context_window, None);
}

#[test]
fn a_catalog_miss_stays_unset() {
    let mut endpoint = grok();
    endpoint.model = "no-such-model".to_string();
    assert_eq!(super::with_catalog_window(endpoint).context_window, None);
}

#[test]
fn an_explicit_window_is_kept() {
    let mut endpoint = grok();
    endpoint.context_window = Some(42);
    assert_eq!(
        super::with_catalog_window(endpoint).context_window,
        Some(42)
    );
}
