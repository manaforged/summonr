use super::*;

#[test]
fn the_default_prompt_does_not_steer_the_model() {
    let workspace = tempfile::tempdir().unwrap();
    let prompt = compose(workspace.path(), None, Sources::default());
    for steering in [
        "<work>",
        "<persistence>",
        "contract",
        "Finish the authorized task",
    ] {
        assert!(!prompt.contains(steering), "{steering}: {prompt}");
    }
}

#[test]
fn the_default_prompt_names_the_module_surface() {
    let workspace = tempfile::tempdir().unwrap();
    let prompt = compose(workspace.path(), None, Sources::default());
    for capability in [
        "web (search, fetch)",
        "mail (agents, send, inbox, board)",
        "`summonr modules`",
    ] {
        assert!(prompt.contains(capability), "{capability}: {prompt}");
    }
}

#[test]
fn an_at_include_pulls_the_referenced_file_in_once() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("AGENTS.md"), "AGENTS RULE").unwrap();
    std::fs::create_dir(workspace.path().join("docs")).unwrap();
    std::fs::write(workspace.path().join("docs/map.md"), "MAP RULE").unwrap();
    std::fs::write(
        workspace.path().join("CLAUDE.md"),
        "@AGENTS.md\n@docs/map.md\n\nCLAUDE RULE",
    )
    .unwrap();
    let prompt = compose(
        workspace.path(),
        None,
        Sources {
            trust: true,
            ..Sources::default()
        },
    );
    assert!(prompt.contains("MAP RULE"), "{prompt}");
    assert!(prompt.contains("CLAUDE RULE"), "{prompt}");
    assert_eq!(prompt.matches("AGENTS RULE").count(), 1, "{prompt}");
}

#[test]
fn a_missing_at_include_stays_a_literal_line() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("CLAUDE.md"), "@NOPE.md").unwrap();
    let prompt = compose(
        workspace.path(),
        None,
        Sources {
            trust: true,
            ..Sources::default()
        },
    );
    assert!(prompt.contains("@NOPE.md"), "{prompt}");
}

#[test]
fn at_include_cycles_terminate() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("AGENTS.md"), "@CLAUDE.md").unwrap();
    std::fs::write(workspace.path().join("CLAUDE.md"), "@AGENTS.md").unwrap();
    let prompt = compose(
        workspace.path(),
        None,
        Sources {
            trust: true,
            ..Sources::default()
        },
    );
    assert!(
        prompt.contains("@CLAUDE.md") || prompt.contains("@AGENTS.md"),
        "{prompt}"
    );
}

#[test]
fn a_workspace_system_file_keeps_the_preamble() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("SYSTEM.md"), "WORKSPACE RULE").unwrap();
    let prompt = compose(
        workspace.path(),
        None,
        Sources {
            trust: true,
            ..Sources::default()
        },
    );
    assert!(
        prompt.contains("You are an expert coding assistant running inside summonr"),
        "{prompt}"
    );
    assert!(prompt.contains("WORKSPACE RULE"), "{prompt}");
}
