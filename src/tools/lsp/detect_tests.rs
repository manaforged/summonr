use super::*;

fn plugin_claiming(extensions: &[&str]) -> Server {
    Server {
        key: "demo:all".to_string(),
        argv: vec!["sh".to_string()],
        env: BTreeMap::new(),
        languages: extensions
            .iter()
            .map(|extension| ((*extension).to_string(), (*extension).to_string()))
            .collect(),
    }
}

#[test]
fn a_plugin_server_never_takes_a_file_a_built_in_module_handles() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\n",
    )
    .unwrap();
    let launches = Launches {
        source: Mutex::new(Some(Source::Tree(Arc::from(vec![plugin_claiming(&[
            "rs", "svelte",
        ])])))),
        built: Mutex::default(),
    };
    let rust = launches.launch(
        workspace.path(),
        &workspace.path().join("main.rs"),
        Scope::Request,
    );
    assert!(!rust.is_ok_and(|launch| launch.key == "demo:all"));
    let svelte = launches
        .launch(
            workspace.path(),
            &workspace.path().join("App.svelte"),
            Scope::Request,
        )
        .unwrap();
    assert_eq!(svelte.key, "demo:all");
    assert_eq!(svelte.origin, Origin::Plugin);
}
