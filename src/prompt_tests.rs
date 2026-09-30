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
