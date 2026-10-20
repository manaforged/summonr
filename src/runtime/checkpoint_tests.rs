use super::*;
use crate::provider::scripted::Scripted;

#[tokio::test]
async fn settling_a_turn_returns_the_workspace_snapshot() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let driver =
        super::super::configure::open_driver(home.path(), workspace.path(), "m", "system").unwrap();
    let mut loop_ = Loop::open(Scripted::new([]), "m", workspace.path(), "system", driver).unwrap();
    let mut pre = loop_.checkpoint();
    let mut events = Vec::new();
    let snapshot = loop_
        .settle(&mut pre, &mut events, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(snapshot, crate::diff::snapshot(workspace.path()));
    assert_eq!(pre.1, snapshot);
}
