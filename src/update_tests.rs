use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use super::{ARCHIVE, BINARY, Outcome, Source, run, unpack};

struct Release {
    url: String,
    asked: Arc<Mutex<Vec<String>>>,
}

fn serve(version: &str, files: Vec<(String, Vec<u8>)>) -> Release {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&asked);
    let tag = format!("/releases/tag/v{version}");
    let location = format!("{base}{tag}");
    let download = format!("/releases/download/v{version}/");
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
            let file = path
                .strip_prefix(&download)
                .and_then(|name| files.iter().find(|(file, _)| file == name));
            let (status, extra, body) = match (path.as_str(), file) {
                ("/releases/latest", _) => {
                    ("302 Found", format!("Location: {location}\r\n"), Vec::new())
                }
                (path, _) if path == tag => ("200 OK", String::new(), b"release".to_vec()),
                (_, Some((_, bytes))) => ("200 OK", String::new(), bytes.clone()),
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

fn version(exe: &Path) -> String {
    let output = Command::new(exe).arg("--version").output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn beside(exe: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(exe.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
#[ignore = "release rehearsal: SUMMONR_REHEARSE_DIST names a folder of release archives"]
async fn a_release_archive_replaces_a_running_install() {
    let dist = PathBuf::from(std::env::var("SUMMONR_REHEARSE_DIST").unwrap());
    let built = Source::this_build();
    let target =
        std::env::var("SUMMONR_REHEARSE_TARGET").unwrap_or_else(|_| built.target.to_string());
    let asset = format!("summonr-{target}.{ARCHIVE}");
    let archive = std::fs::read(dist.join(&asset)).unwrap();
    let sum = std::fs::read(dist.join(format!("{asset}.sha256"))).unwrap();
    let release = serve(
        built.current,
        vec![
            (asset.clone(), archive.clone()),
            (format!("{asset}.sha256"), sum),
        ],
    );
    let root = tempfile::tempdir().unwrap();
    unpack(root.path(), &asset, &archive).unwrap();
    let bin = root.path().join("home/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join(BINARY);
    std::fs::copy(
        root.path().join(format!("summonr-{target}")).join(BINARY),
        &exe,
    )
    .unwrap();
    let home = root.path().join("home");
    let mut session = Command::new(&exe)
        .arg("acp")
        .env("SUMMONR_HOME", home.join(".summonr"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    #[cfg(unix)]
    let before = std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&exe).unwrap());
    let source = Source {
        releases: &release.url,
        target: &target,
        current: "0.0.0",
    };
    let outcome = run(&source, &exe).await;
    let alive = session.try_wait().unwrap().is_none();
    session.kill().unwrap();
    assert_eq!(
        outcome.unwrap(),
        Outcome::Updated {
            from: "0.0.0".to_string(),
            to: built.current.to_string()
        }
    );
    assert!(alive, "the running session stopped during the update");
    let asked = release.asked.lock().unwrap().clone();
    let download = format!("/releases/download/v{}/{asset}", built.current);
    assert_eq!(
        asked,
        [
            "/releases/latest".to_string(),
            format!("/releases/tag/v{}", built.current),
            download.clone(),
            format!("{download}.sha256"),
        ]
    );
    assert!(version(&exe).contains(built.current), "{}", version(&exe));
    #[cfg(unix)]
    assert_ne!(
        std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&exe).unwrap()),
        before,
        "the binary was rewritten in place"
    );
    let kept: Vec<String> = if cfg!(windows) {
        vec![BINARY.to_string(), format!("{BINARY}.old")]
    } else {
        vec![BINARY.to_string()]
    };
    assert_eq!(beside(&exe), kept);
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use sha2::{Digest, Sha256};

    use super::super::{Outcome, Source, run};
    use super::{beside, serve, version};

    const ASSET: &str = "summonr-test-target.tar.gz";

    fn script(path: &Path, prints: &str) {
        std::fs::write(path, format!("#!/bin/sh\necho '{prints}'\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn release(root: &Path, sum_of: Option<&[u8]>) -> super::Release {
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
        let archive = std::fs::read(out).unwrap();
        let sum = format!(
            "{:x}  {ASSET}\n",
            Sha256::digest(sum_of.unwrap_or(&archive))
        );
        serve(
            "9.9.9",
            vec![
                (ASSET.to_string(), archive),
                (format!("{ASSET}.sha256"), sum.into_bytes()),
            ],
        )
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

    #[tokio::test]
    async fn an_update_swaps_in_the_checked_release() {
        let root = tempfile::tempdir().unwrap();
        let release = release(root.path(), None);
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
        let release = release(root.path(), Some(b"another archive"));
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
    async fn a_read_only_folder_keeps_the_installed_binary() {
        let root = tempfile::tempdir().unwrap();
        let release = release(root.path(), None);
        let exe = installed(root.path());
        let bin = exe.parent().unwrap().to_path_buf();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o555)).unwrap();
        let outcome = run(&source(&release.url, "0.1.0"), &exe).await;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = outcome.unwrap_err().to_string();
        assert!(error.contains("cannot replace"), "{error}");
        assert_eq!(version(&exe), "summonr 0.1.0 (old)");
        let asked = release.asked.lock().unwrap().clone();
        assert!(
            !asked.iter().any(|path| path.contains("/download/")),
            "{asked:?}"
        );
    }

    #[tokio::test]
    async fn a_killed_update_leaves_no_staging_folder_behind() {
        let root = tempfile::tempdir().unwrap();
        let release = release(root.path(), None);
        let exe = installed(root.path());
        let bin = exe.parent().unwrap();
        let stale = bin.join(".summonr-update-killed");
        let live = bin.join(".summonr-update-live");
        std::fs::create_dir(&stale).unwrap();
        std::fs::create_dir(&live).unwrap();
        let long_ago = std::time::SystemTime::now()
            - crate::limits::limits().update()
            - std::time::Duration::from_secs(60);
        std::fs::File::open(&stale)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        let outcome = run(&source(&release.url, "9.9.9"), &exe).await.unwrap();
        assert_eq!(outcome, Outcome::Current("9.9.9".to_string()));
        assert_eq!(beside(&exe), [".summonr-update-live", "summonr"]);
    }

    #[tokio::test]
    async fn the_latest_release_downloads_nothing() {
        let root = tempfile::tempdir().unwrap();
        let release = release(root.path(), None);
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
}
