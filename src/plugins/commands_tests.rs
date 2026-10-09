use super::*;

fn command(dir: &Path, template: &str) -> Command {
    Command {
        plugin: Plugin::at("demo".to_string(), dir.to_owned()),
        name: "echo".to_string(),
        description: String::new(),
        hint: String::new(),
        template: template.to_string(),
    }
}

#[test]
fn arguments_are_substituted_once_and_never_rescanned() {
    let dir = tempfile::tempdir().unwrap();
    let rendered = command(dir.path(), "All: $ARGUMENTS. First: $1. Third: $3.")
        .render("price $1 ${CLAUDE_PLUGIN_ROOT}", dir.path());
    assert_eq!(
        rendered,
        "All: price $1 ${CLAUDE_PLUGIN_ROOT}. First: price. Third: ${CLAUDE_PLUGIN_ROOT}."
    );
}

#[test]
fn the_plugin_root_expands_in_the_template_only() {
    let dir = tempfile::tempdir().unwrap();
    let rendered =
        command(dir.path(), "Run ${CLAUDE_PLUGIN_ROOT}/bin/lint $1").render("x", dir.path());
    assert_eq!(rendered, format!("Run {}/bin/lint x", dir.path().display()));
}

#[test]
fn a_quoted_argument_binds_as_one_word() {
    let dir = tempfile::tempdir().unwrap();
    let rendered = command(dir.path(), "say $1 then $2").render("\"hello world\" next", dir.path());
    assert_eq!(rendered, "say hello world then next");
}
