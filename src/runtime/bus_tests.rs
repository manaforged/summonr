#[test]
fn an_empty_inbox_has_an_explicit_result() {
    assert_eq!(super::inbox(Vec::new()), "no new mail");
}
