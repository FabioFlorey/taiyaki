//! Declarative plan model. Metadata here is not authenticated identity or proof.
//! The execution authority, receipts, and phase transitions are separate concerns.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use uuid::Uuid;

pub const PLAN_SCHEMA_VERSION: u32 = 1;

/// Human-supplied descriptors and timestamps, not verified audit provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub owner: Option<String>,
    #[serde(default)]
    pub labels: BTreeSet<String>,
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,
}

impl Metadata {
    pub fn new(at: DateTime<Utc>) -> Self {
        Self {
            created_at: at,
            updated_at: at,
            owner: None,
            labels: BTreeSet::new(),
            annotations: BTreeMap::new(),
        }
    }

    fn validate(&self) -> Result<(), PlanError> {
        if self.updated_at < self.created_at {
            return Err(PlanError::Invalid("updated_at precedes created_at".into()));
        }
        if self.owner.as_deref().is_some_and(|v| v.trim().is_empty())
            || self.labels.iter().any(|v| v.trim().is_empty())
            || self.annotations.keys().any(|v| v.trim().is_empty())
        {
            return Err(PlanError::Invalid(
                "empty owner, label, or annotation key".into(),
            ));
        }
        Ok(())
    }
}

/// One planned unit of work. Parent IDs form a hierarchy; dependencies express ordering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItem {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    /// Criteria are drafts until separately approved and enforced by the authority.
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    /// References to configured gate IDs, NOT shell commands or PASS evidence.
    #[serde(default)]
    pub required_gate_ids: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<Uuid>,
    pub metadata: Metadata,
}

