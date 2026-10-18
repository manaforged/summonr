#[cfg(any(target_os = "macos", all(test, unix)))]
use std::path::Path;
use std::path::PathBuf;

use base64::Engine as _;
#[cfg(any(target_os = "macos", all(test, unix)))]
use base64::engine::general_purpose::STANDARD as BASE64;
use toml_edit::{DocumentMut, value};

use crate::durable::store;
use crate::error::{Error, Result};

const SERVICE: &str = "summoner";
#[cfg(any(target_os = "macos", all(test, unix)))]
const ACCOUNT: &str = "credentials";
const SEAL: &str = "seal";
const MAGIC: &[u8] = b"SMNR1";
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const FILE: &str = "credentials.toml";
const LOCKED: &str = "the keychain did not answer; unlock it";
#[cfg_attr(
    test,
    expect(
        dead_code,
        reason = "discover() is elided under test, so the choice parser has no caller"
    )
)]
const CHOICES: &str = "secrets must be \"file\" or \"keychain\"";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Env,
    Keychain,
    File,
    Config,
    Builtin,
}

impl Source {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Keychain => "keychain",
            Self::File => "file",
            Self::Config => "config (plaintext)",
            Self::Builtin => "built-in",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Backend {
    File,
    #[cfg(any(target_os = "macos", all(test, unix)))]
    #[cfg_attr(
        test,
        expect(
            dead_code,
            reason = "discover() is elided under test, so platform() never builds this"
        )
    )]
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
        let file = crate::prompt::summonr_home()?.join(FILE);
        let choice = match std::env::var("SUMMONR_SECRETS") {
            Ok(choice) => choice,
            Err(_) => crate::config::file()?.secrets.clone().unwrap_or_default(),
        };
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
    pub(crate) fn file_with_seal(file: PathBuf, seal: [u8; KEY_LEN]) -> Self {
        Self {
            file,
            backend: Backend::File,
            seal: Some(seal),
            wrap_tool: None,
        }
    }

    pub(crate) fn get(&self, name: &str) -> Result<Option<(String, Source)>> {
        match &self.backend {
            Backend::File => self.file_get(name),
            #[cfg(any(target_os = "macos", all(test, unix)))]
            Backend::Security(tool) => self.security_get(tool, name),
            #[cfg(not(target_os = "macos"))]
            Backend::Native => Ok(native_get(name)?.map(|secret| (secret, Source::Keychain))),
        }
    }

    pub(crate) fn set(&self, name: &str, secret: &str) -> Result<Source> {
        cache::clear();
        let outcome = self.write_secret(name, secret);
        cache::clear();
        outcome
    }

    pub(crate) fn forget(&self, name: &str) -> Result<bool> {
        cache::clear();
        let outcome = self.remove_secret(name);
        cache::clear();
        outcome
    }

    fn write_secret(&self, name: &str, secret: &str) -> Result<Source> {
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

    fn remove_secret(&self, name: &str) -> Result<bool> {
        match &self.backend {
            Backend::File => self.locked(|| self.file_remove(name)),
            #[cfg(any(target_os = "macos", all(test, unix)))]
            Backend::Security(tool) => self.locked(|| self.blob_remove(tool, name)),
            #[cfg(not(target_os = "macos"))]
            Backend::Native => native_forget(name),
        }
    }

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
                let key = sealed.then(|| self.wrap()).transpose()?;
                let document = open_file(key.as_ref(), &bytes).map_err(|error| {
                    Error::Config(format!(
                        "{error}; fix or remove {} and log in again",
                        self.file.display()
                    ))
                })?;
                if sealed {
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
        Ok(store::write(
            &self.file,
            document.to_string().as_bytes(),
            store::Durability::Synced,
        )?)
    }

    fn wrap(&self) -> Result<[u8; KEY_LEN]> {
        if let Some(key) = self.seal {
            return Ok(key);
        }
        match self.backend {
            Backend::File => file_wrap(&self.file),
            #[cfg(any(target_os = "macos", all(test, unix)))]
            Backend::Security(_) => os_wrap_key(self.wrap_tool.as_deref()),
            #[cfg(not(target_os = "macos"))]
            Backend::Native => os_wrap_key(self.wrap_tool.as_deref()),
        }
    }
}

#[cfg_attr(
    test,
    expect(
        dead_code,
        reason = "discover() is elided under test, so nothing parses the choice"
    )
)]
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

#[cfg_attr(
    test,
    expect(
        dead_code,
        reason = "discover() is elided under test, so backend() never asks for it"
    )
)]
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

mod backends;
mod cache;
mod seal;
pub(crate) mod shield;

#[cfg(not(target_os = "macos"))]
use backends::native_forget;
use seal::*;
pub fn get(name: &str) -> Result<Option<(String, Source)>> {
    Store::discover()?.get(name)
}

pub fn set(name: &str, secret: &str) -> Result<Source> {
    Store::discover()?.set(name, secret)
}

pub(crate) fn forget(name: &str) -> Result<bool> {
    Store::discover()?.forget(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chacha20poly1305::KeyInit;
    use chacha20poly1305::aead::Aead;
    use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
    use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
    use std::fs;

    #[test]
    fn a_stored_key_is_plain_toml_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let store = Store::file_with_seal(file.clone(), [7_u8; KEY_LEN]);
        store.set("zai", "key-value").unwrap();
        let bytes = fs::read(&file).unwrap();
        assert!(!bytes.starts_with(MAGIC));
        assert!(String::from_utf8(bytes).unwrap().contains("zai"));
    }

    #[test]
    fn a_legacy_sealed_store_is_rewritten_plaintext_on_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let seal = [9_u8; KEY_LEN];
        let cipher = ChaCha20Poly1305::new(&Key::from(seal));
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let sealed = cipher
            .encrypt(&Nonce::from(nonce_bytes), b"zai = \"old-key\"".as_slice())
            .unwrap();
        let mut blob = MAGIC.to_vec();
        blob.extend_from_slice(&nonce_bytes);
        blob.extend(sealed);
        fs::write(&file, blob).unwrap();
        let store = Store::file_with_seal(file.clone(), seal);
        assert_eq!(
            store.get("zai").unwrap().map(|(value, _)| value),
            Some("old-key".to_string())
        );
        let bytes = fs::read(&file).unwrap();
        assert!(!bytes.starts_with(MAGIC));
        assert!(String::from_utf8(bytes).unwrap().contains("old-key"));
    }
}
