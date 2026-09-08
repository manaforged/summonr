//! Where keys and tokens live.
//!
//! Default: a sealed file `~/.mage/credentials.toml` (ChaCha20-Poly1305,
//! mode 0600). The wrapping key is a 32-byte OS item (`summoner` / `seal`),
//! not a sibling file. `cat` of the blob does not show keys. Opt in to
//! putting every secret in the OS store with `secrets = "keychain"` or
//! `MAGE_SECRETS=keychain`. Neither mode falls back to the other.

#[cfg(unix)]
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng, rand_core::RngCore};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use toml_edit::{DocumentMut, value};

use crate::error::{Error, Result};

const SERVICE: &str = "summoner";
#[cfg(any(target_os = "macos", all(test, unix)))]
const ACCOUNT: &str = "credentials";
const SEAL: &str = "seal";
const MAGIC: &[u8] = b"MAGE1";
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const FILE: &str = "credentials.toml";
/// How long one `security` call may take before it counts as a locked or
/// unreachable keychain.
#[cfg(not(test))]
const TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const TIMEOUT: Duration = Duration::from_millis(600);
const LOCKED: &str = "the keychain did not answer — unlock it";
const CHOICES: &str = "secrets must be \"file\" or \"keychain\"";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Env,
    Keychain,
    File,
    Config,
}

impl Source {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Keychain => "keychain",
            Self::File => "file",
            Self::Config => "config (plaintext)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Backend {
    File,
    /// The `security` tool; the path is a test seam.
    #[cfg(any(target_os = "macos", all(test, unix)))]
    Security(PathBuf),
    #[cfg(not(target_os = "macos"))]
    Native,
}

pub(crate) struct Store {
    file: PathBuf,
    backend: Backend,
    seal: Option<[u8; KEY_LEN]>,
    wrap_tool: Option<PathBuf>,
}

