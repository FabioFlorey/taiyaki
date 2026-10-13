//! Authority-owned SQLite plan storage. The MCP process never opens this file.
use crate::planning::{Plan, PlanChange, PlanError};
use crate::workflow::{Assignment, GateObservation, TestGate, Workflow};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::{
    fs,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

#[derive(Debug)]
pub enum DbError {
    Sql(rusqlite::Error),
    Plan(PlanError),
    Invalid(&'static str),
    NotFound,
    AlreadyExists,
    Workflow(String),
}
impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sql(e) => write!(f, "database error: {e}"),
            Self::Plan(e) => write!(f, "plan rejected: {e}"),
            Self::Invalid(s) => write!(f, "unsafe database configuration: {s}"),
            Self::NotFound => write!(f, "plan has not been created"),
            Self::AlreadyExists => write!(f, "a plan already exists"),
            Self::Workflow(reason) => write!(f, "workflow rejected: {reason}"),
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
            1 | 2 => {}
            _ => return Err(DbError::Invalid("unknown schema version")),
        }
        if current < 2 {
            con.execute_batch(
                "BEGIN IMMEDIATE;
                CREATE TABLE IF NOT EXISTS workflow (
                    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                    payload TEXT NOT NULL
                );
                PRAGMA user_version=2;
                COMMIT;",
            )?;
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
        // Any plan edit invalidates the prior runtime state and gate receipts.
        tx.execute("DELETE FROM workflow WHERE singleton=1", [])?;
        if *plan.review_state() == crate::planning::ReviewState::Submitted {
            let wf = Workflow::new(&plan).map_err(DbError::Workflow)?;
            tx.execute(
                "INSERT INTO workflow(singleton,payload) VALUES(1,?1)",
                params![serde_json::to_string(&wf)
                    .map_err(|_| DbError::Invalid("workflow serialization"))?],
            )?;
        }
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

    fn workflow_unlocked(con: &Connection, plan: &Plan) -> Result<Workflow, DbError> {
        let serialized: Option<String> = con
            .query_row(
                "SELECT payload FROM workflow WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let serialized = serialized.ok_or(DbError::Workflow(
            "plan has not been submitted for review".into(),
        ))?;
        if serialized.len() > 1_048_576 {
            return Err(DbError::Workflow("workflow too large".into()));
        }
        let wf: Workflow = serde_json::from_str(&serialized)
            .map_err(|_| DbError::Workflow("invalid stored workflow".into()))?;
        wf.validate_against(plan).map_err(DbError::Workflow)?;
        Ok(wf)
    }
    pub fn workflow(&self) -> Result<Workflow, DbError> {
        let con = self.connect()?;
        let plan = Self::existing(&con)?.ok_or(DbError::NotFound)?;
        Self::workflow_unlocked(&con, &plan)
    }
    fn mutate_workflow<T>(
        &self,
        expected: u64,
        transform: impl FnOnce(&Plan, &mut Workflow) -> Result<T, String>,
    ) -> Result<(Workflow, T), DbError> {
        let mut con = self.connect()?;
        let tx = con.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let plan = Self::existing(&tx)?.ok_or(DbError::NotFound)?;
        let mut wf = Self::workflow_unlocked(&tx, &plan)?;
        if wf.revision != expected {
            return Err(DbError::Workflow(format!(
                "workflow revision conflict: expected {expected}, actual {}",
                wf.revision
            )));
        }
        let output = transform(&plan, &mut wf).map_err(DbError::Workflow)?;
        let data =
            serde_json::to_string(&wf).map_err(|_| DbError::Invalid("workflow serialization"))?;
        if data.len() > 1_048_576 {
            return Err(DbError::Workflow("workflow too large".into()));
        }
        let changes = tx.execute(
            "UPDATE workflow SET payload=?1 WHERE singleton=1",
            params![data],
        )?;
        if changes != 1 {
            return Err(DbError::Invalid("workflow disappeared during mutation"));
        }
        tx.commit()?;
        Ok((wf, output))
    }
    pub fn define_gate(&self, expected: u64, gate: TestGate) -> Result<Workflow, DbError> {
        self.mutate_workflow(expected, |plan, flow| {
            // Definitions must be anchored to an actual required gate reference.
            if !plan
                .work_items()
                .iter()
                .any(|item| item.required_gate_ids.contains(&gate.test_id))
            {
                return Err("test ID must be referenced by at least one work item".into());
            }
            flow.define_gate(gate)
        })
        .map(|(state, _)| state)
    }
    pub fn claim_next(
        &self,
        expected: u64,
        agent: &str,
    ) -> Result<(Workflow, Option<Assignment>), DbError> {
        self.mutate_workflow(expected, |plan, flow| flow.next(plan, agent, Utc::now()))
    }
    pub fn submit_work(
        &self,
        expected: u64,
        id: Uuid,
        lease: Uuid,
        notes: String,
    ) -> Result<Workflow, DbError> {
        self.mutate_workflow(expected, |_, flow| flow.report(id, lease, notes))
            .map(|(s, _)| s)
    }
    pub fn fail_work(&self, expected: u64, id: Uuid, reason: String) -> Result<Workflow, DbError> {
        self.mutate_workflow(expected, |_, flow| flow.fail(id, reason))
            .map(|(s, _)| s)
    }
    pub fn refresh_verifier_source(
        &self,
        expected: u64,
        tree: Option<String>,
    ) -> Result<Workflow, DbError> {
        self.mutate_workflow(expected, |plan, flow| {
            flow.refresh_source(plan, tree)?;
            Ok(())
        })
        .map(|(flow, _)| flow)
    }
    pub fn commit_verification(
        &self,
        expected: u64,
        id: Uuid,
        tree_sha: String,
        observations: Vec<GateObservation>,
    ) -> Result<Workflow, DbError> {
        self.mutate_workflow(expected, |plan, flow| {
            flow.observe(plan, id, tree_sha, observations)
        })
        .map(|(state, _)| state)
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
    fn existing_schema_one_database_migrates_workflow_without_losing_plan() {
        let (root, db) = fixture();
        let created = db.create("Surviving plan".into(), "".into()).unwrap();
        {
            let conn = Connection::open(root.path().join("plans.sqlite")).unwrap();
            conn.execute_batch("DROP TABLE workflow; PRAGMA user_version=1;")
                .unwrap();
        }
        let reopened = Database::open(&root.path().join("plans.sqlite")).unwrap();
        assert_eq!(reopened.get().unwrap(), created);
        let version: i64 = Connection::open(root.path().join("plans.sqlite"))
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
        assert!(reopened.workflow().is_err()); // plan has not yet been submitted
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
