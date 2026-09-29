use super::*;

#[test]
fn sudo_in_a_command_asks_through_the_broker() {
    if on_path("sudo").is_none() {
        return;
    }
    start(|request| request.answer(None)).unwrap();
    let resolved = std::process::Command::new("sh")
        .args(["-c", "command -v sudo"])
        .env_clear()
        .envs(env())
        .output()
        .unwrap();
    let shim = String::from_utf8(resolved.stdout).unwrap();
    let shim = Path::new(shim.trim());
    let helper = &BROKER.get().unwrap().helper;
    assert_eq!(shim.parent(), helper.parent());
    let script = std::fs::read_to_string(shim).unwrap();
    assert!(script.contains("' -A \"$@\""), "{script}");
}