impl Store {
    #[cfg(not(test))]
    pub(crate) fn discover() -> Result<Self> {
        let file = crate::prompt::mage_home()?.join(FILE);
        let choice = std::env::var("MAGE_SECRETS")
            .ok()
            .or_else(|| crate::config::load().ok().and_then(|file| file.secrets))
            .unwrap_or_default();
        Ok(Self {
            file,
            backend: backend(&choice)?,
            seal: None,
            wrap_tool: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn discover() -> Result<Self> {
        let file = std::env::temp_dir()
            .join(format!("summon-test-{}", std::process::id()))
            .join(FILE);
        Ok(Self::file_with_seal(file.clone(), test_seal(&file)))
    }

    #[cfg(test)]
    pub(crate) fn file_only(file: PathBuf) -> Self {
        let mut seal = [0_u8; KEY_LEN];
        OsRng.fill_bytes(&mut seal);
        Self::file_with_seal(file, seal)
    }

    #[cfg(test)]
    pub(crate) fn file_with_seal(file: PathBuf, seal: [u8; KEY_LEN]) -> Self {
        Self {
            file,
            backend: Backend::File,
            seal: Some(seal),
            wrap_tool: None,
        }
    }

    #[cfg(all(test, unix))]
    pub(crate) fn file_via_security(file: PathBuf, security: PathBuf) -> Self {
        Self {
            file,
            backend: Backend::File,
            seal: None,
            wrap_tool: Some(security),
        }
    }

    #[cfg(all(test, unix))]
    pub(crate) fn with_security(file: PathBuf, security: PathBuf) -> Self {
        Self {
            file,
            backend: Backend::Security(security),
            seal: None,
            wrap_tool: None,
        }
    }

    pub(crate) fn get(&self, name: &str) -> Result<Option<(String, Source)>> {
        match &self.backend {
            Backend::File => self.locked(|| {
                Ok(lookup(&self.read_file()?, name).map(|secret| (secret, Source::File)))
            }),
            #[cfg(any(target_os = "macos", all(test, unix)))]
            Backend::Security(tool) => {
                if let Some(secret) = lookup(&self.read_blob(tool)?, name) {
                    return Ok(Some((secret, Source::Keychain)));
                }
                let Some(secret) = find(tool, name)? else {
                    return Ok(None);
                };
                self.locked(|| {
                    let mut blob = self.read_blob(tool)?;
                    blob[name] = value(&secret);
                    self.write_blob(tool, &blob)?;
                    security(
                        tool,
                        &["delete-generic-password", "-s", SERVICE, "-a", name],
                        None,
                    )
                    .map(drop)
                })?;
                Ok(Some((secret, Source::Keychain)))
            }
            #[cfg(not(target_os = "macos"))]
            Backend::Native => Ok(native_get(name)?.map(|secret| (secret, Source::Keychain))),
        }
    }

    pub(crate) fn set(&self, name: &str, secret: &str) -> Result<Source> {
        match &self.backend {
            Backend::File => self.locked(|| {
                let mut document = self.read_file()?;
                document[name] = value(secret);
                self.write_file(&document)?;
                Ok(Source::File)
            }),
            #[cfg(any(target_os = "macos", all(test, unix)))]
            Backend::Security(tool) => self.locked(|| {
                let mut blob = self.read_blob(tool)?;
                blob[name] = value(secret);
                self.write_blob(tool, &blob)?;
                Ok(Source::Keychain)
            }),
            #[cfg(not(target_os = "macos"))]
            Backend::Native => {
                native_entry(name)?
                    .set_password(secret)
                    .map_err(|error| Error::Config(format!("credential store: {error}")))?;
                Ok(Source::Keychain)
            }
        }
    }

    pub(crate) fn forget(&self, name: &str) -> Result<bool> {
        match &self.backend {
            Backend::File => self.locked(|| {
                let mut document = self.read_file()?;
                if document.remove(name).is_none() {
                    return Ok(false);
                }
                self.write_file(&document)?;
                Ok(true)
            }),
            #[cfg(any(target_os = "macos", all(test, unix)))]
            Backend::Security(tool) => self.locked(|| {
                let mut blob = self.read_blob(tool)?;
                if blob.remove(name).is_none() {
                    return Ok(false);
                }
                self.write_blob(tool, &blob)?;
                Ok(true)
            }),
            #[cfg(not(target_os = "macos"))]
            Backend::Native => match native_entry(name)?.delete_credential() {
                Ok(()) => Ok(true),
                Err(keyring::Error::NoEntry) => Ok(false),
                Err(error) => Err(Error::Config(format!("credential store: {error}"))),
            },
        }
    }

    /// Runs one read-modify-write under a file lock shared by every Summoner
    /// process on this machine, so two logins or refreshes cannot drop each
    /// other's change.
    fn locked<T>(&self, work: impl FnOnce() -> Result<T>) -> Result<T> {
        let path = self.file.with_extension("lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(&path)?;
        lock.lock()?;
        let outcome = work();
        drop(lock.unlock());
        outcome
    }

    #[cfg(any(target_os = "macos", all(test, unix)))]
    fn read_blob(&self, tool: &Path) -> Result<DocumentMut> {
        let Some(encoded) = find(tool, ACCOUNT)? else {
            return Ok(DocumentMut::new());
        };
        let bytes = BASE64
            .decode(encoded.trim())
            .map_err(|error| Error::Config(format!("keychain entry is not base64: {error}")))?;
        parse(&String::from_utf8_lossy(&bytes))
    }

    #[cfg(any(target_os = "macos", all(test, unix)))]
    fn write_blob(&self, tool: &Path, blob: &DocumentMut) -> Result<()> {
        let encoded = BASE64.encode(blob.to_string().as_bytes());
        let line = format!("add-generic-password -U -s {SERVICE} -a {ACCOUNT} -w \"{encoded}\"\n");
        security(tool, &["-i"], Some(line)).map(drop)
    }

    fn read_file(&self) -> Result<DocumentMut> {
        tighten(&self.file)?;
        match std::fs::read(&self.file) {
            Ok(bytes) => {
                let sealed = bytes.starts_with(MAGIC);
                let key = sealed.then(|| self.wrap(false)).transpose()?;
                let document = open_file(key.as_ref(), &bytes).map_err(|error| {
                    Error::Config(format!(
                        "{error} — fix or remove {} and log in again",
                        self.file.display()
                    ))
                })?;
                if !sealed {
                    self.write_file(&document)?;
                }
                Ok(document)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DocumentMut::new()),
            Err(error) => Err(error.into()),
        }
    }

    fn write_file(&self, document: &DocumentMut) -> Result<()> {
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
            tighten_dir(parent)?;
        }
        write_private(
            &self.file,
            &seal_file(&self.wrap(true)?, document.to_string().as_bytes())?,
        )
    }

    fn wrap(&self, create: bool) -> Result<[u8; KEY_LEN]> {
        if let Some(key) = self.seal {
            return Ok(key);
        }
        os_wrap_key(create, self.wrap_tool.as_deref())
    }
}

/// One keychain item's password, `None` when there is no such item.
fn find(tool: &Path, account: &str) -> Result<Option<String>> {
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

/// Runs the `security` tool with a deadline and returns its stdout. A
/// deadline is reported as a locked keychain; any other failure carries the
/// exit code and the tool's first line of stderr. Nothing falls back to a
/// file.
fn security(tool: &Path, args: &[&str], stdin: Option<String>) -> Result<String> {
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
        if wait.recv_timeout(Duration::from_millis(20)).is_ok() {
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

fn backend(choice: &str) -> Result<Backend> {
    match choice.trim().to_ascii_lowercase().as_str() {
        "" | "file" => Ok(Backend::File),
        "keychain" => Ok(platform()),
        other => Err(Error::Config(format!("{CHOICES}, not \"{other}\""))),
    }
}

#[cfg(not(target_os = "macos"))]
fn native_entry(name: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, name)
        .map_err(|error| Error::Config(format!("credential store: {error}")))
}

#[cfg(not(target_os = "macos"))]
fn native_get(name: &str) -> Result<Option<String>> {
    match native_entry(name)?.get_password() {
        Ok(secret) if !secret.trim().is_empty() => Ok(Some(secret)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(Error::Config(format!("credential store: {error}"))),
    }
}

fn platform() -> Backend {
    #[cfg(target_os = "macos")]
    {
        Backend::Security(PathBuf::from("/usr/bin/security"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Backend::Native
    }
}

fn lookup(document: &DocumentMut, name: &str) -> Option<String> {
    document
        .get(name)
        .and_then(toml_edit::Item::as_str)
        .filter(|secret| !secret.trim().is_empty())
        .map(str::to_string)
}

fn parse(text: &str) -> Result<DocumentMut> {
    text.parse::<DocumentMut>()
        .map_err(|error| Error::Config(format!("credential store: {error}")))
}

/// Puts a credentials file back to owner-only if anything loosened it.
fn tighten(_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(_path)
            && meta.is_file()
            && meta.permissions().mode() & 0o077 != 0
        {
            std::fs::set_permissions(_path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}

/// Keeps the directory that holds the credentials file owner-only.
fn tighten_dir(_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(_path)
            && meta.is_dir()
            && meta.permissions().mode() & 0o077 != 0
        {
            std::fs::set_permissions(_path, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

fn open_file(key: Option<&[u8; KEY_LEN]>, bytes: &[u8]) -> Result<DocumentMut> {
    if bytes.starts_with(MAGIC) {
        let key = key.ok_or_else(|| {
            Error::Config("credential store: sealed file has no wrapping key".to_string())
        })?;
        let rest = &bytes[MAGIC.len()..];
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

fn seal_file(key: &[u8; KEY_LEN], plain: &[u8]) -> Result<Vec<u8>> {
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

fn decode_wrap(encoded: &str) -> Result<[u8; KEY_LEN]> {
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

fn os_wrap_key(create: bool, tool: Option<&Path>) -> Result<[u8; KEY_LEN]> {
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

fn wrap_via_security(tool: &Path, create: bool) -> Result<[u8; KEY_LEN]> {
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
fn test_seal(file: &Path) -> [u8; KEY_LEN] {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static KEYS: OnceLock<Mutex<HashMap<PathBuf, [u8; KEY_LEN]>>> = OnceLock::new();
    let mut keys = KEYS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("wrap-key map");
    *keys.entry(file.to_path_buf()).or_insert_with(|| {
        let mut key = [0_u8; KEY_LEN];
        OsRng.fill_bytes(&mut key);
        key
    })
}

/// Writes `bytes` to `path` through a fresh owner-only temporary file in the
/// same directory and a rename, then syncs the directory so the rename is
/// durable before anything that depends on it.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .ok_or_else(|| Error::Config(format!("{} has no directory", path.display())))?;
    let mut temp = tempfile::Builder::new()
        .prefix(".credentials-")
        .tempfile_in(dir)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| Error::Io(error.error))?;
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    Ok(())
}

pub fn get(name: &str) -> Result<Option<(String, Source)>> {
    Store::discover()?.get(name)
}

pub fn set(name: &str, secret: &str) -> Result<Source> {
    Store::discover()?.set(name, secret)
}

pub fn forget(name: &str) -> Result<bool> {
    Store::discover()?.forget(name)
}

#[cfg(test)]
#[path = "secret_tests.rs"]
mod tests;
