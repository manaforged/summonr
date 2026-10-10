#[test]
fn one_epoch_scans_once_and_a_new_epoch_rescans() {
    let workspace = tempfile::tempdir().unwrap();
    let env = workspace.path().join(".env");
    std::fs::write(&env, "API_KEY=first-secret-value\n").unwrap();
    let shown = super::scrub("got first-secret-value", workspace.path(), 1);
    assert_eq!(shown, "got [secret]");
    std::fs::write(&env, "API_KEY=second-secret-value\n").unwrap();
    let cached = super::scrub(
        "first-secret-value second-secret-value",
        workspace.path(),
        1,
    );
    assert_eq!(cached, "[secret] second-secret-value");
    let fresh = super::scrub(
        "first-secret-value second-secret-value",
        workspace.path(),
        2,
    );
    assert_eq!(fresh, "first-secret-value [secret]");
}
