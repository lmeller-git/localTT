//! The `StoreBackup` capability for the SQLite store: online backup through
//! the backend's own snapshot mechanism, and durability flushing (RFC section
//! 4.3; STORE-7, STORE-12).

use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use localtt_engine::ports::store::{StoreBackup, StoreFault};
use rusqlite::{Connection, MAIN_DB};

use crate::store::sqlite::{SqliteStore, fault};

/// How many complete startup backups rotation keeps (STORE-3).
const KEEP_BACKUPS: usize = 5;

/// The suffix a backup carries until it is renamed into place (S9).
const TEMP_SUFFIX: &str = ".tmp";

impl StoreBackup for SqliteStore {
    #[inline]
    fn backup(&mut self, to: &Path) -> Result<(), StoreFault> {
        write_backup(self.connection(), to)
    }

    #[inline]
    fn sync(&mut self) -> Result<(), StoreFault> {
        // `wal_checkpoint` reports a blocked checkpoint in its result row, not as
        // an error, so `busy` must be inspected or a blocked call reads as
        // success (RFC section 5.4).
        let (busy, _, _): (i64, i64, i64) = self
            .connection()
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|error| fault("checkpointing the WAL", error))?;
        if busy != 0 {
            return Err(StoreFault::Unavailable {
                message: "the WAL checkpoint was blocked, so durability is not forced".to_owned(),
            });
        }
        Ok(())
    }
}

/// Writes the automatic startup backup next to the campaign file and rotates
/// old ones; called by `open_with` before DDL migrations run.
pub(crate) fn startup_backup(connection: &Connection, campaign: &Path) -> Result<(), StoreFault> {
    write_backup(connection, &backup_path(campaign, unix_seconds()))?;
    rotate(campaign);
    Ok(())
}

/// Captures the store through SQLite's online backup API: a consistent copy
/// written to a temporary name, then renamed into place (never a copy of the
/// live files, S9).
fn write_backup(connection: &Connection, to: &Path) -> Result<(), StoreFault> {
    let temporary = temporary_path(to);
    let _ = fs::remove_file(&temporary);
    if let Err(error) = connection.backup(MAIN_DB, &temporary, None) {
        let _ = fs::remove_file(&temporary);
        return Err(fault("writing a backup", error));
    }
    fs::rename(&temporary, to).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        StoreFault::Unavailable {
            message: format!("renaming the backup into place: {error}"),
        }
    })
}

/// Keeps the [`KEEP_BACKUPS`] newest complete startup backups of `campaign`.
///
/// Best-effort by design: this is directory hygiene after a backup has already
/// landed, so a listing or removal failure must not fail the `open` that
/// triggered it. Only regular files named exactly `<stem>.backup-<unix>.sqlite`
/// count: a `.tmp` file is in progress, a directory or anything else is foreign
/// (S9).
fn rotate(campaign: &Path) {
    let directory = parent_directory(campaign);
    let Some(stem) = campaign.file_stem() else {
        return;
    };
    let prefix = format!("{}.backup-", stem.to_string_lossy());

    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut complete = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(seconds) = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".sqlite"))
            .and_then(|stamp| stamp.parse::<u64>().ok())
        else {
            continue;
        };
        complete.push((seconds, entry.path()));
    }

    complete.sort_by_key(|(seconds, _)| *seconds);
    while complete.len() > KEEP_BACKUPS {
        let (_, path) = complete.remove(0);
        let _ = fs::remove_file(&path);
    }
}

/// The startup backup name: `<stem>.backup-<unix>.sqlite`.
fn backup_path(campaign: &Path, seconds: u64) -> PathBuf {
    let stem = campaign
        .file_stem()
        .unwrap_or_else(|| OsStr::new("campaign"));
    campaign.with_file_name(format!(
        "{}.backup-{seconds}.sqlite",
        stem.to_string_lossy()
    ))
}

/// The in-progress name: the target with [`TEMP_SUFFIX`] appended.
fn temporary_path(to: &Path) -> PathBuf {
    let mut name = to
        .file_name()
        .map_or_else(OsString::new, OsStr::to_os_string);
    name.push(TEMP_SUFFIX);
    to.with_file_name(name)
}

