use std::process::Stdio;

use super::{isolated, summonr};

#[test]
fn a_local_server_is_added_listed_first_and_removed_from_the_shell() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let (ok, text) = summonr(home, &["login", "nope"]);
    assert!(!ok && text.contains("--url URL"), "{text}");
    let (ok, text) = summonr(
        home,
        &[
            "login",
            "lab",
            "--url",
            "http://127.0.0.1:9/v1",
            "--model",
            "m",
        ],
    );
    assert!(ok && text.contains("Added lab"), "{text}");
    let (_, listed) = summonr(home, &["endpoints"]);
    assert!(listed.trim_start().starts_with("lab "), "{listed}");
    let (_, text) = summonr(home, &["logout", "lab"]);
    assert!(text.contains("needs no key"), "{text}");
    let (ok, text) = summonr(home, &["logout", "lab", "--remove"]);
    assert!(ok && text.contains("Removed the endpoint lab"), "{text}");
    let (_, listed) = summonr(home, &["endpoints"]);
    assert!(!listed.contains("lab "), "{listed}");
}

#[test]
fn a_local_server_updates_in_place_reads_ready_and_takes_its_default_with_it() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let url = "http://127.0.0.1:9/v1";
    let (ok, text) = summonr(home, &["login", "lab", "--url", url, "--model", "m"]);
    assert!(ok, "{text}");
    let (ok, text) = summonr(home, &["login", "lab", "--url", url, "--model", "m2"]);
    assert!(ok && text.contains("Updated"), "{text}");
    let (ok, text) = summonr(home, &["login", "lab"]);
    assert!(ok && text.contains("needs no key"), "{text}");
    let config = home.join(".summonr/config.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        format!("default = \"lab\"\ndefault_model = \"m2\"\n{text}"),
    )
    .unwrap();
    let (ok, text) = summonr(home, &["logout", "lab", "--remove"]);
    assert!(ok, "{text}");
    let left = std::fs::read_to_string(&config).unwrap();
    assert!(!left.contains("default"), "{left}");
}

fn rejecting_server() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buffer = [0_u8; 4096];
            drop(std::io::Read::read(&mut stream, &mut buffer));
            let body = r#"{"error":{"message":"invalid api key"}}"#;
            let reply = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            drop(std::io::Write::write_all(&mut stream, reply.as_bytes()));
        }
    });
    port
}

#[test]
fn a_rejected_key_is_reported_and_not_stored() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let url = format!("http://127.0.0.1:{}/v1", rejecting_server());
    let (ok, text) = summonr(home, &["login", "lab", "--url", &url, "--model", "m"]);
    assert!(ok, "{text}");
    let mut login = isolated(env!("CARGO_BIN_EXE_summonr"), home)
        .args(["login", "lab", "--key"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(login.stdin.as_mut().unwrap(), b"sk-rejected-fixture\n").unwrap();
    let output = login.wait_with_output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{text}");
    assert!(!text.contains("stored in"), "{text}");
    let stored =
        std::fs::read_to_string(home.join(".summonr/credentials.toml")).unwrap_or_default();
    assert!(
        !stored.contains("sk-rejected-fixture"),
        "the rejected key was stored"
    );
}
