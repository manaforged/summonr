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

#[cfg(unix)]
#[test]
fn a_clean_log_is_left_in_place_and_a_dirty_one_is_scrubbed() {
    use std::os::unix::fs::MetadataExt;

    let dir = tempfile::tempdir().unwrap();
    let clean = dir.path().join("clean.log");
    std::fs::write(&clean, "nothing to hide\n").unwrap();
    let before = std::fs::metadata(&clean).unwrap().ino();
    assert!(!rewrite(&clean, &[b"hunter2"]).unwrap());
    assert_eq!(
        std::fs::metadata(&clean).unwrap().ino(),
        before,
        "no temp file, no rename"
    );

    let dirty = dir.path().join("dirty.log");
    std::fs::write(&dirty, "password: hunter2 ok\n").unwrap();
    assert!(rewrite(&dirty, &[b"hunter2"]).unwrap());
    assert_eq!(
        std::fs::read_to_string(&dirty).unwrap(),
        "password: [secret] ok\n"
    );
}

#[test]
fn a_prompt_names_the_program_that_asked() {
    assert_eq!(
        labeled("/usr/bin/sudo -A true\n", "Password:"),
        "Password: (asked by `/usr/bin/sudo -A true`)"
    );
    assert_eq!(
        labeled("python3 steal.py", "Password:"),
        "unrecognized program python3 asks for a password (`python3 steal.py`): Password:"
    );
    assert!(labeled("", "Password:").starts_with("unrecognized program unknown"));
}

#[test]
fn the_asker_comes_from_the_peer_process_not_the_client() {
    let (server, mut client) = std::os::unix::net::UnixStream::pair().unwrap();
    std::io::Write::write_all(&mut client, b"sudo -A true\n").unwrap();
    let asker = super::peer::asker(&server, 256);
    let parent = std::process::Command::new("ps")
        .args(["-o", "command=", "-p"])
        .arg(std::os::unix::process::parent_id().to_string())
        .output()
        .unwrap();
    let parent = String::from_utf8_lossy(&parent.stdout).trim().to_string();
    assert_eq!(asker, parent.chars().take(256).collect::<String>());
    assert!(!asker.contains("sudo -A true"));
}
