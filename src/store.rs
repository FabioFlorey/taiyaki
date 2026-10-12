//! Local, single-plan persistence for the initial stdio MCP adapter.
//! This is NOT a trusted authority database, OS sandbox or audit ledger.

use crate::planning::{Plan, PlanChange, PlanError};
use chrono::Utc;
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Plan(PlanError),
    UnsafePath(&'static str),
    NotFound,
    AlreadyExists,
    Busy,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "storage error: {err}"),
            Self::Plan(err) => write!(formatter, "plan rejected: {err}"),
            Self::UnsafePath(reason) => write!(formatter, "unsafe plan location: {reason}"),
            Self::NotFound => write!(formatter, "plan has not been created"),
            Self::AlreadyExists => write!(formatter, "a plan already exists"),
            Self::Busy => write!(formatter, "plan is busy; retry the operation"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<PlanError> for StoreError {
    fn from(value: PlanError) -> Self {
        Self::Plan(value)
    }
}

/// The path is fixed at process launch, never supplied by an MCP tool caller.
#[derive(Clone, Debug)]
pub struct PlanStore {
    path: PathBuf,
    lock: PathBuf,
}

impl PlanStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let requested = path.as_ref();
        if !requested.is_absolute() {
            return Err(StoreError::UnsafePath("plan file must be an absolute path"));
        }
        let name = requested
            .file_name()
            .filter(|name| !name.is_empty())
            .ok_or(StoreError::UnsafePath("missing plan filename"))?;
        let parent = requested
            .parent()
            .ok_or(StoreError::UnsafePath("missing plan directory"))?;
        let parent = fs::canonicalize(parent)?;
        let parent_meta = fs::metadata(&parent)?;
        if !parent_meta.is_dir() {
            return Err(StoreError::UnsafePath("plan parent is not a directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if parent_meta.permissions().mode() & 0o077 != 0 {
                return Err(StoreError::UnsafePath(
                    "plan directory must deny group and other access (mode 0700)",
                ));
            }
        }
        // The lock file stays at a fixed name across atomic replacement of the
        // plan file. Locking the replaced inode would not serialize writers.
        let lock = parent.join(format!(".{}.lock", name.to_string_lossy()));
        let path = parent.join(name);
        Self::assert_safe_file(&path)?;
        Self::assert_safe_file(&lock)?;
        Ok(Self { path, lock })
    }

    fn assert_safe_file(path: &Path) -> Result<(), StoreError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if !metadata.is_file() {
                    return Err(StoreError::UnsafePath("non-regular file or symlink"));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o077 != 0 {
                        return Err(StoreError::UnsafePath(
                            "plan and lock files must deny group and other access",
                        ));
                    }
                }
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StoreError::Io(error)),
        }
    }

    fn with_lock<T>(
        &self,
        action: impl FnOnce() -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        Self::assert_safe_file(&self.lock)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        options.mode(0o600);
        let guard = options.open(&self.lock)?;
        Self::assert_safe_file(&self.lock)?;
        guard.try_lock_exclusive().map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                StoreError::Busy
            } else {
                StoreError::Io(error)
            }
        })?;
        let result = action();
        // Drop releases the OS lock on all paths (also when action fails).
        drop(guard);
        result
    }

    fn read_unlocked(&self) -> Result<Plan, StoreError> {
        Self::assert_safe_file(&self.path)?;
        let bytes = match fs::read(&self.path) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(StoreError::NotFound)
            }
            Err(error) => return Err(StoreError::Io(error)),
        };
        if bytes.len() > 1_048_576 {
            return Err(StoreError::UnsafePath("plan file exceeds input limit"));
        }
        let json = std::str::from_utf8(&bytes)
            .map_err(|_| StoreError::UnsafePath("plan file is not UTF-8"))?;
        Ok(Plan::from_json(json)?)
    }

    fn write_unlocked(&self, plan: &Plan) -> Result<(), StoreError> {
        // JSON size is bounded in the model; no write until fully validated.
        let data = plan.to_json_pretty()?;
        Self::assert_safe_file(&self.path)?;
        let parent = self
            .path
            .parent()
            .ok_or(StoreError::UnsafePath("missing plan directory"))?;
        let name = format!(".taiyaki-{}.tmp", Uuid::new_v4());
        let temporary = parent.join(name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let result = (|| -> io::Result<()> {
            let mut file = options.open(&temporary)?;
            file.write_all(data.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)?;
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result.map_err(StoreError::Io)
    }

    pub fn get(&self) -> Result<Plan, StoreError> {
        self.with_lock(|| self.read_unlocked())
    }

    pub fn create(&self, title: String, description: String) -> Result<Plan, StoreError> {
        self.with_lock(|| {
            match self.read_unlocked() {
                Err(StoreError::NotFound) => {}
                Err(error) => return Err(error),
                Ok(_) => return Err(StoreError::AlreadyExists),
            }
            let plan = Plan::new(title, description, Utc::now())?;
            self.write_unlocked(&plan)?;
            Ok(plan)
        })
    }

    pub fn apply(
        &self,
        expected_revision: u64,
        transform: impl FnOnce(&Plan) -> Result<PlanChange, StoreError>,
    ) -> Result<Plan, StoreError> {
        self.with_lock(|| {
            let mut plan = self.read_unlocked()?;
            // Check BEFORE producing the edit so no stale request performs
            // preparatory side effects, even though edits are currently pure.
            if plan.revision() != expected_revision {
                return Err(StoreError::Plan(PlanError::RevisionConflict {
                    expected: expected_revision,
                    actual: plan.revision(),
                }));
            }
            let edit = transform(&plan)?;
            plan.apply(expected_revision, Utc::now(), edit)?;
            self.write_unlocked(&plan)?;
            Ok(plan)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::WorkItem;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, PlanStore) {
        let root = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let store = PlanStore::open(root.path().join("plan.json")).unwrap();
        (root, store)
    }

    #[test]
    fn create_edit_reopen_persists_between_instances() {
        let (_dir, store) = fixture();
        assert!(matches!(store.get(), Err(StoreError::NotFound)));
        let created = store.create("Build MCP".into(), "".into()).unwrap();
        assert_eq!(created.revision(), 1);
        assert!(matches!(
            store.create("Overwrite".into(), "".into()),
            Err(StoreError::AlreadyExists)
        ));
        let updated = store
            .apply(1, |_| {
                Ok(PlanChange::Add(WorkItem::new("Read plan", "", Utc::now())))
            })
            .unwrap();
        assert_eq!(updated.revision(), 2);
        let reopened = PlanStore::open(&store.path).unwrap().get().unwrap();
        assert_eq!(updated, reopened);
        assert_eq!(reopened.work_items().len(), 1);
    }

    #[test]
    fn stale_and_invalid_edits_leave_disk_untouched() {
        let (_dir, store) = fixture();
        store.create("Plan".into(), "".into()).unwrap();
        let old = fs::read(&store.path).unwrap();
        assert!(matches!(
            store.apply(0, |_| Ok(PlanChange::SetDetails {
                title: "No".into(),
                description: "".into()
            })),
            Err(StoreError::Plan(PlanError::RevisionConflict { .. }))
        ));
        assert!(matches!(
            store.apply(1, |_| Ok(PlanChange::SetDetails {
                title: "".into(),
                description: "".into()
            })),
            Err(StoreError::Plan(PlanError::Invalid(_)))
        ));
        assert_eq!(old, fs::read(&store.path).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_and_insecure_directory() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = tempfile::tempdir().unwrap();
        let ordinary = root.path().join("ordinary");
        fs::write(&ordinary, "data").unwrap();
        let alias = root.path().join("alias");
        symlink(&ordinary, &alias).unwrap();
        assert!(matches!(
            PlanStore::open(&alias),
            Err(StoreError::UnsafePath(_))
        ));
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            PlanStore::open(root.path().join("plan.json")),
            Err(StoreError::UnsafePath(_))
        ));
    }

    #[test]
    fn rejects_corrupt_existing_plan_without_overwriting() {
        let (_dir, store) = fixture();
        fs::write(&store.path, "not json").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&store.path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(matches!(
            store.create("Overwrite".into(), "".into()),
            Err(StoreError::Plan(PlanError::Json(_)))
        ));
        assert_eq!(fs::read(&store.path).unwrap(), b"not json");
    }
}
