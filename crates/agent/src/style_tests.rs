use std::path::Path;

use super::*;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn roots(project: Vec<PathBuf>, user: Option<PathBuf>, plugins: Vec<Plugin>) -> Roots {
    Roots {
        project,
        user,
        plugins,
    }
}

fn names(styles: &[Style]) -> Vec<(&str, &Origin)> {
    styles
        .iter()
        .map(|style| (style.name.as_str(), &style.origin))
        .collect()
}

#[test]
fn styles_load_from_project_user_and_plugin_with_their_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("work/.claude/output-styles");
    let user = dir.path().join("home/.claude/output-styles");
    write(
        &project.join("diagrams.md"),
        "---\nname: Diagrams first\ndescription: Lead every explanation with a diagram\nkeep-coding-instructions: true\n---\n\nStart with a Mermaid diagram.\n",
    );
    write(&user.join("terse.md"), "Answer in one line.\n");
    write(&user.join("notes.txt"), "not a style");
    let plugin_dir = dir.path().join("plugins/kit");
    write(
        &plugin_dir.join("output-styles/brief.md"),
        "---\nforce-for-plugin: true\n---\nBe brief.\n",
    );
    let found = roots(
        vec![project],
        Some(user),
        vec![Plugin::at("kit".to_string(), plugin_dir)],
    )
    .styles();
    assert_eq!(
        names(&found),
        [
            ("Diagrams first", &Origin::Project),
            ("terse", &Origin::User),
            ("brief", &Origin::Plugin("kit".to_string())),
        ]
    );
    assert_eq!(
        found[0].description,
        "Lead every explanation with a diagram"
    );
    assert_eq!(found[0].instructions, "Start with a Mermaid diagram.");
    assert!(found[0].keep_coding && !found[0].force);
    assert!(!found[1].keep_coding && found[1].description.is_empty());
    assert!(found[2].force);
}

#[test]
fn the_closest_project_style_wins_a_shared_name() {
    let dir = tempfile::tempdir().unwrap();
    let near = dir.path().join("repo/app/.claude/output-styles");
    let far = dir.path().join("repo/.claude/output-styles");
    let user = dir.path().join("home/.claude/output-styles");
    write(&near.join("voice.md"), "NEAR");
    write(&far.join("voice.md"), "FAR");
    write(&user.join("voice.md"), "USER");
    let found = roots(vec![near, far], Some(user), Vec::new()).styles();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].instructions, "NEAR");
}

#[test]
fn a_plugin_manifest_path_replaces_the_default_folder() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join("kit");
    write(&plugin_dir.join("output-styles/skipped.md"), "SKIPPED");
    write(&plugin_dir.join("styles/listed.md"), "LISTED");
    write(&plugin_dir.join("one.md"), "ONE");
    write(
        &plugin_dir.join(".claude-plugin/plugin.json"),
        r#"{"name":"kit","outputStyles":["./styles/","./one.md"]}"#,
    );
    let found = roots(
        Vec::new(),
        None,
        vec![Plugin::at("kit".to_string(), plugin_dir)],
    )
    .styles();
    let listed: Vec<&str> = found.iter().map(|style| style.name.as_str()).collect();
    assert_eq!(listed, ["listed", "one"]);
}

#[test]
fn force_for_plugin_counts_only_on_plugin_styles() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("output-styles");
    write(
        &user.join("loud.md"),
        "---\nforce-for-plugin: true\n---\nLOUD",
    );
    let found = roots(Vec::new(), Some(user), Vec::new()).styles();
    assert!(!found[0].force);
    assert_eq!(chosen(&found, None), Pick::Default);
}

#[test]
fn an_untrusted_workspace_reads_no_project_styles() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join(".claude/output-styles/local.md"),
        "PROJECT",
    );
    assert!(Roots::of(dir.path(), false).project.is_empty());
    let trusted = Roots::of(dir.path(), true);
    assert!(
        trusted
            .project
            .iter()
            .any(|root| root.join("local.md").is_file()),
        "{:?}",
        trusted.project
    );
}

fn style(name: &str, origin: Origin, force: bool) -> Style {
    Style {
        name: name.to_string(),
        description: String::new(),
        instructions: format!("{name} rules"),
        keep_coding: false,
        force,
        origin,
    }
}

#[test]
fn the_setting_matches_a_name_exactly_and_a_forcing_plugin_overrides_it() {
    let styles = vec![
        style("Explain", Origin::User, false),
        style("Terse", Origin::Plugin("a".to_string()), false),
    ];
    assert_eq!(chosen(&styles, Some("Explain")).name(), "Explain");
    assert_eq!(chosen(&styles, Some("explain")), Pick::Default);
    assert_eq!(chosen(&styles, None), Pick::Default);
    let mut forced = styles.clone();
    forced.push(style("First", Origin::Plugin("b".to_string()), true));
    forced.push(style("Second", Origin::Plugin("c".to_string()), true));
    assert_eq!(chosen(&forced, Some("Explain")).name(), "First");
}

#[test]
fn the_command_matches_names_with_case_ignored_and_lists_the_current_one() {
    let styles = vec![Style {
        description: "Say why".to_string(),
        ..style("Explain", Origin::User, false)
    }];
    assert_eq!(named(&styles, "explain").unwrap().name(), "Explain");
    assert_eq!(named(&styles, "DEFAULT"), Some(Pick::Default));
    assert_eq!(named(&styles, "missing"), None);
    assert_eq!(
        listing(&styles, "Explain"),
        "  default  no output style\n* Explain  Say why"
    );
    assert_eq!(
        listing(&styles, DEFAULT),
        "* default  no output style\n  Explain  Say why"
    );
}

#[test]
fn a_switch_waits_for_the_next_turn_and_the_next_history_rewrite() {
    let mut switch = Switch::default();
    let system = "intro\n\n<cwd>/w</cwd>";
    assert_eq!(switch.current(system), DEFAULT);
    assert_eq!(switch.rewritten(system), None);
    let chosen = style("Explain", Origin::User, false);
    switch.choose(Pick::Custom(chosen.clone()));
    assert_eq!(switch.current(system), "Explain");
    let note = switch.take_note().unwrap();
    assert!(note.contains("Explain rules"), "{note}");
    assert_eq!(switch.take_note(), None);
    switch.choose(Pick::Custom(chosen.clone()));
    let rebuilt = switch.rewritten(system).unwrap();
    assert_eq!(rebuilt, crate::prompt::restyle(system, Some(&chosen)));
    assert_eq!(crate::prompt::style_name(&rebuilt), Some("Explain"));
    assert_eq!(switch.take_note(), None);
}
