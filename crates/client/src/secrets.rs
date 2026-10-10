//! Small secrets: the local event key, device identity, and family
//! membership. Android keeps them in the platform Keystore. Other targets are
//! development builds and keep them as owner-only files in the data directory,
//! so several desktop instances can run side by side.

use std::path::{Path, PathBuf};

use anyhow::Result;

#[derive(Clone, Debug)]
pub struct Secrets {
    #[cfg_attr(target_os = "android", allow(dead_code))]
    dir: PathBuf,
}

impl Secrets {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("secrets"),
        }
    }
}

#[cfg(not(target_os = "android"))]
impl Secrets {
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.dir.join(name)) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn write(&self, name: &str, value: &[u8]) -> Result<()> {
        use std::io::Write;
        std::fs::create_dir_all(&self.dir)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(self.dir.join(name))?.write_all(value)?;
        Ok(())
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        match std::fs::remove_file(self.dir.join(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(target_os = "android")]
impl Secrets {
    fn entry(name: &str) -> Result<keyring_core::Entry> {
        static STORE: std::sync::Once = std::sync::Once::new();
        let mut failure = None;
        STORE.call_once(|| match android_native_keyring_store::Store::new() {
            Ok(store) => keyring_core::set_default_store(store),
            Err(error) => failure = Some(error),
        });
        if let Some(error) = failure {
            return Err(error.into());
        }
        Ok(keyring_core::Entry::new("tackly", name)?)
    }

    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        match Self::entry(name)?.get_secret() {
            Ok(value) => Ok(Some(value)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn write(&self, name: &str, value: &[u8]) -> Result<()> {
        Self::entry(name)?.set_secret(value)?;
        Ok(())
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
