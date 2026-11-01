#[tokio::test(flavor = "multi_thread")]
async fn ui_capture_reproduces_scenes_and_rejects_missing_content() {
    let dir = tempfile::tempdir().unwrap();
    for scenario in ["help", "agents", "agents-failed"] {
        let mut previous = None;
        for attempt in 0..2 {
            let output = dir.path().join(format!("{scenario}-{attempt}.json"));
            let args = [
                scenario,
                "--frames",
                "1",
                "--history",
                "0",
                "--fps",
                "240",
                "--output",
            ]
            .into_iter()
            .map(str::to_owned)
            .chain([output.display().to_string()]);
            super::run(args).unwrap();
            let text = std::fs::read_to_string(output.with_extension("txt")).unwrap();
            assert!(
                !std::fs::read(output.with_extension("ansi"))
                    .unwrap()
                    .is_empty()
            );
            if let Some(previous) = previous {
                assert_eq!(text, previous);
            }
            previous = Some(text);
            #[cfg(feature = "gui")]
            {
                let image = image::open(output.with_extension("png"))
                    .unwrap()
                    .to_rgba8();
                assert_eq!(image.dimensions(), (1200, 800));
                if attempt == 1 {
                    let first = image::open(dir.path().join(format!("{scenario}-0.png")))
                        .unwrap()
                        .to_rgba8();
                    assert_eq!(image, first);
                }
            }
        }
    }
    let output = dir.path().join("clipped.json");
    let args = [
        "agents",
        "--frames",
        "1",
        "--history",
        "0",
        "--cols",
        "40",
        "--rows",
        "10",
        "--output",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([output.display().to_string()]);
    let error = super::run(args).unwrap_err().to_string();
    assert!(
        error.contains("UI capture missing expected content"),
        "{error}"
    );
    assert!(!output.with_extension("png").exists());
    assert!(!output.with_extension("txt").exists());
}

#[cfg(not(feature = "gui"))]
#[test]
fn requesting_png_without_the_renderer_fails_before_capture() {
    let error = super::run(["--output", "capture.png"].into_iter().map(str::to_owned))
        .unwrap_err()
        .to_string();
    assert!(error.contains("--features gui"), "{error}");
}

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
