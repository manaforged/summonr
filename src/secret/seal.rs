use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

use base64::engine::general_purpose::STANDARD as BASE64;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng, rand_core::RngCore};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use toml_edit::DocumentMut;

use crate::durable::store;
use crate::error::{Error, Result};

use super::*;

pub(super) fn wrap_path(credentials: &Path) -> PathBuf {
    credentials.with_file_name("credentials.wrap")
}

pub(super) fn file_wrap(credentials: &Path, create: bool) -> Result<[u8; KEY_LEN]> {
    let path = wrap_path(credentials);
    match std::fs::read(&path) {
        Ok(bytes) => decode_wrap_bytes(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            #[cfg(target_os = "macos")]
            if credentials.exists()
                && let Ok(key) = wrap_via_security(Path::new("/usr/bin/security"), false)
            {
                write_wrap(&path, &key)?;
                return Ok(key);
            }
            if !create {
                return Err(Error::Config(
                    "credential store: missing wrap key — set SUMMONR_SECRETS=file and retry"
                        .to_string(),
                ));
            }
            let mut key = [0_u8; KEY_LEN];
            OsRng.fill_bytes(&mut key);
            write_wrap(&path, &key)?;
            Ok(key)
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn write_wrap(path: &Path, key: &[u8; KEY_LEN]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        tighten_dir(parent)?;
    }
    Ok(store::write(path, key)?)
}

pub(super) fn decode_wrap_bytes(bytes: &[u8]) -> Result<[u8; KEY_LEN]> {
    if bytes.len() == KEY_LEN {
        let mut key = [0_u8; KEY_LEN];
        key.copy_from_slice(bytes);
        return Ok(key);
    }
    let text = std::str::from_utf8(bytes).map_err(|error| {
        Error::Config(format!("credential store: wrap key is not utf-8: {error}"))
    })?;
    decode_wrap(text)
}

pub(super) fn find(tool: &Path, account: &str) -> Result<Option<String>> {
    match security(
        tool,
        &["find-generic-password", "-s", SERVICE, "-a", account, "-w"],
        None,
    ) {
        Ok(text) => Ok(Some(text)),
        Err(Error::Config(message)) if message.starts_with("security exit 44") => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn security(tool: &Path, args: &[&str], stdin: Option<String>) -> Result<String> {
    let mut child = Command::new(tool)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| Error::Config(format!("cannot run {}: {error}", tool.display())))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        use std::io::Write as _;
        pipe.write_all(text.as_bytes())?;
        pipe.flush()?;
    }
    let (_tick, wait) = mpsc::channel::<()>();
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            drop(child.kill());
            drop(child.wait());
            return Err(Error::Config(LOCKED.to_string()));
        }
        if wait.recv_timeout(crate::limits::KEYCHAIN_POLL).is_ok() {
            continue;
        }
    };
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        drop(stdout.read_to_string(&mut out));
    }
    if status.success() {
        return Ok(out.trim_end_matches(['\n', '\r']).to_string());
    }
    let mut err = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        drop(stderr.read_to_string(&mut err));
    }
    let code = status.code().unwrap_or(-1);
    let reason = err.lines().next().unwrap_or("").trim();
    Err(Error::Config(format!("security exit {code}: {reason}")))
}

pub(super) fn lookup(document: &DocumentMut, name: &str) -> Option<String> {
    document
        .get(name)
        .and_then(toml_edit::Item::as_str)
        .filter(|secret| !secret.trim().is_empty())
        .map(str::to_string)
}

pub(super) fn parse(text: &str) -> Result<DocumentMut> {
    text.parse::<DocumentMut>()
        .map_err(|error| Error::Config(format!("credential store: {error}")))
}

