use super::*;

fn agent(plugin: &str, name: &str) -> Agent {
    Agent {
        plugin: plugin.to_string(),
        name: name.to_string(),
        description: String::new(),
        tools: None,
        instructions: String::new(),
    }
}

#[test]
fn a_block_list_of_tools_is_read_and_not_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(DIR)).unwrap();
    std::fs::write(
        dir.path().join(DIR).join("reviewer.md"),
        "---\nname: reviewer\ntools:\n  - Read\n  - Grep\n---\nReview it.\n",
    )
    .unwrap();
    let plugin = Plugin::at("demo".to_string(), dir.path().to_owned());
    let [found] = of_plugin(&plugin, workspace.path()).try_into().unwrap();
    assert_eq!(
        found.tools,
        Some(vec!["Read".to_string(), "Grep".to_string()])
    );
    assert_eq!(found.instructions, "Review it.");
}

#[test]
fn agents_that_share_a_name_are_listed_by_plugin() {
    let all = [
        agent("alpha", "reviewer"),
        agent("beta", "reviewer"),
        agent("beta", "writer"),
    ];
    assert_eq!(labels(&all), ["alpha:reviewer", "beta:reviewer", "writer"]);
}
