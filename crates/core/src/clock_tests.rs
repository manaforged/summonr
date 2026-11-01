#[test]
fn now_is_after_the_2020_epoch() {
    let seconds = super::now();
    let millis = super::now_millis();
    assert!(seconds > 1_577_836_800);
    assert!(millis > 1_577_836_800_000);
    assert!(millis >= seconds * 1000);
}