#[cfg(unix)]
pub(super) fn tighten(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path)
        && meta.is_file()
        && meta.permissions().mode() & 0o077 != 0
    {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn tighten(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn tighten_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path)
        && meta.is_dir()
        && meta.permissions().mode() & 0o077 != 0
    {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn tighten_dir(_: &Path) -> Result<()> {
    Ok(())
}

pub(super) fn open_file(key: Option<&[u8; KEY_LEN]>, bytes: &[u8]) -> Result<DocumentMut> {
    let magic = [MAGIC, MAGIC_LEGACY]
        .into_iter()
        .find(|magic| bytes.starts_with(magic));
    if let Some(magic) = magic {
        let key = key.ok_or_else(|| {
            Error::Config("credential store: sealed file has no wrapping key".to_string())
        })?;
        let rest = &bytes[magic.len()..];
        if rest.len() < NONCE_LEN {
            return Err(Error::Config(
                "credential store: sealed file is truncated".to_string(),
            ));
        }
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        nonce_bytes.copy_from_slice(&rest[..NONCE_LEN]);
        let cipher = ChaCha20Poly1305::new(&Key::from(*key));
        let plain = cipher
            .decrypt(&Nonce::from(nonce_bytes), &rest[NONCE_LEN..])
            .map_err(|error| {
                Error::Config(format!(
                    "credential store: sealed file did not decrypt: {error}"
                ))
            })?;
        let text = String::from_utf8(plain).map_err(|error| {
            Error::Config(format!(
                "credential store: sealed file is not text: {error}"
            ))
        })?;
        parse(&text)
    } else {
        let text = std::str::from_utf8(bytes).map_err(|error| {
            Error::Config(format!("credential store: file is not text: {error}"))
        })?;
        parse(text)
    }
}

pub(super) fn seal_file(key: &[u8; KEY_LEN], plain: &[u8]) -> Result<Vec<u8>> {
    let mut nonce_bytes = [0_u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let cipher = ChaCha20Poly1305::new(&Key::from(*key));
    let ct = cipher
        .encrypt(&Nonce::from(nonce_bytes), plain)
        .map_err(|error| {
            Error::Config(format!(
                "credential store: could not seal the file: {error}"
            ))
        })?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend(ct);
    Ok(out)
}

pub(super) fn decode_wrap(encoded: &str) -> Result<[u8; KEY_LEN]> {
    let bytes = BASE64.decode(encoded.trim()).map_err(|error| {
        Error::Config(format!("credential store: wrap key is not base64: {error}"))
    })?;
    if bytes.len() != KEY_LEN {
        return Err(Error::Config(
            "credential store: wrap key is the wrong size".to_string(),
        ));
    }
    let mut key = [0_u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    Ok(key)
}

pub(super) fn os_wrap_key(create: bool, tool: Option<&Path>) -> Result<[u8; KEY_LEN]> {
    if let Some(tool) = tool {
        return wrap_via_security(tool, create);
    }
    #[cfg(target_os = "macos")]
    {
        wrap_via_security(Path::new("/usr/bin/security"), create)
    }
    #[cfg(not(target_os = "macos"))]
    {
        match native_get(SEAL)? {
            Some(encoded) => decode_wrap(&encoded),
            None if create => {
                let mut key = [0_u8; KEY_LEN];
                OsRng.fill_bytes(&mut key);
                native_entry(SEAL)?
                    .set_password(&BASE64.encode(key))
                    .map_err(|error| Error::Config(format!("credential store: {error}")))?;
                Ok(key)
            }
            None => Err(Error::Config(
                "credential store: missing wrap key — unlock the keychain or log in again"
                    .to_string(),
            )),
        }
    }
}

pub(super) fn wrap_via_security(tool: &Path, create: bool) -> Result<[u8; KEY_LEN]> {
    match find(tool, SEAL)? {
        Some(encoded) => decode_wrap(&encoded),
        None if create => {
            let mut key = [0_u8; KEY_LEN];
            OsRng.fill_bytes(&mut key);
            let encoded = BASE64.encode(key);
            let line = format!("add-generic-password -U -s {SERVICE} -a {SEAL} -w \"{encoded}\"\n");
            security(tool, &["-i"], Some(line))?;
            Ok(key)
        }
        None => Err(Error::Config(
            "credential store: missing wrap key — unlock the keychain or log in again".to_string(),
        )),
    }
}

#[cfg(test)]
pub(super) fn test_seal(file: &Path) -> [u8; KEY_LEN] {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static KEYS: OnceLock<Mutex<HashMap<PathBuf, [u8; KEY_LEN]>>> = OnceLock::new();
    let mut keys = KEYS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *keys.entry(file.to_path_buf()).or_insert_with(|| {
        let mut key = [0_u8; KEY_LEN];
        OsRng.fill_bytes(&mut key);
        key
    })
}
