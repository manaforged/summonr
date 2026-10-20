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