impl WorkItem {
    pub fn new(
        title: impl Into<String>,
        description: impl Into<String>,
        at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            parent_id: None,
            title: title.into(),
            description: description.into(),
            acceptance_criteria: Vec::new(),
            required_gate_ids: Vec::new(),
            depends_on: Vec::new(),
            metadata: Metadata::new(at),
        }
    }

    fn validate(&self) -> Result<(), PlanError> {
        if self.id.is_nil() || self.title.trim().is_empty() {
            return Err(PlanError::Invalid("work item needs an ID and title".into()));
        }
        self.metadata.validate()?;
        if self
            .acceptance_criteria
            .iter()
            .chain(self.required_gate_ids.iter())
            .any(|s| s.trim().is_empty())
        {
            return Err(PlanError::Invalid("empty criterion or gate ID".into()));
        }
        if self.required_gate_ids.iter().collect::<HashSet<_>>().len()
            != self.required_gate_ids.len()
            || self.depends_on.iter().collect::<HashSet<_>>().len() != self.depends_on.len()
        {
            return Err(PlanError::Invalid("duplicate gate or dependency".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema_version: u32,
    id: Uuid,
    revision: u64,
    title: String,
    description: String,
    metadata: Metadata,
    work_items: Vec<WorkItem>,
}

/// Every supported change is validated and committed with exactly one revision increment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanChange {
    Add(WorkItem),
    Replace(WorkItem),
    Remove(Uuid),
    SetDetails {
        title: String,
        description: String,
    },
    SetMetadata {
        owner: Option<String>,
        labels: BTreeSet<String>,
        annotations: BTreeMap<String, String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    Invalid(String),
    UnknownItem(Uuid),
    RevisionConflict { expected: u64, actual: u64 },
    RevisionOverflow,
    TimeRegression,
    Json(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PlanError {}

impl Plan {
    pub fn new(
        title: impl Into<String>,
        description: impl Into<String>,
        at: DateTime<Utc>,
    ) -> Result<Self, PlanError> {
        let plan = Self {
            schema_version: PLAN_SCHEMA_VERSION,
            id: Uuid::new_v4(),
            revision: 1,
            title: title.into(),
            description: description.into(),
            metadata: Metadata::new(at),
            work_items: Vec::new(),
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub fn work_items(&self) -> &[WorkItem] {
        &self.work_items
    }

    pub fn get_item(&self, id: Uuid) -> Option<&WorkItem> {
        self.work_items.iter().find(|item| item.id == id)
    }

    pub fn apply(
        &mut self,
        expected_revision: u64,
        at: DateTime<Utc>,
        change: PlanChange,
    ) -> Result<(), PlanError> {
        if expected_revision != self.revision {
            return Err(PlanError::RevisionConflict {
                expected: expected_revision,
                actual: self.revision,
            });
        }
        if at < self.metadata.updated_at {
            return Err(PlanError::TimeRegression);
        }

        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(PlanError::RevisionOverflow)?;
        // Changes happen on a copy so invalid edits cannot leave a half-updated plan.
        let mut next = self.clone();
        match change {
            PlanChange::Add(mut item) => {
                // Caller-provided timestamps are not used for new items.
                item.metadata.created_at = at;
                item.metadata.updated_at = at;
                next.work_items.push(item);
            }
            PlanChange::Replace(mut item) => {
                let old = next
                    .work_items
                    .iter_mut()
                    .find(|old| old.id == item.id)
                    .ok_or(PlanError::UnknownItem(item.id))?;
                // Preserve original creation time when a draft item is replaced.
                item.metadata.created_at = old.metadata.created_at;
                item.metadata.updated_at = at;
                *old = item;
            }
            PlanChange::Remove(id) => {
                let before = next.work_items.len();
                next.work_items.retain(|item| item.id != id);
                if next.work_items.len() == before {
                    return Err(PlanError::UnknownItem(id));
                }
            }
            PlanChange::SetDetails { title, description } => {
                next.title = title;
                next.description = description;
            }
            PlanChange::SetMetadata {
                owner,
                labels,
                annotations,
            } => {
                next.metadata.owner = owner;
                next.metadata.labels = labels;
                next.metadata.annotations = annotations;
            }
        }
        next.revision = next_revision;
        next.metadata.updated_at = at;
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Reads an untrusted JSON plan, validating relationships and metadata.
    pub fn from_json(input: &str) -> Result<Self, PlanError> {
        let plan: Self =
            serde_json::from_str(input).map_err(|err| PlanError::Json(err.to_string()))?;
        plan.validate()?;
        Ok(plan)
    }

    pub fn to_json_pretty(&self) -> Result<String, PlanError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|err| PlanError::Json(err.to_string()))
    }

    pub fn validate(&self) -> Result<(), PlanError> {
        if self.schema_version != PLAN_SCHEMA_VERSION
            || self.id.is_nil()
            || self.revision == 0
            || self.title.trim().is_empty()
        {
            return Err(PlanError::Invalid("invalid plan header".into()));
        }
        self.metadata.validate()?;

        let mut ids = HashSet::new();
        for item in &self.work_items {
            item.validate()?;
            if item.id == self.id || !ids.insert(item.id) {
                return Err(PlanError::Invalid(
                    "duplicate or reserved work item ID".into(),
                ));
            }
            if item.metadata.created_at < self.metadata.created_at
                || item.metadata.updated_at > self.metadata.updated_at
            {
                return Err(PlanError::Invalid(
                    "work item timestamps outside plan interval".into(),
                ));
            }
        }

        let index: HashMap<Uuid, &WorkItem> =
            self.work_items.iter().map(|item| (item.id, item)).collect();
        for item in &self.work_items {
            if let Some(parent) = item.parent_id {
                if parent == item.id || !index.contains_key(&parent) {
                    return Err(PlanError::Invalid("self or missing parent".into()));
                }
            }
            for dependency in &item.depends_on {
                if *dependency == item.id || !index.contains_key(dependency) {
                    return Err(PlanError::Invalid("self or missing dependency".into()));
                }
            }
        }

        // Parent cycles and dependency cycles are different kinds of invalid graph.
        for item in &self.work_items {
            let mut visited = HashSet::new();
            let mut current = Some(item.id);
            while let Some(id) = current {
                if !visited.insert(id) {
                    return Err(PlanError::Invalid("parent cycle".into()));
                }
                current = index.get(&id).and_then(|node| node.parent_id);
            }
        }

        fn dependencies_acyclic(
            id: Uuid,
            index: &HashMap<Uuid, &WorkItem>,
            visiting: &mut HashSet<Uuid>,
            complete: &mut HashSet<Uuid>,
        ) -> bool {
            if complete.contains(&id) {
                return true;
            }
            if !visiting.insert(id) {
                return false;
            }
            for dependency in &index[&id].depends_on {
                if !dependencies_acyclic(*dependency, index, visiting, complete) {
                    return false;
                }
            }
            visiting.remove(&id);
            complete.insert(id);
            true
        }

        let mut complete = HashSet::new();
        for item in &self.work_items {
            if !dependencies_acyclic(item.id, &index, &mut HashSet::new(), &mut complete) {
                return Err(PlanError::Invalid("dependency cycle".into()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 12, 10, 0, 0).unwrap()
    }

    fn later() -> DateTime<Utc> {
        now() + chrono::Duration::minutes(3)
    }

    fn plan() -> Plan {
        Plan::new("Build server", "First useful milestone", now()).unwrap()
    }

    #[test]
    fn creates_metadata_and_stable_identity() {
        let p = plan();
        assert_ne!(p.id(), Uuid::nil());
        assert_eq!(p.revision(), 1);
        assert_eq!(p.metadata().created_at, now());
        assert_eq!(p.metadata().updated_at, now());
        assert_eq!(p.title(), "Build server");
        assert!(p.work_items().is_empty());
    }

    #[test]
    fn adds_item_and_increments_plan_revision() {
        let mut p = plan();
        let mut item = WorkItem::new("Plan tests", "Document acceptance", now());
        item.acceptance_criteria.push("Tests are reviewable".into());
        item.required_gate_ids.push("cargo-test".into());
        item.metadata.labels.insert("priority:high".into());
        let id = item.id;
        p.apply(1, later(), PlanChange::Add(item)).unwrap();
        assert_eq!(p.revision(), 2);
        assert_eq!(p.metadata().updated_at, later());
        assert_eq!(p.get_item(id).unwrap().metadata.created_at, later());
        assert_eq!(p.get_item(id).unwrap().required_gate_ids, ["cargo-test"]);
    }

    #[test]
    fn metadata_can_be_updated_and_json_round_trips() {
        let mut p = plan();
        let mut labels = BTreeSet::new();
        labels.insert("security".into());
        labels.insert("rust".into());
        let mut annotations = BTreeMap::new();
        annotations.insert("ticket".into(), "TAI-1".into());
        p.apply(
            1,
            later(),
            PlanChange::SetMetadata {
                owner: Some("engineering".into()),
                labels,
                annotations,
            },
        )
        .unwrap();
        let json = p.to_json_pretty().unwrap();
        let parsed = Plan::from_json(&json).unwrap();
        assert_eq!(p, parsed);
        assert_eq!(parsed.metadata().owner.as_deref(), Some("engineering"));
        assert_eq!(parsed.metadata().annotations["ticket"], "TAI-1");
        assert!(json.contains(r#""schema_version": 1"#));
    }

    #[test]
    fn stale_revision_cannot_overwrite_newer_work() {
        let mut p = plan();
        p.apply(
            1,
            later(),
            PlanChange::SetDetails {
                title: "Revised".into(),
                description: "changed".into(),
            },
        )
        .unwrap();
        let before = p.clone();
        let err = p.apply(1, later(), PlanChange::Add(WorkItem::new("X", "", now())));
        assert_eq!(
            err,
            Err(PlanError::RevisionConflict {
                expected: 1,
                actual: 2,
            })
        );
        assert_eq!(p, before);
    }

    #[test]
    fn invalid_metadata_and_time_regression_are_atomic() {
        let mut p = plan();
        let before = p.clone();
        let mut labels = BTreeSet::new();
        labels.insert(" ".to_string());
        let err = p.apply(
            1,
            later(),
            PlanChange::SetMetadata {
                owner: None,
                labels,
                annotations: BTreeMap::new(),
            },
        );
        assert!(matches!(err, Err(PlanError::Invalid(_))));
        assert_eq!(p, before);
        assert_eq!(
            p.apply(
                1,
                now() - chrono::Duration::seconds(1),
                PlanChange::Remove(Uuid::new_v4())
            ),
            Err(PlanError::TimeRegression)
        );
        assert_eq!(p, before);
    }

    #[test]
    fn detects_parent_and_dependency_cycles() {
        let mut p = plan();
        let mut a = WorkItem::new("A", "", now());
        let mut b = WorkItem::new("B", "", now());
        b.parent_id = Some(a.id);
        a.parent_id = Some(b.id);
        p.apply(1, now(), PlanChange::Add(a.clone())).unwrap_err();
        // Both references must exist at commit time, so start with valid items.
        a.parent_id = None;
        b.parent_id = None;
        p.apply(1, now(), PlanChange::Add(a.clone())).unwrap();
        p.apply(2, now(), PlanChange::Add(b.clone())).unwrap();

        a.parent_id = Some(b.id);
        p.apply(3, now(), PlanChange::Replace(a.clone())).unwrap();
        b.parent_id = Some(a.id);
        let before = p.clone();
        assert!(matches!(
            p.apply(4, now(), PlanChange::Replace(b.clone())),
            Err(PlanError::Invalid(message)) if message == "parent cycle"
        ));
        assert_eq!(p, before);

        a.parent_id = None;
        a.depends_on.push(b.id);
        p.apply(4, now(), PlanChange::Replace(a)).unwrap();
        b.depends_on.push(p.work_items()[0].id);
        let before = p.clone();
        assert!(matches!(
            p.apply(5, now(), PlanChange::Replace(b)),
            Err(PlanError::Invalid(message)) if message == "dependency cycle"
        ));
        assert_eq!(p, before);
    }

    #[test]
    fn rejects_missing_links_and_deleting_referenced_items() {
        let mut p = plan();
        let mut a = WorkItem::new("A", "", now());
        a.depends_on.push(Uuid::new_v4());
        assert!(matches!(
            p.apply(1, now(), PlanChange::Add(a.clone())),
            Err(PlanError::Invalid(_))
        ));
        assert_eq!(p.revision(), 1);
        a.depends_on.clear();
        p.apply(1, now(), PlanChange::Add(a.clone())).unwrap();
        let mut b = WorkItem::new("B", "", now());
        b.parent_id = Some(a.id);
        p.apply(2, now(), PlanChange::Add(b)).unwrap();
        let before = p.clone();
        assert!(matches!(
            p.apply(3, now(), PlanChange::Remove(a.id)),
            Err(PlanError::Invalid(_))
        ));
        assert_eq!(p, before);
    }

    #[test]
    fn replace_preserves_creation_time_and_checks_freshness() {
        let mut p = plan();
        let item = WorkItem::new("First", "", now());
        let id = item.id;
        p.apply(1, now(), PlanChange::Add(item)).unwrap();
        let mut changed = p.get_item(id).unwrap().clone();
        changed.title = "Second".into();
        changed.metadata.created_at = later(); // never trust this replacement field.
        p.apply(2, later(), PlanChange::Replace(changed)).unwrap();
        assert_eq!(p.get_item(id).unwrap().metadata.created_at, now());
        assert_eq!(p.get_item(id).unwrap().metadata.updated_at, later());
        assert_eq!(p.revision(), 3);
    }

    #[test]
    fn invalid_json_is_rejected_instead_of_normalized_silently() {
        let p = plan();
        let encoded = p.to_json_pretty().unwrap();
        assert!(matches!(
            Plan::from_json(&encoded.replace(r#""schema_version": 1"#, r#""schema_version": 42"#)),
            Err(PlanError::Invalid(_))
        ));
        assert!(matches!(Plan::from_json("{}"), Err(PlanError::Json(_))));
    }

    #[test]
    fn gate_references_are_identifiers_not_executable_verification() {
        let mut p = plan();
        let mut item = WorkItem::new("Checks", "", now());
        item.required_gate_ids.push("cargo-test".into());
        item.required_gate_ids.push("cargo-test".into());
        let original = p.clone();
        assert!(matches!(
            p.apply(1, now(), PlanChange::Add(item)),
            Err(PlanError::Invalid(_))
        ));
        assert_eq!(p, original);
    }
}
