use super::*;

#[test]
fn the_default_prompt_says_what_done_means() {
    let workspace = tempfile::tempdir().unwrap();
    let prompt = compose(workspace.path(), None, false, false, false);
    assert!(
        prompt.contains("Treat the request as a contract"),
        "{prompt}"
    );
    assert!(
        prompt.contains("run it the way the request says it will be used"),
        "{prompt}"
    );
    assert!(
        prompt.contains("not when your own check passes"),
        "{prompt}"
    );
}
