//! Small secrets: the local event key, device identity, and family
//! membership, kept as owner-only files in the app's data directory. On Android
//! that is the app-private storage, which other apps cannot read. Moving the
//! keys into the hardware-backed Android Keystore is still to do.

use std::path::{Path, PathBuf};

use anyhow::Result;

#[derive(Clone, Debug)]
pub struct Secrets {
    dir: PathBuf,
}

impl Secrets {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("secrets"),
        }
    }
}

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
