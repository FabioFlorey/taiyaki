//! Declarative plan model. Metadata here is not authenticated identity or proof.
//! The execution authority, receipts, and phase transitions are separate concerns.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use uuid::Uuid;

pub const PLAN_SCHEMA_VERSION: u32 = 2;
const MAX_PLAN_BYTES: usize = 1_048_576;
const MAX_ITEMS: usize = 4096;
const MAX_TEXT_BYTES: usize = 8192;
const MAX_ENTRY_BYTES: usize = 256;
const MAX_COLLECTION_ENTRIES: usize = 128;

fn text_ok(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.len() <= max
}

/// Planning-only priority. Not a runtime scheduler instruction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

/// What kind of planning node this represents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemKind {
    #[default]
    Task,
    Milestone,
}

/// Planning readiness; not a verified execution phase.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    #[default]
    Proposed,
    Ready,
    Blocked,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub not_before: Option<DateTime<Utc>>,
    pub due_at: Option<DateTime<Utc>>,
    /// Estimate only; never a measured time or verification result.
    pub estimate_minutes: Option<u32>,
}

impl Schedule {
    fn validate(&self) -> Result<(), PlanError> {
        if let (Some(start), Some(end)) = (self.not_before, self.due_at) {
            if end < start {
                return Err(PlanError::Invalid(
                    "due date precedes earliest start".into(),
                ));
            }
        }
        if self.estimate_minutes == Some(0) {
            return Err(PlanError::Invalid(
                "estimated effort must be positive".into(),
            ));
        }
        Ok(())
    }
}

