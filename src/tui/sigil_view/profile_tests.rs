#[tokio::test(flavor = "multi_thread")]
async fn a_profile_run_records_the_workload_shape() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("trace.json");
    let args = [
        "--frames",
        "2",
        "--history",
        "3",
        "--fps",
        "240",
        "--output",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([output.display().to_string()]);
    super::run(args).unwrap();
    let trace = std::fs::read_to_string(&output).unwrap();
    for name in [
        "app.update",
        "workload.items",
        "workload.headless",
        "workload.fps",
        "workload.scenario",
        "build.profile",
    ] {
        assert!(trace.contains(name), "{name} missing from the trace");
    }
}
