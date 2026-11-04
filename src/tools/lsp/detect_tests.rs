use super::*;

fn plugin_claiming(extensions: &[&str]) -> Server {
    Server {
        key: "demo:all".to_string(),
        argv: vec![std::env::current_exe().unwrap().display().to_string()],
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

#[test]
fn a_lean_project_selects_its_language_module()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    std::fs::write(
        workspace.path().join("lean-toolchain"),
        "leanprover/lean4:stable",
    )?;
    let file = workspace.path().join("proof.lean");
    std::fs::write(&file, "theorem identity : True := by trivial\n")?;
    let module = language::detect(workspace.path(), &file)?;
    assert_eq!(module.id, language::Language::Lean);
    assert_eq!(language::language_id(&file), "lean4");
    Ok(())
}

#[test]
fn marker_rooted_servers_are_cached_per_project()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    let first = workspace.path().join("first");
    let second = workspace.path().join("second");
    for root in [&first, &second] {
        std::fs::create_dir(root)?;
        std::fs::write(root.join("lean-toolchain"), "leanprover/lean4:stable")?;
    }
    let module = language::Module {
        id: language::Language::Lean,
        markers: vec!["lean-toolchain".to_string()],
        root_at_marker: true,
        servers: vec![language::Server {
            name: std::env::current_exe()?.to_string_lossy().into_owned(),
            args: Vec::new(),
        }],
        commands: Vec::new(),
        options: json!({}),
    };
    let launches = Launches::default();
    let first_launch = launches.built(&module, workspace.path(), &first.join("proof.lean"))?;
    let first_again = launches.built(&module, workspace.path(), &first.join("other.lean"))?;
    let second_launch = launches.built(&module, workspace.path(), &second.join("proof.lean"))?;
    assert_eq!(first_launch.root.as_deref(), Some(first.as_path()));
    assert_eq!(second_launch.root.as_deref(), Some(second.as_path()));
    assert_eq!(first_launch.key, second_launch.key);
    assert_ne!(first_launch.root, second_launch.root);
    assert!(Arc::ptr_eq(&first_launch, &first_again));
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let left = workspace
            .path()
            .join(std::ffi::OsString::from_vec(vec![b'p', 0x80]));
        let right = workspace
            .path()
            .join(std::ffi::OsString::from_vec(vec![b'p', 0x81]));
        assert_eq!(left.display().to_string(), right.display().to_string());
        let mut slots = BTreeMap::new();
        slots.insert((first_launch.key.clone(), Some(left)), 1);
        slots.insert((first_launch.key.clone(), Some(right)), 2);
        assert_eq!(slots.len(), 2);
    }
    Ok(())
}
