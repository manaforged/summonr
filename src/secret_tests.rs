use std::path::{Path, PathBuf};

use super::{SEAL, Source, Store, backend};

/// A stand-in for /usr/bin/security that keeps items in one directory.
#[cfg(unix)]
fn fake_security(dir: &Path, delay: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let items = dir.join("items");
    std::fs::create_dir_all(&items).expect("items dir");
    let script = dir.join("security");
    let body = format!(
        r#"#!/bin/sh
ITEMS="{items}"
{delay}
if [ "$1" = "-i" ]; then read -r line; set -- $line; fi
cmd="$1"; shift
while [ $# -gt 0 ]; do
  case "$1" in
    -s) shift;; -a) account="$2"; shift;; -w) shift; secret="$(printf '%s' "$1" | tr -d '"')";;
  esac
  shift
done
case "$cmd" in
  find-generic-password) [ -f "$ITEMS/$account" ] || exit 44; cat "$ITEMS/$account";;
  add-generic-password) printf '%s' "$secret" > "$ITEMS/$account";;
  delete-generic-password) rm -f "$ITEMS/$account";;
esac
"#,
        items = items.display()
    );
    std::fs::write(&script, body).expect("script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    script
}

#[test]
fn the_file_store_round_trips_with_private_permissions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::file_only(dir.path().join("credentials.toml"));
    assert!(store.get("glm").expect("get").is_none());
    assert_eq!(store.set("glm", "sk-test").expect("set"), Source::File);
    assert_eq!(
        store
            .set("oauth:copilot", "{\"access_token\":\"t\"}")
            .expect("set"),
        Source::File
    );
    assert_eq!(
        store.get("glm").expect("get"),
        Some(("sk-test".to_string(), Source::File))
    );
    assert!(
        store
            .get("oauth:copilot")
            .expect("get")
            .is_some_and(|(blob, _)| blob.contains("access_token"))
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("credentials.toml"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    }
    assert!(store.forget("glm").expect("forget"));
    assert!(store.get("glm").expect("get").is_none());
    assert!(!store.forget("glm").expect("nothing left"));
    let sealed = std::fs::read(dir.path().join("credentials.toml")).expect("sealed");
    assert!(sealed.starts_with(b"MAGE1"), "the blob is sealed, not toml");
    let body = String::from_utf8_lossy(&sealed);
    assert!(!body.contains("sk-test"), "{body}");
    assert!(!body.contains("access_token"), "{body}");
    assert!(
        !dir.path().join("credentials.key").exists(),
        "the wrap key is not a sibling file"
    );
}

#[test]
fn a_plaintext_credentials_file_is_sealed_on_the_next_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("credentials.toml");
    std::fs::write(&path, "glm = \"sk-old\"\n").expect("plaintext");
    let store = Store::file_only(path.clone());
    assert_eq!(
        store.get("glm").expect("get"),
        Some(("sk-old".to_string(), Source::File))
    );
    assert_eq!(
        store.get("glm").expect("get"),
        Some(("sk-old".to_string(), Source::File))
    );
    let sealed = std::fs::read(&path).expect("sealed");
    assert!(sealed.starts_with(b"MAGE1"));
    assert!(!String::from_utf8_lossy(&sealed).contains("sk-old"));
    assert_eq!(
        store.get("glm").expect("kept"),
        Some(("sk-old".to_string(), Source::File))
    );
}

#[test]
fn a_sealed_file_without_its_key_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("credentials.toml");
    let store = Store::file_only(path.clone());
    store.set("glm", "sk").expect("set");
    let other = Store::file_with_seal(path.clone(), [9_u8; 32]);
    let error = other.get("glm").expect_err("wrong wrap key");
    assert!(error.to_string().contains("did not decrypt"), "{error}");
    assert!(std::fs::read(&path).expect("left").starts_with(b"MAGE1"));
}

#[cfg(unix)]
#[test]
fn the_file_store_wraps_through_security() {
    let dir = tempfile::tempdir().expect("tempdir");
    let security = fake_security(dir.path(), "");
    let path = dir.path().join("credentials.toml");
    let store = Store::file_via_security(path.clone(), security.clone());
    assert_eq!(store.set("glm", "sk-wrap").expect("set"), Source::File);
    let sealed = std::fs::read(&path).expect("sealed");
    assert!(sealed.starts_with(b"MAGE1"));
    assert!(!String::from_utf8_lossy(&sealed).contains("sk-wrap"));
    let wrap = std::fs::read_to_string(dir.path().join("items").join(SEAL)).expect("seal item");
    assert!(!wrap.is_empty(), "wrap key landed in the OS item");
    let again = Store::file_via_security(path, security);
    assert_eq!(
        again.get("glm").expect("get"),
        Some(("sk-wrap".to_string(), Source::File))
    );
}

