//! Data-root resolution, directory layout and the single-instance lock
//! (`docs/architecture.md` section 6; STORE-3/15).

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use fs2::{FileExt, lock_contended_error};
use thiserror::Error;
use uuid::Uuid;

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

    /// The per-run HTTP token file: `<root>/server.token`.
    #[must_use]
    #[inline]
    pub fn server_token(&self) -> PathBuf {
        self.root.join("server.token")
    }

    /// Where the adopted pre-M5 token file is moved: `<root>/server.token.migrated`.
    #[must_use]
    #[inline]
    pub fn migrated_server_token(&self) -> PathBuf {
        self.root.join("server.token.migrated")
    }
}

/// Writes `token` as the per-run HTTP token, owner-only where the platform
/// supports Unix permissions.
///
/// # Errors
///
/// [`ConfigError::Io`] when the file cannot be created, written or have its
/// permissions tightened.
#[inline]
pub fn write_server_token(path: &Path, token: &str) -> Result<(), ConfigError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|source| ConfigError::Io {
                path: path.to_path_buf(),
                source,
            })?;
    }
    file.write_all(token.as_bytes())
        .map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.flush().map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// `<campaign>.dm.key`, e.g. `default.sqlite.dm.key`.
#[must_use]
#[inline]
pub fn dm_key_path(campaign: &Path) -> PathBuf {
    let mut name = campaign
        .file_name()
        .map_or_else(|| "campaign".into(), |name| name.to_os_string());
    name.push(".dm.key");
    campaign.with_file_name(name)
}

