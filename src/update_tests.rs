#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use super::{Outcome, Source, run};

const ASSET: &str = "summonr-test-target.tar.gz";

struct Release {
    url: String,
    asked: Arc<Mutex<Vec<String>>>,
}

fn serve(archive: Vec<u8>, sum: String) -> Release {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&asked);
    let tag = format!("{base}/releases/tag/v9.9.9");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut head = Vec::new();
            let mut byte = [0_u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            let path = String::from_utf8_lossy(&head)
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();
            log.lock().unwrap().push(path.clone());
            let download = format!("/releases/download/v9.9.9/{ASSET}");
            let (status, extra, body) = match path.as_str() {
                "/releases/latest" => ("302 Found", format!("Location: {tag}\r\n"), Vec::new()),
                "/releases/tag/v9.9.9" => ("200 OK", String::new(), b"release".to_vec()),
                path if path == download => ("200 OK", String::new(), archive.clone()),
                path if path == format!("{download}.sha256") => {
                    ("200 OK", String::new(), sum.clone().into_bytes())
                }
                _ => ("404 Not Found", String::new(), Vec::new()),
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .and_then(|()| stream.write_all(&body))
            .unwrap();
        }
    });
    Release {
        url: format!("{base}/releases"),
        asked,
    }
}

fn script(path: &Path, prints: &str) {
    std::fs::write(path, format!("#!/bin/sh\necho '{prints}'\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn archive(root: &Path) -> Vec<u8> {
    let pack = root.join("pack");
    std::fs::create_dir_all(pack.join("summonr-test-target")).unwrap();
    script(
        &pack.join("summonr-test-target/summonr"),
        "summonr 9.9.9 (test)",
    );
    let out = root.join(ASSET);
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&out)
        .arg("-C")
        .arg(&pack)
        .arg("summonr-test-target")
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read(out).unwrap()
}

fn sum(bytes: &[u8]) -> String {
    format!("{:x}  {ASSET}\n", Sha256::digest(bytes))
}

fn installed(root: &Path) -> PathBuf {
    let bin = root.join("home/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("summonr");
    script(&exe, "summonr 0.1.0 (old)");
    exe
}

fn source<'a>(url: &'a str, current: &'a str) -> Source<'a> {
    Source {
        releases: url,
        target: "test-target",
        current,
    }
}

fn version(exe: &Path) -> String {
    let output = Command::new(exe).arg("--version").output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn beside(exe: &Path) -> Vec<String> {
    std::fs::read_dir(exe.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

#[tokio::test]
async fn an_update_swaps_in_the_checked_release() {
    let root = tempfile::tempdir().unwrap();
    let packed = archive(root.path());
    let release = serve(packed.clone(), sum(&packed));
    let exe = installed(root.path());
    let outcome = run(&source(&release.url, "0.1.0"), &exe).await.unwrap();
    assert_eq!(
        outcome,
        Outcome::Updated {
            from: "0.1.0".to_string(),
            to: "9.9.9".to_string()
        }
    );
    assert_eq!(version(&exe), "summonr 9.9.9 (test)");
    assert_eq!(beside(&exe), ["summonr"]);
}

#[tokio::test]
async fn a_checksum_mismatch_keeps_the_installed_binary() {
    let root = tempfile::tempdir().unwrap();
    let packed = archive(root.path());
    let release = serve(packed, sum(b"another archive"));
    let exe = installed(root.path());
    let error = run(&source(&release.url, "0.1.0"), &exe)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("checksum mismatch"), "{error}");
    assert_eq!(version(&exe), "summonr 0.1.0 (old)");
    assert_eq!(beside(&exe), ["summonr"]);
}

#[tokio::test]
async fn the_latest_release_downloads_nothing() {
    let root = tempfile::tempdir().unwrap();
    let packed = archive(root.path());
    let release = serve(packed.clone(), sum(&packed));
    let exe = installed(root.path());
    let outcome = run(&source(&release.url, "9.9.9"), &exe).await.unwrap();
    assert_eq!(outcome, Outcome::Current("9.9.9".to_string()));
    let asked = release.asked.lock().unwrap().clone();
    assert!(
        !asked.iter().any(|path| path.contains("/download/")),
        "{asked:?}"
    );
    assert_eq!(version(&exe), "summonr 0.1.0 (old)");
}

#[tokio::test]
async fn a_source_build_names_the_cargo_command() {
    let root = tempfile::tempdir().unwrap();
    let exe = installed(root.path());
    std::fs::create_dir(exe.parent().unwrap().join(".fingerprint")).unwrap();
    let error = run(&source("http://127.0.0.1:9/releases", "0.1.0"), &exe)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("cargo install --git"), "{error}");
    assert_eq!(version(&exe), "summonr 0.1.0 (old)");
}
