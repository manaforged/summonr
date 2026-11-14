#[test]
fn a_secret_written_after_a_scan_is_hidden_on_the_next_result() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(
        workspace.path().join(".env"),
        "API_KEY=first-secret-value\n",
    )
    .unwrap();
    assert_eq!(
        super::scrub("got first-secret-value", workspace.path()),
        "got [secret]"
    );
    let nested = workspace.path().join("app");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join(".env.local"), "TOKEN=second-secret-value\n").unwrap();
    assert_eq!(
        super::scrub("second-secret-value", workspace.path()),
        "[secret]"
    );
    std::fs::write(
        workspace.path().join(".env"),
        "API_KEY=third-secret-value!\n",
    )
    .unwrap();
    assert_eq!(
        super::scrub("third-secret-value!", workspace.path()),
        "[secret]"
    );
}

#[test]
fn an_unchanged_workspace_reuses_its_scan() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join(".env"), "API_KEY=kept-secret-value\n").unwrap();
    assert_eq!(
        super::scrub("kept-secret-value", workspace.path()),
        "[secret]"
    );
    let reused = super::known(workspace.path());
    assert!(std::sync::Arc::ptr_eq(
        &reused,
        &super::known(workspace.path())
    ));
}

#[test]
fn a_secret_rewritten_within_the_clock_tick_of_a_scan_is_hidden() {
    let workspace = tempfile::tempdir().unwrap();
    let env = workspace.path().join(".env");
    std::fs::write(&env, "API_KEY=first-secret-value\n").unwrap();
    let written = std::fs::metadata(&env).unwrap().modified().unwrap();
    assert_eq!(
        super::scrub("first-secret-value", workspace.path()),
        "[secret]"
    );
    std::fs::write(&env, "API_KEY=other-secret-value\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&env)
        .unwrap()
        .set_modified(written)
        .unwrap();
    assert_eq!(
        super::scrub("other-secret-value", workspace.path()),
        "[secret]"
    );
}