/// Submission only means a draft is ready for review. This is never server approval.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    #[default]
    Draft,
    Submitted,
}

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
        if self
            .owner
            .as_deref()
            .is_some_and(|v| !text_ok(v, MAX_ENTRY_BYTES))
            || self.labels.len() > MAX_COLLECTION_ENTRIES
            || self.annotations.len() > MAX_COLLECTION_ENTRIES
            || self.labels.iter().any(|v| !text_ok(v, MAX_ENTRY_BYTES))
            || self
                .annotations
                .iter()
                .any(|(key, value)| !text_ok(key, MAX_ENTRY_BYTES) || value.len() > MAX_TEXT_BYTES)
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
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub kind: WorkItemKind,
    #[serde(default)]
    pub disposition: Disposition,
    #[serde(default)]
    pub blocker: Option<String>,
    #[serde(default)]
    pub schedule: Schedule,
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
            priority: Priority::Normal,
            kind: WorkItemKind::Task,
            disposition: Disposition::Proposed,
            blocker: None,
            schedule: Schedule::default(),
            metadata: Metadata::new(at),
        }
    }

    fn validate(&self) -> Result<(), PlanError> {
        if self.id.is_nil()
            || !text_ok(&self.title, MAX_ENTRY_BYTES)
            || self.description.len() > MAX_TEXT_BYTES
        {
            return Err(PlanError::Invalid(
                "work item needs a valid ID and title".into(),
            ));
        }
        self.metadata.validate()?;
        self.schedule.validate()?;
        if self.disposition == Disposition::Blocked
            && !self
                .blocker
                .as_deref()
                .is_some_and(|value| text_ok(value, MAX_TEXT_BYTES))
        {
            return Err(PlanError::Invalid("blocked work requires a reason".into()));
        }
        if self.disposition != Disposition::Blocked && self.blocker.is_some() {
            return Err(PlanError::Invalid(
                "blocker only allowed on blocked work".into(),
            ));
        }
        if self.acceptance_criteria.len() > MAX_COLLECTION_ENTRIES
            || self.required_gate_ids.len() > MAX_COLLECTION_ENTRIES
            || self.depends_on.len() > MAX_COLLECTION_ENTRIES
        {
            return Err(PlanError::Invalid("too many work item entries".into()));
        }
        if self
            .acceptance_criteria
            .iter()
            .chain(self.required_gate_ids.iter())
            .any(|s| !text_ok(s, MAX_TEXT_BYTES))
            || self
                .required_gate_ids
                .iter()
                .any(|s| s.len() > MAX_ENTRY_BYTES)
        {
            return Err(PlanError::Invalid("invalid criterion or gate ID".into()));
        }
        if self
            .acceptance_criteria
            .iter()
            .collect::<HashSet<_>>()
            .len()
            != self.acceptance_criteria.len()
        {
            return Err(PlanError::Invalid("duplicate acceptance criterion".into()));
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
    #[serde(default)]
    review_state: ReviewState,
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
    SubmitForReview,
    ReturnToDraft,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    Invalid(String),
    UnknownItem(Uuid),
    RevisionConflict { expected: u64, actual: u64 },
    RevisionOverflow,
    TimeRegression,
    ReviewLocked,
    NotReady(String),
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
            review_state: ReviewState::Draft,
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

    pub fn review_state(&self) -> &ReviewState {
        &self.review_state
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
        self.apply_batch(expected_revision, at, vec![change])
    }

    /// Apply related edits under one revision. References are checked against
    /// the final graph, so a parent and its children can be inserted together.
    pub fn apply_batch(
        &mut self,
        expected_revision: u64,
        at: DateTime<Utc>,
        changes: Vec<PlanChange>,
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

        if changes.is_empty() || changes.len() > MAX_COLLECTION_ENTRIES {
            return Err(PlanError::Invalid("invalid batch size".into()));
        }
        if self.review_state == ReviewState::Submitted {
            if changes.len() != 1 || !matches!(changes[0], PlanChange::ReturnToDraft) {
                return Err(PlanError::ReviewLocked);
            }
        } else if changes
            .iter()
            .any(|change| matches!(change, PlanChange::ReturnToDraft))
        {
            return Err(PlanError::Invalid("plan is already a draft".into()));
        }
        if let Some(index) = changes
            .iter()
            .position(|change| matches!(change, PlanChange::SubmitForReview))
        {
            if index != changes.len() - 1 {
                return Err(PlanError::Invalid("review submission must be last".into()));
            }
        }

        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(PlanError::RevisionOverflow)?;
        // Changes happen on a copy so invalid edits cannot leave a half-updated plan.
        let mut next = self.clone();
        for change in changes {
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
                PlanChange::SubmitForReview => {
                    next.check_review_readiness()?;
                    next.review_state = ReviewState::Submitted;
                }
                PlanChange::ReturnToDraft => {
                    next.review_state = ReviewState::Draft;
                }
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
        if input.len() > MAX_PLAN_BYTES {
            return Err(PlanError::Invalid("plan JSON is too large".into()));
        }
        // Upgrade old v1 documents without changing identity or revision. Unknown
        // fields are still rejected by the versioned model's strict deserializer.
        let mut value: serde_json::Value =
            serde_json::from_str(input).map_err(|err| PlanError::Json(err.to_string()))?;
        if value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            == Some(1)
        {
            value["schema_version"] = serde_json::json!(PLAN_SCHEMA_VERSION);
        }
        let plan: Self =
            serde_json::from_value(value).map_err(|err| PlanError::Json(err.to_string()))?;
        plan.validate()?;
        Ok(plan)
    }

    pub fn to_json_pretty(&self) -> Result<String, PlanError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|err| PlanError::Json(err.to_string()))
    }

    fn check_review_readiness(&self) -> Result<(), PlanError> {
        if self.work_items.is_empty() {
            return Err(PlanError::NotReady("plan has no work items".into()));
        }
        let parents: HashSet<Uuid> = self
            .work_items
            .iter()
            .filter_map(|item| item.parent_id)
            .collect();
        for item in &self.work_items {
            if item.disposition != Disposition::Ready {
                return Err(PlanError::NotReady(format!(
                    "item {} is not ready for review",
                    item.id
                )));
            }
            if !parents.contains(&item.id)
                && (item.acceptance_criteria.is_empty() || item.required_gate_ids.is_empty())
            {
                return Err(PlanError::NotReady(format!(
                    "leaf {} needs acceptance criteria and gate references",
                    item.id
                )));
            }
        }
        Ok(())
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
        if self.description.len() > MAX_TEXT_BYTES || self.title.len() > MAX_ENTRY_BYTES {
            return Err(PlanError::Invalid("invalid plan text size".into()));
        }
        if self.work_items.len() > MAX_ITEMS {
            return Err(PlanError::Invalid("too many work items".into()));
        }
        if self.review_state == ReviewState::Submitted {
            self.check_review_readiness()?;
        }

        let mut ids = HashSet::new();
        let mut text_budget = self.title.len() + self.description.len();
        let metadata_size = |m: &Metadata| {
            m.owner.as_ref().map_or(0, String::len)
                + m.labels.iter().map(String::len).sum::<usize>()
                + m.annotations
                    .iter()
                    .map(|(k, v)| k.len() + v.len())
                    .sum::<usize>()
        };
        text_budget += metadata_size(&self.metadata);
        for item in &self.work_items {
            item.validate()?;
            text_budget = text_budget.saturating_add(
                item.title.len()
                    + item.description.len()
                    + item
                        .acceptance_criteria
                        .iter()
                        .map(String::len)
                        .sum::<usize>()
                    + item
                        .required_gate_ids
                        .iter()
                        .map(String::len)
                        .sum::<usize>()
                    + item.blocker.as_ref().map_or(0, String::len)
                    + metadata_size(&item.metadata),
            );
            if text_budget > MAX_PLAN_BYTES {
                return Err(PlanError::Invalid("plan content exceeds size limit".into()));
            }
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

        // Iterative cycle detection prevents stack overflows from untrusted graphs.
        fn acyclic(
            index: &HashMap<Uuid, &WorkItem>,
            links: impl Fn(&WorkItem) -> Vec<Uuid>,
        ) -> bool {
            let mut complete = HashSet::new();
            for &root in index.keys() {
                if complete.contains(&root) {
                    continue;
                }
                let mut visiting = HashSet::new();
                let mut stack = vec![(root, false)];
                while let Some((id, exiting)) = stack.pop() {
                    if exiting {
                        visiting.remove(&id);
                        complete.insert(id);
                        continue;
                    }
                    if complete.contains(&id) {
                        continue;
                    }
                    if !visiting.insert(id) {
                        return false;
                    }
                    stack.push((id, true));
                    for next in links(index[&id]) {
                        if visiting.contains(&next) {
                            return false;
                        }
                        if !complete.contains(&next) {
                            stack.push((next, false));
                        }
                    }
                }
            }
            true
        }
        if !acyclic(&index, |item| item.parent_id.into_iter().collect()) {
            return Err(PlanError::Invalid("parent cycle".into()));
        }
        if !acyclic(&index, |item| item.depends_on.clone()) {
            return Err(PlanError::Invalid("dependency cycle".into()));
        }
        // Reject in-memory graphs which cannot be persisted using the same
        // size limit as untrusted JSON imports.
        let serialized =
            serde_json::to_vec_pretty(self).map_err(|err| PlanError::Json(err.to_string()))?;
        if serialized.len() > MAX_PLAN_BYTES {
            return Err(PlanError::Invalid("plan exceeds JSON limit".into()));
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
        assert!(json.contains(r#""schema_version": 2"#));
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
            Plan::from_json(&encoded.replace(r#""schema_version": 2"#, r#""schema_version": 42"#)),
            Err(PlanError::Invalid(_))
        ));
        assert!(matches!(Plan::from_json("{}"), Err(PlanError::Json(_))));
    }

    #[test]
    fn typed_metadata_and_schedule_survive_json() {
        let mut p = plan();
        let mut item = WorkItem::new("Design gate", "Permission boundary", now());
        item.priority = Priority::Critical;
        item.kind = WorkItemKind::Milestone;
        item.disposition = Disposition::Ready;
        item.schedule.not_before = Some(now());
        item.schedule.due_at = Some(later());
        item.schedule.estimate_minutes = Some(45);
        item.metadata.owner = Some("backend".into());
        p.apply(1, later(), PlanChange::Add(item)).unwrap();

        let json = p.to_json_pretty().unwrap();
        let loaded = Plan::from_json(&json).unwrap();
        let node = &loaded.work_items()[0];
        assert_eq!(node.priority, Priority::Critical);
        assert_eq!(node.kind, WorkItemKind::Milestone);
        assert_eq!(node.disposition, Disposition::Ready);
        assert_eq!(node.schedule.estimate_minutes, Some(45));
        assert_eq!(node.metadata.owner.as_deref(), Some("backend"));
    }

    #[test]
    fn v1_json_upgrades_with_original_identity_and_revision() {
        let p = plan();
        let json = p.to_json_pretty().unwrap();
        let v1 = json
            .replace(r#""schema_version": 2"#, r#""schema_version": 1"#)
            .replace("  \"review_state\": \"draft\",\n", "");
        let loaded = Plan::from_json(&v1).unwrap();
        assert_eq!(loaded.id(), p.id());
        assert_eq!(loaded.revision(), p.revision());
        assert_eq!(loaded.review_state(), &ReviewState::Draft);
        assert!(loaded
            .to_json_pretty()
            .unwrap()
            .contains(r#""schema_version": 2"#));

        let item = WorkItem::new("legacy", "", now());
        let mut populated = p.clone();
        populated.apply(1, now(), PlanChange::Add(item)).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&populated.to_json_pretty().unwrap()).unwrap();
        value["schema_version"] = serde_json::json!(1);
        value.as_object_mut().unwrap().remove("review_state");
        for item in value["work_items"].as_array_mut().unwrap() {
            let node = item.as_object_mut().unwrap();
            for key in ["priority", "kind", "disposition", "blocker", "schedule"] {
                node.remove(key);
            }
        }
        let recovered = Plan::from_json(&value.to_string()).unwrap();
        assert_eq!(recovered.work_items()[0].priority, Priority::Normal);
        assert_eq!(recovered.work_items()[0].disposition, Disposition::Proposed);
        assert_eq!(recovered.revision(), 2);
    }

    fn reviewable_plan() -> Plan {
        let mut p = plan();
        let mut item = WorkItem::new("Implementation", "Independent proof", now());
        item.disposition = Disposition::Ready;
        item.acceptance_criteria.push("A real test passes".into());
        item.required_gate_ids.push("cargo-test".into());
        p.apply(1, now(), PlanChange::Add(item)).unwrap();
        p
    }

    #[test]
    fn submitting_requires_evaluable_leaf_work() {
        let mut empty = plan();
        let before = empty.clone();
        assert!(matches!(
            empty.apply(1, later(), PlanChange::SubmitForReview),
            Err(PlanError::NotReady(_))
        ));
        assert_eq!(empty, before);

        let mut missing = plan();
        let mut item = WorkItem::new("Incomplete", "", now());
        item.disposition = Disposition::Ready;
        missing.apply(1, now(), PlanChange::Add(item)).unwrap();
        let before = missing.clone();
        assert!(matches!(
            missing.apply(2, later(), PlanChange::SubmitForReview),
            Err(PlanError::NotReady(_))
        ));
        assert_eq!(missing, before);
    }

    #[test]
    fn submit_freezes_edits_and_reopening_changes_revision() {
        let mut p = reviewable_plan();
        p.apply(2, later(), PlanChange::SubmitForReview).unwrap();
        assert_eq!(p.revision(), 3);
        assert_eq!(p.review_state(), &ReviewState::Submitted);
        let serialized = p.to_json_pretty().unwrap();
        assert_eq!(Plan::from_json(&serialized).unwrap(), p);
        let before = p.clone();
        assert_eq!(
            p.apply(
                3,
                later(),
                PlanChange::SetDetails {
                    title: "Tamper".into(),
                    description: String::new()
                }
            ),
            Err(PlanError::ReviewLocked)
        );
        assert_eq!(p, before);
        p.apply(3, later(), PlanChange::ReturnToDraft).unwrap();
        assert_eq!(p.review_state(), &ReviewState::Draft);
        assert_eq!(p.revision(), 4);
        p.apply(
            4,
            later(),
            PlanChange::SetDetails {
                title: "New scope".into(),
                description: String::new(),
            },
        )
        .unwrap();
        assert_eq!(p.title(), "New scope");
        assert_eq!(p.revision(), 5);
    }

    #[test]
    fn blocked_work_requires_reason_and_cannot_be_submitted() {
        let mut p = plan();
        let mut blocked = WorkItem::new("Review dependency", "", now());
        blocked.disposition = Disposition::Blocked;
        let unchanged = p.clone();
        assert!(matches!(
            p.apply(1, now(), PlanChange::Add(blocked.clone())),
            Err(PlanError::Invalid(_))
        ));
        assert_eq!(p, unchanged);
        blocked.blocker = Some("Awaiting API contract".into());
        p.apply(1, now(), PlanChange::Add(blocked)).unwrap();
        assert!(matches!(
            p.apply(2, later(), PlanChange::SubmitForReview),
            Err(PlanError::NotReady(_))
        ));
    }

    #[test]
    fn invalid_schedule_is_atomic() {
        let mut p = plan();
        let mut item = WorkItem::new("Timeline", "", now());
        item.schedule.not_before = Some(later());
        item.schedule.due_at = Some(now());
        let unchanged = p.clone();
        assert!(matches!(
            p.apply(1, now(), PlanChange::Add(item)),
            Err(PlanError::Invalid(_))
        ));
        assert_eq!(p, unchanged);
        let mut item = WorkItem::new("Estimate", "", now());
        item.schedule.estimate_minutes = Some(0);
        assert!(matches!(
            p.apply(1, now(), PlanChange::Add(item)),
            Err(PlanError::Invalid(_))
        ));
    }

    #[test]
    fn malformed_new_fields_and_unknown_json_are_rejected() {
        let mut p = plan();
        let item = WorkItem::new("Doc", "", now());
        p.apply(1, now(), PlanChange::Add(item)).unwrap();
        let json = p.to_json_pretty().unwrap();
        assert!(matches!(
            Plan::from_json(&json.replace("\"priority\": \"normal\"", "\"priority\": \"top\"")),
            Err(PlanError::Json(_))
        ));
        assert!(matches!(
            Plan::from_json(&json.replace(
                "\"review_state\": \"draft\"",
                "\"review_state\": \"approved\""
            )),
            Err(PlanError::Json(_))
        ));
        assert!(matches!(
            Plan::from_json(&json.replace("\"revision\": 2", "\"revision\": 0")),
            Err(PlanError::Invalid(_))
        ));
        assert!(matches!(
            Plan::from_json(&format!("{}{}", " ", "x".repeat(MAX_PLAN_BYTES + 1))),
            Err(PlanError::Invalid(_))
        ));
    }

    #[test]
    fn bounds_check_long_titles_and_metadata() {
        let mut p = plan();
        let before = p.clone();
        let item = WorkItem::new("X".repeat(MAX_ENTRY_BYTES + 1), "", now());
        assert!(matches!(
            p.apply(1, now(), PlanChange::Add(item)),
            Err(PlanError::Invalid(_))
        ));
        assert_eq!(p, before);
        let mut labels = BTreeSet::new();
        labels.insert("bad".repeat(MAX_ENTRY_BYTES));
        assert!(matches!(
            p.apply(
                1,
                later(),
                PlanChange::SetMetadata {
                    owner: None,
                    labels,
                    annotations: BTreeMap::new()
                }
            ),
            Err(PlanError::Invalid(_))
        ));
        assert_eq!(p, before);
    }

    #[test]
    fn iterative_dependency_validation_handles_long_chains() {
        let mut p = plan();
        let mut last = None;
        for id in 0..350 {
            let mut item = WorkItem::new(format!("Node {id}"), "", now());
            if let Some(previous) = last {
                item.depends_on.push(previous);
            }
            last = Some(item.id);
            p.apply(p.revision(), now(), PlanChange::Add(item)).unwrap();
        }
        assert_eq!(p.work_items().len(), 350);
        p.validate().unwrap();
    }

    #[test]
    fn atomic_batch_can_add_related_nodes_in_one_revision() {
        let mut p = plan();
        let parent = WorkItem::new("Parent", "", now());
        let mut child = WorkItem::new("Child", "", now());
        child.parent_id = Some(parent.id);
        child.depends_on.push(parent.id);
        // Forward reference is valid within a batch, not in an individual add.
        p.apply_batch(
            1,
            later(),
            vec![
                PlanChange::Add(child.clone()),
                PlanChange::Add(parent.clone()),
            ],
        )
        .unwrap();
        assert_eq!(p.revision(), 2);
        assert_eq!(p.work_items().len(), 2);
        assert_eq!(p.get_item(child.id).unwrap().parent_id, Some(parent.id));
        assert_eq!(p.get_item(child.id).unwrap().depends_on, [parent.id]);
        p.validate().unwrap();
    }

    #[test]
    fn invalid_batch_is_atomic_and_rejects_illegal_sequences() {
        let mut p = plan();
        let before = p.clone();
        let one = WorkItem::new("One", "", now());
        let mut two = WorkItem::new("Two", "", now());
        two.parent_id = Some(Uuid::new_v4());
        assert!(matches!(
            p.apply_batch(1, later(), vec![PlanChange::Add(one), PlanChange::Add(two)]),
            Err(PlanError::Invalid(_))
        ));
        assert_eq!(p, before);
        assert!(matches!(
            p.apply_batch(1, later(), Vec::new()),
            Err(PlanError::Invalid(_))
        ));
        assert!(matches!(
            p.apply_batch(
                1,
                later(),
                vec![
                    PlanChange::SubmitForReview,
                    PlanChange::SetDetails {
                        title: "Mutated".into(),
                        description: String::new()
                    }
                ]
            ),
            Err(PlanError::Invalid(_))
        ));
        assert_eq!(p, before);
    }

    #[test]
    fn batch_can_build_ready_plan_and_submit_it_once() {
        let mut p = plan();
        let mut item = WorkItem::new("Gate", "Checked boundary", now());
        item.disposition = Disposition::Ready;
        item.acceptance_criteria
            .push("Denied calls stay denied".into());
        item.required_gate_ids.push("phase-gate-test".into());
        p.apply_batch(
            1,
            later(),
            vec![PlanChange::Add(item), PlanChange::SubmitForReview],
        )
        .unwrap();
        assert_eq!(p.revision(), 2);
        assert_eq!(p.review_state(), &ReviewState::Submitted);
    }

    #[test]
    fn cannot_build_more_json_than_the_loader_accepts() {
        let mut p = plan();
        let before = p.clone();
        let mut items = Vec::new();
        for i in 0..128 {
            let item = WorkItem::new(format!("Task {i}"), "x".repeat(8192), now());
            items.push(PlanChange::Add(item));
        }
        assert!(matches!(
            p.apply_batch(1, now(), items),
            Err(PlanError::Invalid(_))
        ));
        assert_eq!(p, before);
        assert_eq!(Plan::from_json(&p.to_json_pretty().unwrap()).unwrap(), p);
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
