//! Authority-owned SQLite plan storage. The MCP process never opens this file.
use crate::planning::{Plan, PlanChange, PlanError};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::{
    fs,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug)]
pub enum DbError {
    Sql(rusqlite::Error),
    Plan(PlanError),
    Invalid(&'static str),
    NotFound,
    AlreadyExists,
}
impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sql(e) => write!(f, "database error: {e}"),
            Self::Plan(e) => write!(f, "plan rejected: {e}"),
            Self::Invalid(s) => write!(f, "unsafe database configuration: {s}"),
            Self::NotFound => write!(f, "plan has not been created"),
            Self::AlreadyExists => write!(f, "a plan already exists"),
        }
    }
}
impl std::error::Error for DbError {}
impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
impl From<PlanError> for DbError {
    fn from(e: PlanError) -> Self {
        Self::Plan(e)
    }
}

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
}
impl Database {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if !path.is_absolute() {
            return Err(DbError::Invalid("database path must be absolute"));
        }
        if path.file_name().is_none() {
            return Err(DbError::Invalid("missing filename"));
        }
        let parent = fs::canonicalize(path.parent().ok_or(DbError::Invalid("missing parent"))?)
            .map_err(|_| DbError::Invalid("database directory does not exist"))?;
        let meta = fs::metadata(&parent)
            .map_err(|_| DbError::Invalid("database directory inaccessible"))?;
        if !meta.is_dir() || meta.mode() & 0o077 != 0 || meta.uid() != unsafe { libc::geteuid() } {
            return Err(DbError::Invalid(
                "database directory must be owned by authority and mode 0700",
            ));
        }
        let path = parent.join(path.file_name().unwrap());
        if fs::symlink_metadata(&path).is_err() {
            // Create privately before SQLite opens it, regardless of caller umask.
            let _ = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .map_err(|_| DbError::Invalid("cannot create private database file"))?;
        }
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if !meta.is_file()
                || meta.mode() & 0o077 != 0
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.nlink() != 1
            {
                return Err(DbError::Invalid(
                    "database file must be private regular file, owned by authority",
                ));
            }
        }
        let this = Self { path };
        let con = this.connect()?;
        let current: i32 = con.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        match current {
            0 => {
                con.execute_batch(
                    "BEGIN IMMEDIATE;
                    CREATE TABLE IF NOT EXISTS plans (
                        singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                        revision INTEGER NOT NULL CHECK(revision>0),
                        payload TEXT NOT NULL
                    );
                    PRAGMA user_version=1;
                    COMMIT;",
                )?;
            }
            1 => {}
            _ => return Err(DbError::Invalid("unknown schema version")),
        }
        Ok(this)
    }
    fn connect(&self) -> Result<Connection, DbError> {
        let con = Connection::open(&self.path)?;
        con.busy_timeout(Duration::from_secs(3))?;
        con.execute_batch(
            "PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF; PRAGMA journal_mode=DELETE;",
        )?;
        Ok(con)
    }
    fn existing(con: &Connection) -> Result<Option<Plan>, DbError> {
        let result: Option<(i64, String)> = con
            .query_row(
                "SELECT revision,payload FROM plans WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        result
            .map(|(revision, payload)| {
                let plan = Plan::from_json(&payload)?;
                if i64::try_from(plan.revision()).ok() != Some(revision) {
                    return Err(DbError::Invalid("revision and JSON disagree"));
                }
                Ok(plan)
            })
            .transpose()
    }
    pub fn get(&self) -> Result<Plan, DbError> {
        let con = self.connect()?;
        Self::existing(&con)?.ok_or(DbError::NotFound)
    }
    pub fn create(&self, title: String, description: String) -> Result<Plan, DbError> {
        let mut con = self.connect()?;
        let tx = con.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if Self::existing(&tx)?.is_some() {
            return Err(DbError::AlreadyExists);
        }
        let plan = Plan::new(title, description, Utc::now())?;
        tx.execute(
            "INSERT INTO plans(singleton,revision,payload) VALUES (1,?1,?2)",
            params![plan.revision(), plan.to_json_pretty()?],
        )?;
        tx.commit()?;
        Ok(plan)
    }
    pub fn apply(
        &self,
        revision: u64,
        transform: impl FnOnce(&Plan) -> Result<PlanChange, PlanError>,
    ) -> Result<Plan, DbError> {
        let mut con = self.connect()?;
        let tx = con.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut plan = Self::existing(&tx)?.ok_or(DbError::NotFound)?;
        if plan.revision() != revision {
            return Err(DbError::Plan(PlanError::RevisionConflict {
                expected: revision,
                actual: plan.revision(),
            }));
        }
        let change = transform(&plan)?;
        plan.apply(revision, Utc::now(), change)?;
        let rows = tx.execute(
            "UPDATE plans SET revision=?1,payload=?2 WHERE singleton=1 AND revision=?3",
            params![plan.revision(), plan.to_json_pretty()?, revision],
        )?;
        if rows != 1 {
            return Err(DbError::Invalid("revision changed during transaction"));
        }
        tx.commit()?;
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, Database) {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let db = Database::open(&root.path().join("plans.sqlite")).unwrap();
        (root, db)
    }

    #[test]
    fn transactions_preserve_revisions_and_roll_back_bad_edits() {
        let (_root, db) = fixture();
        assert!(matches!(db.get(), Err(DbError::NotFound)));
        db.create("Plan".into(), "".into()).unwrap();
        assert!(matches!(
            db.create("Again".into(), "".into()),
            Err(DbError::AlreadyExists)
        ));
        let next = db
            .apply(1, |_| {
                Ok(PlanChange::SetDetails {
                    title: "Updated".into(),
                    description: "".into(),
                })
            })
            .unwrap();
        assert_eq!(next.revision(), 2);
        let previous = db.get().unwrap();
        assert!(matches!(
            db.apply(1, |_| Ok(PlanChange::SetDetails {
                title: "Stale".into(),
                description: "".into()
            })),
            Err(DbError::Plan(PlanError::RevisionConflict { .. }))
        ));
        assert!(matches!(
            db.apply(2, |_| Ok(PlanChange::SetDetails {
                title: "".into(),
                description: "".into()
            })),
            Err(DbError::Plan(PlanError::Invalid(_)))
        ));
        assert_eq!(db.get().unwrap(), previous);
    }

    #[test]
    fn simultaneous_editors_cannot_both_commit_same_revision() {
        let (_root, db) = fixture();
        db.create("Plan".into(), "".into()).unwrap();
        let a = db.clone();
        let b = db.clone();
        let first = std::thread::spawn(move || {
            a.apply(1, |_| {
                Ok(PlanChange::SetDetails {
                    title: "First".into(),
                    description: "".into(),
                })
            })
        });
        let second = std::thread::spawn(move || {
            b.apply(1, |_| {
                Ok(PlanChange::SetDetails {
                    title: "Second".into(),
                    description: "".into(),
                })
            })
        });
        let result_a = first.join().unwrap();
        let result_b = second.join().unwrap();
        assert_ne!(result_a.is_ok(), result_b.is_ok());
        assert_eq!(db.get().unwrap().revision(), 2);
    }

    #[test]
    fn refuses_unsafe_permissions_and_schema_versions() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("plans.sqlite");
        assert!(matches!(Database::open(&path), Err(DbError::Invalid(_))));
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let outside = root.path().join("other");
        fs::write(&outside, "bad").unwrap();
        let alias = root.path().join("alias");
        symlink(&outside, &alias).unwrap();
        assert!(matches!(Database::open(&alias), Err(DbError::Invalid(_))));
        let db = Database::open(&path).unwrap();
        db.create("A".into(), "".into()).unwrap();
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(meta.mode() & 0o777, 0o600);
        let raw = Connection::open(&path).unwrap();
        raw.execute_batch("PRAGMA user_version=75").unwrap();
        assert!(matches!(
            Database::open(&path),
            Err(DbError::Invalid("unknown schema version"))
        ));
    }
}
