use super::*;

#[test]
fn polyglot_workspace_selects_by_file() -> Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("Cargo.toml"), "[workspace]\n")?;
    std::fs::write(directory.path().join("package.json"), "{}\n")?;
    std::fs::write(directory.path().join("pyproject.toml"), "[project]\n")?;
    std::fs::write(directory.path().join("go.mod"), "module test\n")?;
    let rust = directory.path().join("src/main.rs");
    let typescript = directory.path().join("web/app.ts");
    let python = directory.path().join("scripts/check.py");
    let go = directory.path().join("cmd/main.go");

    assert_eq!(detect(directory.path(), &rust)?.id, "rust");
    assert_eq!(detect(directory.path(), &typescript)?.id, "typescript");
    assert_eq!(detect(directory.path(), &python)?.id, "python");
    assert_eq!(detect(directory.path(), &go)?.id, "go");
    Ok(())
}

#[test]
fn nested_marker_selects_module() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("services/api");
    std::fs::create_dir_all(package.join("src"))?;
    std::fs::write(package.join("pyproject.toml"), "[project]\nname='api'\n")?;

    assert_eq!(
        detect(directory.path(), &package.join("src/main.py"))?.id,
        "python"
    );
    Ok(())
}

#[test]
fn rust_module_disables_implicit_builds() -> Result<()> {
    let options = options(named("rust")?);

    assert_eq!(options["checkOnSave"], false);
    assert_eq!(options["cargo"]["allTargets"], false);
    assert_eq!(options["cargo"]["buildScripts"]["enable"], false);
    assert_eq!(options["procMacro"]["enable"], false);
    Ok(())
}

#[test]
fn unknown_module_is_rejected() {
    let error = named("java").expect_err("unknown module must fail");

    assert!(error.to_string().contains("unknown language module `java`"));
}

#[test]
fn present_finds_markers_at_the_root_and_one_level_down() -> Result<()> {
    let workspace = tempfile::tempdir()?;
    std::fs::write(workspace.path().join("go.mod"), "module x\n")?;
    std::fs::create_dir(workspace.path().join("api"))?;
    std::fs::write(workspace.path().join("api/Cargo.toml"), "[package]\n")?;
    std::fs::create_dir_all(workspace.path().join("deep/er"))?;
    std::fs::write(workspace.path().join("deep/er/package.json"), "{}")?;
    let ids: Vec<&str> = present(workspace.path())
        .iter()
        .map(|module| module.id)
        .collect();
    assert_eq!(ids, ["rust", "go"]);
    Ok(())
}
