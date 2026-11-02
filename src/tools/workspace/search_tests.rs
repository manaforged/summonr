use super::*;

#[test]
fn a_cancelled_search_halts_its_walk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "needle\n").unwrap();
    let tools =
        ToolRegistry::with_policy_and_search(dir.path(), crate::policy::Policy::builtin(), None)
            .unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let job = tools
        .search_job(
            &serde_json::json!({"pattern": "needle"}),
            Arc::clone(&cancelled),
        )
        .unwrap();
    drop(CancelOnDrop(cancelled));
    let text = job.run().unwrap();
    assert!(text.contains("stopped after"), "{text}");
    assert!(!text.contains("a.txt"), "{text}");
}
