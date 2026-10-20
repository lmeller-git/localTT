//! Data-root resolution, directory layout and the single-instance lock
//! (`docs/rfcs/server.md` 4.5, STORE-3/15).

use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use fs2::FileExt;
use thiserror::Error;

/// The on-disk data root and its standard subdirectories.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataPaths {
    /// Root of the local data directory.
    pub root: PathBuf,
    /// Campaign stores.
    pub campaigns: PathBuf,
    /// Runtime reference snapshots.
    pub reference: PathBuf,
    /// Backups (the SQLite store also writes backups beside the campaign).
    pub backups: PathBuf,
}

impl DataPaths {
    /// Resolves `--data`, or the platform local data directory.
    ///
    /// # Errors
    ///
    /// [`ConfigError::NoDataDir`] when the platform has no local data
    /// directory and no explicit root was passed.
    #[inline]
    pub fn resolve(explicit: Option<&Path>) -> Result<Self, ConfigError> {
        let root = match explicit {
            Some(path) => path.to_path_buf(),
            None => ProjectDirs::from("", "", "localtt")
                .ok_or(ConfigError::NoDataDir)?
                .data_dir()
                .to_path_buf(),
        };
        Ok(Self {
            campaigns: root.join("campaigns"),
            reference: root.join("reference"),
            backups: root.join("backups"),
            root,
        })
    }

    /// Creates the data layout when missing.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Io`] when a directory cannot be created.
    #[inline]
    pub fn ensure(&self) -> Result<(), ConfigError> {
        for path in [&self.root, &self.campaigns, &self.reference, &self.backups] {
            fs::create_dir_all(path).map_err(|source| ConfigError::Io {
                path: path.clone(),
                source,
            })?;
        }
        Ok(())
    }

    /// The default campaign store: `<root>/campaigns/default.sqlite`.
    #[must_use]
    #[inline]
    pub fn default_campaign(&self) -> PathBuf {
        self.campaigns.join("default.sqlite")
    }
}

/// An exclusive advisory lock for one campaign file.
///
/// The lock lives for the process lifetime; dropping it unlocks. The lock file
/// is `<campaign>.lock`, next to the store (not inside it).
#[derive(Debug)]
pub struct CampaignLock {
    file: File,
}

impl CampaignLock {
    /// Takes the exclusive lock, failing fast when another process holds it.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Locked`] when the lock is held, or [`ConfigError::Io`]
    /// when the lock file cannot be opened.
    #[inline]
    pub fn acquire(campaign: &Path) -> Result<Self, ConfigError> {
        let path = lock_path(campaign);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| ConfigError::Io {
                path: path.clone(),
                source,
            })?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Self { file }),
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                Err(ConfigError::Locked { path })
            }
            Err(source) => Err(ConfigError::Io { path, source }),
        }
    }
}

impl Drop for CampaignLock {
    #[inline]
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// `<campaign>.lock`, e.g. `default.sqlite.lock`.
#[must_use]
#[inline]
pub fn lock_path(campaign: &Path) -> PathBuf {
    let mut name = campaign
        .file_name()
        .map_or_else(|| "campaign".into(), |name| name.to_os_string());
    name.push(".lock");
    campaign.with_file_name(name)
}

/// Failure resolving or securing the data root.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The platform has no local data directory; `--data` is required.
    #[error("cannot determine the local data directory; pass --data")]
    NoDataDir,
    /// A filesystem operation failed.
    #[error("{path}: {source}")]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// Another process holds the campaign lock.
    #[error("another localtt process holds {path}")]
    Locked {
        /// The lock file.
        path: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{CampaignLock, ConfigError, DataPaths, lock_path};

    #[test]
    fn explicit_roots_create_the_standard_layout() {
        let dir = tempdir().expect("a temporary directory");
        let paths = DataPaths::resolve(Some(dir.path())).expect("an explicit root resolves");
        paths.ensure().expect("the layout is created");

        assert!(paths.campaigns.is_dir());
        assert!(paths.reference.is_dir());
        assert!(paths.backups.is_dir());
        assert_eq!(
            paths.default_campaign(),
            dir.path().join("campaigns/default.sqlite")
        );
    }

    #[test]
    fn the_lock_is_exclusive_and_released_on_drop() {
        let dir = tempdir().expect("a temporary directory");
        let campaign = dir.path().join("campaign.sqlite");
        assert_eq!(
            lock_path(&campaign),
            dir.path().join("campaign.sqlite.lock")
        );

        let first = CampaignLock::acquire(&campaign).expect("the first lock succeeds");
        assert!(matches!(
            CampaignLock::acquire(&campaign),
            Err(ConfigError::Locked { .. })
        ));
        drop(first);

        CampaignLock::acquire(&campaign).expect("the lock is released on drop");
    }

    #[test]
    fn a_lock_file_in_a_missing_directory_is_an_io_error() {
        let dir = tempdir().expect("a temporary directory");
        let missing = dir.path().join("missing/campaign.sqlite");
        assert!(matches!(
            CampaignLock::acquire(&missing),
            Err(ConfigError::Io { .. })
        ));
    }
}