/// The directory a backup of `path` lives in; an empty parent means the cwd.
fn parent_directory(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Wall-clock seconds for the backup name.
fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use localtt_core::builders::CampaignBuilder;
    use localtt_engine::ports::store::{CampaignStore, StatePatch};
    use tempfile::TempDir;

    use super::*;
    use crate::store::sqlite::SqliteOptions;

    /// A temporary directory and the campaign path inside it.
    fn scenario() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("a temporary directory");
        let campaign = dir.path().join("campaign.sqlite");
        (dir, campaign)
    }

    /// The unix stamps of the complete `campaign.backup-*.sqlite` files.
    fn complete_backups(directory: &Path) -> Vec<u64> {
        let mut stamps: Vec<u64> = fs::read_dir(directory)
            .expect("the directory is listable")
            .filter_map(|entry| {
                let name = entry.expect("entries are readable").file_name();
                let name = name.to_str()?.to_owned();
                let stamp = name
                    .strip_prefix("campaign.backup-")?
                    .strip_suffix(".sqlite")?;
                stamp.parse().ok()
            })
            .collect();
        stamps.sort_unstable();
        stamps
    }

    #[test]
    fn rotation_keeps_the_five_newest_complete_files() {
        let (dir, campaign) = scenario();
        for seconds in 1..=7 {
            fs::write(
                dir.path().join(format!("campaign.backup-{seconds}.sqlite")),
                b"complete",
            )
            .expect("the backup is writable");
        }
        fs::write(
            dir.path().join("campaign.backup-8.sqlite.tmp"),
            b"in progress",
        )
        .expect("the temporary is writable");
        fs::write(dir.path().join("campaign.backup-latest.sqlite"), b"foreign")
            .expect("the foreign file is writable");
        fs::write(dir.path().join("other.backup-9.sqlite"), b"foreign")
            .expect("the foreign file is writable");

        rotate(&campaign);

        assert_eq!(complete_backups(dir.path()), vec![3, 4, 5, 6, 7]);
        assert!(dir.path().join("campaign.backup-8.sqlite.tmp").exists());
        assert!(dir.path().join("campaign.backup-latest.sqlite").exists());
        assert!(dir.path().join("other.backup-9.sqlite").exists());
    }

    #[test]
    fn a_backup_is_renamed_into_place_from_its_tmp_name() {
        let (dir, campaign) = scenario();
        let store = SqliteStore::open(&campaign).expect("a fresh store opens");
        let target = dir.path().join("copy.sqlite");

        write_backup(store.connection(), &target).expect("the backup is written");

        assert!(target.exists());
        assert!(!temporary_path(&target).exists());
        let copy = SqliteStore::open(&target).expect("the copy opens as a store");
        assert!(matches!(copy.load(), Err(StoreFault::NoCampaign)));
    }

    #[test]
    fn sync_truncates_the_wal() {
        let (_dir, campaign_path) = scenario();
        let mut store = SqliteStore::open(&campaign_path).expect("a fresh store opens");
        let campaign = CampaignBuilder::new("WAL fixture").build();
        store
            .apply(&StatePatch {
                campaign: Some(&campaign),
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: Vec::new(),
                feed_keep_from: None,
            })
            .expect("a campaign-only patch is accepted");
        let wal = PathBuf::from(format!("{}-wal", campaign_path.display()));
        assert!(
            fs::metadata(&wal).is_ok_and(|metadata| metadata.len() > 0),
            "the write waits in the WAL before the checkpoint"
        );

        StoreBackup::sync(&mut store).expect("an unblocked checkpoint succeeds");

        assert!(
            fs::metadata(&wal).map_or(true, |metadata| metadata.len() == 0),
            "a successful sync truncated the WAL"
        );
    }

    #[test]
    fn sync_reports_a_blocked_checkpoint() {
        let (_dir, campaign_path) = scenario();
        let mut store = SqliteStore::open(&campaign_path).expect("a fresh store opens");
        let campaign = CampaignBuilder::new("WAL fixture").build();
        store
            .apply(&StatePatch {
                campaign: Some(&campaign),
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: Vec::new(),
                feed_keep_from: None,
            })
            .expect("a campaign-only patch is accepted");
        // A second connection holding a read transaction blocks a TRUNCATE
        // checkpoint.
        let mut reader = Connection::open(&campaign_path).expect("a second connection opens");
        let transaction = reader.transaction().expect("a read transaction starts");
        let _: i64 = transaction
            .query_row("SELECT count(*) FROM campaign", [], |row| row.get(0))
            .expect("the read starts the snapshot");
        store
            .connection()
            .busy_timeout(Duration::ZERO)
            .expect("the timeout is settable");

        let blocked = StoreBackup::sync(&mut store)
            .expect_err("a blocked checkpoint is not reported as success");

        assert!(
            matches!(blocked, StoreFault::Unavailable { .. }),
            "a blocked checkpoint is Unavailable, got {blocked:?}"
        );
        drop(transaction);
    }

    #[test]
    fn rotation_tolerates_an_unprunable_obstacle() {
        let (dir, campaign) = scenario();
        drop(SqliteStore::open(&campaign).expect("a fresh store opens"));
        for seconds in 2..=6 {
            fs::write(
                dir.path().join(format!("campaign.backup-{seconds}.sqlite")),
                b"complete",
            )
            .expect("the fabricated backup is writable");
        }
        // A directory named like the oldest backup sorts into the rotation set
        // but cannot be removed; the open must not fail because of it.
        let obstacle = dir.path().join("campaign.backup-1.sqlite");
        fs::create_dir_all(&obstacle).expect("the obstacle directory is creatable");

        let _store = SqliteStore::open_with(
            &campaign,
            SqliteOptions {
                startup_backup: true,
            },
        )
        .expect("an unprunable rotation entry does not fail the open");

        assert!(obstacle.is_dir(), "the obstacle is left alone");
        assert!(
            !dir.path().join("campaign.backup-2.sqlite").exists(),
            "rotation still pruned the oldest real backup"
        );
    }
}