/// Reads a secret file, returning `None` when it does not exist.
///
/// The caller trims and validates the contents; this helper only turns a
/// missing file into a value instead of an error.
///
/// # Errors
///
/// [`ConfigError::Io`] for a path that exists but cannot be read.
#[inline]
pub fn read_secret(path: &Path) -> Result<Option<String>, ConfigError> {
    match fs::read_to_string(path) {
        Ok(secret) => Ok(Some(secret)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// A secret written to a same-directory temporary file, not yet visible at its
/// final path.
///
/// [`Self::write`] creates the temporary file with owner-only permissions and
/// fsyncs it; [`Self::commit`] makes it visible with one atomic rename. The
/// temporary file is removed on drop, so a failed engine commit leaves nothing
/// behind. On Unix the parent directory is fsynced after the rename; other
/// platforms rely on the rename itself.
#[derive(Debug)]
pub struct StagedSecret {
    temp: PathBuf,
    target: PathBuf,
}

impl StagedSecret {
    /// Stages `secret` beside `target`.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Io`] when the temporary file cannot be created, written
    /// or fsynced.
    #[inline]
    pub fn write(target: &Path, secret: &str) -> Result<Self, ConfigError> {
        let temp = temporary_path(target);
        let staged = Self {
            temp,
            target: target.to_path_buf(),
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&staged.temp)
            .map_err(|source| ConfigError::Io {
                path: staged.temp.clone(),
                source,
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|source| ConfigError::Io {
                    path: staged.temp.clone(),
                    source,
                })?;
        }
        file.write_all(secret.as_bytes())
            .map_err(|source| ConfigError::Io {
                path: staged.temp.clone(),
                source,
            })?;
        file.sync_all().map_err(|source| ConfigError::Io {
            path: staged.temp.clone(),
            source,
        })?;
        Ok(staged)
    }

    /// Atomically renames the staged secret over its target.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Io`] when the rename or the parent-directory fsync fails.
    /// The temporary file is removed on failure.
    #[inline]
    pub fn commit(self) -> Result<(), ConfigError> {
        fs::rename(&self.temp, &self.target).map_err(|source| ConfigError::Io {
            path: self.target.clone(),
            source,
        })?;
        #[cfg(unix)]
        {
            let parent = self.target.parent().unwrap_or_else(|| Path::new("."));
            let directory = File::open(parent).map_err(|source| ConfigError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
            directory.sync_all().map_err(|source| ConfigError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        Ok(())
    }
}

impl Drop for StagedSecret {
    #[inline]
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temp);
    }
}

fn temporary_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map_or_else(|| "secret".into(), |name| name.to_os_string());
    let mut temp = std::ffi::OsString::from(".");
    temp.push(name);
    temp.push(format!(".tmp-{}", Uuid::new_v4().simple()));
    target.with_file_name(temp)
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
            Err(source) if source.raw_os_error() == lock_contended_error().raw_os_error() => {
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
    /// The DM key file exists but is empty.
    #[error("{path} is empty; delete it or pass --rotate-dm-key")]
    DmKeyEmpty {
        /// The key file.
        path: PathBuf,
    },
    /// The DM key file does not match the selected DM seat.
    #[error("{path} does not match the DM seat's token hash; pass --rotate-dm-key to replace it")]
    DmKeyMismatch {
        /// The key file.
        path: PathBuf,
    },
    /// The DM seat has a hash but the key file is gone.
    #[error(
        "{path} is missing while the DM seat already has a key; restore it or pass --rotate-dm-key"
    )]
    DmKeyMissing {
        /// The expected key file.
        path: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{
        CampaignLock,
        ConfigError,
        DataPaths,
        StagedSecret,
        dm_key_path,
        lock_path,
        read_secret,
        write_server_token,
    };

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
    fn the_server_token_path_is_under_the_data_root() {
        let dir = tempdir().expect("a temporary directory");
        let paths = DataPaths::resolve(Some(dir.path())).expect("an explicit root resolves");
        assert_eq!(paths.server_token(), dir.path().join("server.token"));
        assert_eq!(
            paths.migrated_server_token(),
            dir.path().join("server.token.migrated")
        );
    }

    #[test]
    fn the_dm_key_lives_beside_the_campaign() {
        let campaign = std::path::Path::new("/data/campaigns/default.sqlite");
        assert_eq!(
            dm_key_path(campaign),
            std::path::PathBuf::from("/data/campaigns/default.sqlite.dm.key")
        );
    }

    #[test]
    fn reading_a_missing_secret_is_not_an_error() {
        let dir = tempdir().expect("a temporary directory");
        assert_eq!(
            read_secret(&dir.path().join("missing")).expect("a missing file is absent"),
            None
        );
    }

    #[test]
    fn a_staged_secret_becomes_visible_only_on_commit() {
        // Arrange
        let dir = tempdir().expect("a temporary directory");
        let path = dir.path().join("campaign.sqlite.dm.key");

        // Act
        let staged = StagedSecret::write(&path, "dm-secret").expect("the secret stages");
        assert!(!path.exists(), "the target is untouched before the commit");
        staged.commit().expect("the rename succeeds");

        // Assert
        assert_eq!(
            read_secret(&path)
                .expect("the key file reads")
                .expect("the key file exists"),
            "dm-secret"
        );
        let leftovers = std::fs::read_dir(dir.path())
            .expect("the directory reads")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != "campaign.sqlite.dm.key")
            .count();
        assert_eq!(leftovers, 0, "the temporary file is gone");
    }

    #[test]
    fn dropping_a_staged_secret_removes_the_temporary_file() {
        // Arrange
        let dir = tempdir().expect("a temporary directory");
        let path = dir.path().join("campaign.sqlite.dm.key");
        let staged = StagedSecret::write(&path, "dm-secret").expect("the secret stages");

        // Act
        drop(staged);

        // Assert
        assert!(!path.exists(), "an abandoned secret is never published");
        assert_eq!(
            std::fs::read_dir(dir.path())
                .expect("the directory reads")
                .count(),
            0
        );
    }

    #[test]
    fn committing_a_staged_secret_replaces_an_existing_key() {
        // Arrange
        let dir = tempdir().expect("a temporary directory");
        let path = dir.path().join("campaign.sqlite.dm.key");
        write_server_token(&path, "old-key").expect("the initial key writes");

        // Act
        StagedSecret::write(&path, "new-key")
            .expect("the replacement stages")
            .commit()
            .expect("the rename succeeds");

        // Assert
        assert_eq!(
            read_secret(&path)
                .expect("the key file reads")
                .expect("the key file exists"),
            "new-key"
        );
    }

    #[test]
    fn a_staged_secret_cannot_be_written_to_a_missing_directory() {
        // Arrange
        let dir = tempdir().expect("a temporary directory");
        let path = dir.path().join("missing/campaign.sqlite.dm.key");

        // Act
        let result = StagedSecret::write(&path, "dm-secret");

        // Assert
        assert!(matches!(result, Err(ConfigError::Io { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn a_staged_secret_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        // Arrange
        let dir = tempdir().expect("a temporary directory");
        let path = dir.path().join("campaign.sqlite.dm.key");

        // Act
        StagedSecret::write(&path, "dm-secret")
            .expect("the secret stages")
            .commit()
            .expect("the rename succeeds");

        // Assert
        let mode = std::fs::metadata(&path)
            .expect("the key file has metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn the_server_token_file_is_written() {
        let dir = tempdir().expect("a temporary directory");
        let path = dir.path().join("server.token");
        write_server_token(&path, "secret-token").expect("the token file writes");
        assert_eq!(
            std::fs::read_to_string(&path).expect("the token file reads"),
            "secret-token"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_server_token_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("a temporary directory");
        let path = dir.path().join("server.token");
        write_server_token(&path, "secret-token").expect("the token file writes");
        let mode = std::fs::metadata(&path)
            .expect("the token file has metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
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