#[cfg(unix)]
#[test]
fn the_keychain_holds_every_secret_in_one_entry_and_folds_an_old_item_in() {
    let dir = tempfile::tempdir().expect("tempdir");
    let security = fake_security(dir.path(), "");
    std::fs::write(dir.path().join("items/oauth:rt"), "old-token").expect("legacy item");
    let store = Store::with_security(dir.path().join("credentials.toml"), security);
    assert!(
        store.get("glm").expect("get").is_none(),
        "no probe finds nothing"
    );
    assert!(
        !dir.path().join("items/credentials").exists(),
        "a miss writes nothing"
    );
    assert_eq!(
        store.get("oauth:rt").expect("get"),
        Some(("old-token".to_string(), Source::Keychain)),
        "the old per-secret item is folded into the entry on first use"
    );
    assert!(
        !dir.path().join("items/oauth:rt").exists(),
        "the old item is gone"
    );
    assert_eq!(store.set("glm", "sk-1").expect("set"), Source::Keychain);
    let mut items: Vec<String> = std::fs::read_dir(dir.path().join("items"))
        .expect("items")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    items.sort();
    assert_eq!(
        items,
        vec!["credentials".to_string()],
        "one entry holds everything"
    );
    assert!(
        !dir.path().join("credentials.toml").exists(),
        "nothing lands on disk"
    );
    assert_eq!(
        store.get("glm").expect("get"),
        Some(("sk-1".to_string(), Source::Keychain))
    );
    assert_eq!(
        store.get("oauth:rt").expect("get"),
        Some(("old-token".to_string(), Source::Keychain))
    );
    assert!(store.forget("glm").expect("forget"));
    assert!(store.get("glm").expect("get").is_none());
}

#[cfg(unix)]
#[test]
fn a_locked_or_missing_keychain_is_an_error_not_a_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let slow = fake_security(dir.path(), "sleep 5");
    let store = Store::with_security(dir.path().join("credentials.toml"), slow);
    let started = std::time::Instant::now();
    let error = store
        .get("glm")
        .expect_err("a hung security call is reported");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "bounded"
    );
    assert!(error.to_string().contains("did not answer"), "{error}");
    store
        .set("glm", "x")
        .expect_err("a hung store refuses writes");
    assert!(
        !dir.path().join("credentials.toml").exists(),
        "no plaintext fallback"
    );
    let none = Store::with_security(
        dir.path().join("credentials.toml"),
        dir.path().join("no-such-security"),
    );
    none.get("glm").expect_err("no tool is an error");
}

#[cfg(unix)]
#[test]
fn a_loosened_credentials_file_is_tightened_on_the_next_read() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("mage");
    let store = Store::file_only(home.join("credentials.toml"));
    store.set("glm", "sk").expect("set");
    assert_eq!(
        std::fs::metadata(&home).expect("dir").permissions().mode() & 0o777,
        0o700
    );
    std::fs::set_permissions(
        home.join("credentials.toml"),
        std::fs::Permissions::from_mode(0o644),
    )
    .expect("loosen");
    assert_eq!(
        store.get("glm").expect("get"),
        Some(("sk".to_string(), Source::File))
    );
    let mode = std::fs::metadata(home.join("credentials.toml"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "{mode:o}");
}

#[test]
fn a_misspelled_store_choice_is_refused_not_downgraded() {
    backend("").expect("unset means the file");
    backend("file").expect("file");
    backend("keychain").expect("keychain");
    let error = backend("keychian").expect_err("typo");
    assert!(error.to_string().contains("secrets must be"), "{error}");
}

#[cfg(unix)]
#[test]
fn writes_go_through_a_fresh_private_temp_file_and_leave_no_stale_copy() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::file_only(dir.path().join("credentials.toml"));
    let stale = dir.path().join("credentials.tmp");
    std::fs::write(&stale, "").expect("stale tmp");
    std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o644)).expect("loose");
    store.set("glm", "sk").expect("set");
    assert_eq!(
        std::fs::read_to_string(&stale).expect("untouched"),
        "",
        "a stale temp file is not reused"
    );
    let mode = std::fs::metadata(dir.path().join("credentials.toml"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    let leftovers: Vec<String> = std::fs::read_dir(dir.path())
        .expect("dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with(".credentials-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_corrupt_credentials_file_names_itself_instead_of_being_replaced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("credentials.toml");
    std::fs::write(&path, "glm = \"sk\nnot toml").expect("write");
    let store = Store::file_only(path.clone());
    let error = store.get("glm").expect_err("corrupt file is an error");
    assert!(error.to_string().contains("credentials.toml"), "{error}");
    store
        .set("kimi", "x")
        .expect_err("nothing is written over a corrupt file");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "glm = \"sk\nnot toml"
    );
}
