use std::path::Path;

use super::{Role, Seat, Workflow, from_file};
use crate::config;

fn file(text: &str) -> config::File {
    toml::from_str(text).expect("config parses")
}

#[test]
fn roles_round_trip_through_their_names() {
    for role in Role::ALL {
        assert_eq!(role.name().parse::<Role>().expect("parses"), role);
        assert_eq!(role.to_string(), role.name());
    }
    "reviewer".parse::<Role>().unwrap_err();
}

#[test]
fn seats_parse_as_a_name_or_an_inline_table() {
    let parsed = from_file(
        &file(
            "workflow = \"cheap\"\n\
             [workflows.cheap]\n\
             scout = \"kimi\"\n\
             review = { endpoint = \"glm\", model = \"glm-5.3\" }\n",
        ),
        "cheap",
    )
    .expect("workflow loads");
    assert_eq!(parsed.name, "cheap");
    assert_eq!(
        parsed.seat(Role::Scout),
        Some(&Seat {
            endpoint: "kimi".to_string(),
            model: None,
        })
    );
    assert_eq!(
        parsed.seat(Role::Review),
        Some(&Seat {
            endpoint: "glm".to_string(),
            model: Some("glm-5.3".to_string()),
        })
    );
    assert_eq!(parsed.seat(Role::Implement), None);
    assert_eq!(parsed.summary(), "scout kimi · review glm glm-5.3");
}

#[test]
fn unknown_roles_missing_workflows_and_blank_endpoints_are_refused() {
    let unknown = from_file(&file("[workflows.x]\nreviewer = \"glm\"\n"), "x").unwrap_err();
    assert!(unknown.to_string().contains("unknown workflow role"));
    let missing = from_file(&file(""), "x").unwrap_err();
    assert!(missing.to_string().contains("no workflow `x`"));
    let blank = from_file(&file("[workflows.x]\nplan = \"\"\n"), "x").unwrap_err();
    assert!(blank.to_string().contains("names no endpoint"));
    assert_eq!(
        Workflow::default().summary(),
        "every role on the driving seat"
    );
}

#[test]
fn save_replaces_the_table_and_keeps_the_rest_of_the_file() {
    let home = tempfile::tempdir().expect("temp home");
    let path = home.path().join("config.toml");
    std::fs::write(
        &path,
        "default = \"glm\"\n[endpoint.glm]\nmodel = \"glm-5.3\"\n\n[workflows.cheap]\nscout = \"kimi\"\njudge = \"grok\"\n",
    )
    .expect("seed");
    let workflow = Workflow {
        name: "cheap".to_string(),
        seats: [
            (
                Role::Plan,
                Seat {
                    endpoint: "grok".to_string(),
                    model: Some("grok-4.6".to_string()),
                },
            ),
            (
                Role::Summary,
                Seat {
                    endpoint: "qwen".to_string(),
                    model: None,
                },
            ),
        ]
        .into_iter()
        .collect(),
    };

    config::save_workflow(&path, &workflow).expect("save");

    let saved = config::load_from(&path).expect("reload");
    assert_eq!(saved.default.as_deref(), Some("glm"));
    assert!(saved.endpoint.contains_key("glm"));
    let reloaded = from_file(&saved, "cheap").expect("workflow reloads");
    assert_eq!(reloaded, workflow, "old rows are gone, new rows are exact");
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("[workflows.cheap]"), "{text}");
    assert!(
        !text.contains("[workflows]\n\n[workflows.cheap]"),
        "the parent table stays implicit: {text}"
    );
    assert_eq!(
        config::load_from(Path::new("/nonexistent/config.toml"))
            .expect("missing config is empty")
            .workflows
            .len(),
        0
    );
}

#[test]
fn verbs_are_not_names_and_default_must_exist() {
    let home = tempfile::tempdir().expect("temp home");
    let path = home.path().join("config.toml");
    std::fs::write(&path, "[workflows.cheap]\nscout = \"kimi\"\n").expect("seed");
    for verb in super::RESERVED {
        let saved = config::save_workflow(
            &path,
            &Workflow {
                name: verb.to_string(),
                seats: std::collections::BTreeMap::new(),
            },
        );
        saved.expect("the writer itself does not judge names");
    }
    config::save_default_workflow(&path, "cheap").expect("default");
    let file = config::load_from(&path).expect("reload");
    assert_eq!(file.workflow.as_deref(), Some("cheap"));
    assert!(file.workflows.contains_key("cheap"));
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.starts_with("workflow = \"cheap\"\n"), "{text}");
}
