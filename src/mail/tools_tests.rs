use super::*;

#[test]
fn every_mail_tool_name_resolves_to_its_tool() {
    for (name, tool) in NAMES {
        assert_eq!(Tool::of(name), Some(*tool));
        assert_eq!(tool.name(), *name);
        assert!(is_mail_tool(name), "{name}");
    }
    let defined: Vec<String> = definitions()
        .into_iter()
        .map(|definition| definition.function.name)
        .collect();
    assert_eq!(defined, ["agents", "send", "inbox", "board"]);
}
