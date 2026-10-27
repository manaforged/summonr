#[cfg(any(target_os = "macos", all(test, unix)))]
use std::path::Path;

#[cfg(any(target_os = "macos", all(test, unix)))]
use toml_edit::value;

use super::seal::lookup;
#[cfg(any(target_os = "macos", all(test, unix)))]
use super::{
    SERVICE,
    seal::{find, security},
};
use super::{Source, Store};
use crate::durable::store;
#[cfg(not(target_os = "macos"))]
use crate::error::Error;
use crate::error::Result;

impl Store {
    pub(super) fn file_get(&self, name: &str) -> Result<Option<(String, Source)>> {
        let document = store::memo(&self.file, || self.locked(|| self.read_file()))?;
        Ok(lookup(&document, name).map(|secret| (secret, Source::File)))
    }

    #[cfg(any(target_os = "macos", all(test, unix)))]
    fn blob_lookup(&self, tool: &Path, name: &str) -> Result<Option<String>> {
        let blob = store::memo(&self.file, || self.read_blob(tool))?;
        Ok(lookup(&blob, name))
    }

    #[cfg(any(target_os = "macos", all(test, unix)))]
    pub(super) fn security_get(&self, tool: &Path, name: &str) -> Result<Option<(String, Source)>> {
        if let Some(secret) = self.blob_lookup(tool, name)? {
            return Ok(Some((secret, Source::Keychain)));
        }
        let Some(secret) = find(tool, name)? else {
            return Ok(None);
        };
        drop(self.locked(|| {
            let mut blob = self.read_blob(tool)?;
            blob[name] = value(&secret);
            self.write_blob(tool, &blob)?;
            security(
                tool,
                &["delete-generic-password", "-s", SERVICE, "-a", name],
                None,
            )
            .map(drop)
        }));
        store::forget(&self.file);
        Ok(Some((secret, Source::Keychain)))
    }

    pub(super) fn file_remove(&self, name: &str) -> Result<bool> {
        let mut document = self.read_file()?;
        if document.remove(name).is_none() {
            return Ok(false);
        }
        self.write_file(&document)?;
        Ok(true)
    }

    #[cfg(any(target_os = "macos", all(test, unix)))]
    pub(super) fn blob_remove(&self, tool: &Path, name: &str) -> Result<bool> {
        let mut blob = self.read_blob(tool)?;
        if blob.remove(name).is_none() {
            return Ok(false);
        }
        self.write_blob(tool, &blob)?;
        Ok(true)
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn native_forget(name: &str) -> Result<bool> {
    match super::native_entry(name)?.delete_credential() {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(Error::Config(format!("credential store: {error}"))),
    }
}
