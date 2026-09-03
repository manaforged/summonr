use std::collections::BTreeMap;

use super::*;

#[test]
fn defaults_apply_when_config_says_nothing() {
    let switches = Switches::from_maps(&BTreeMap::new(), &BTreeMap::new());
    assert!(switches.enabled("search"));
    assert!(switches.enabled("task"));
    assert!(!switches.enabled("bus"));
    assert!(!switches.enabled("nonsense"));
}

#[test]
fn a_project_can_turn_a_module_off_but_not_on() {
    let user = BTreeMap::from([("search".to_string(), false)]);
    let project = BTreeMap::from([("search".to_string(), true), ("task".to_string(), false)]);
    let switches = Switches::from_maps(&user, &project);
    assert!(!switches.enabled("search"), "the project cannot re-enable");
    assert!(!switches.enabled("task"), "the project can disable");
    assert!(switches.table().contains("task    off (project)"));
    assert!(switches.table().contains("search  off "));
}

#[test]
fn module_names_are_unique_and_known() {
    let names: std::collections::BTreeSet<&str> = ALL.iter().map(|module| module.name).collect();
    assert_eq!(names.len(), ALL.len());
    assert_eq!(named("lsp").expect("lsp").name, "lsp");
    assert!(
        named("plugin")
            .expect_err("unknown")
            .to_string()
            .contains("unknown module")
    );
}
