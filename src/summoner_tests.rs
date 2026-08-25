#[test]
fn alias_keeps_the_shared_entry_point() {
    let _entry: fn() = super::main;
    assert_eq!(option_env!("CARGO_BIN_NAME"), Some("summoner"));
}
