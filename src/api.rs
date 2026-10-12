//! Strict request types shared by MCP and the independent authority.
//! Request metadata is untrusted; identity is taken from OS peer credentials.
use crate::planning::{
    Disposition, Plan, PlanChange, PlanError, Priority, Schedule, WorkItem, WorkItemKind,
};
use chrono::Utc;
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewPlan {
    /// Name for the one configured local plan.
    pub title: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    Submit,
    ReturnToDraft,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewInput {
    pub expected_revision: u64,
    pub action: ReviewAction,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DraftPriority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DraftKind {
    Task,
    Milestone,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DraftDisposition {
    Proposed,
    Ready,
    Blocked,
}

/// Item is a *complete replacement* in edit.replace_item, not a partial patch.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemDraft {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    acceptance_criteria: Vec<String>,
    #[serde(default)]
    required_gate_ids: Vec<String>,
    #[serde(default)]
    priority: Option<DraftPriority>,
    #[serde(default)]
    kind: Option<DraftKind>,
    #[serde(default)]
    disposition: Option<DraftDisposition>,
    #[serde(default)]
    blocker: Option<String>,
    #[serde(default)]
    not_before: Option<String>,
    #[serde(default)]
    due_at: Option<String>,
    #[serde(default)]
    estimate_minutes: Option<u32>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    labels: BTreeSet<String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditOperation {
    AddItem {
        item: ItemDraft,
    },
    ReplaceItem {
        id: String,
        item: ItemDraft,
    },
    RemoveItem {
        id: String,
    },
    SetDetails {
        title: String,
        description: String,
    },
    SetMetadata {
        owner: Option<String>,
        #[serde(default)]
        labels: BTreeSet<String>,
        #[serde(default)]
        annotations: BTreeMap<String, String>,
    },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditInput {
    pub expected_revision: u64,
    pub edit: EditOperation,
}

fn invalid(reason: impl Into<String>) -> PlanError {
    PlanError::Invalid(reason.into())
}

fn parse_uuid(raw: &str) -> Result<Uuid, PlanError> {
    Uuid::parse_str(raw).map_err(|_| invalid("invalid work-item UUID"))
}

fn parse_timestamp(raw: Option<String>) -> Result<Option<chrono::DateTime<Utc>>, PlanError> {
    raw.map(|value| {
        chrono::DateTime::parse_from_rfc3339(&value)
            .map(|date| date.with_timezone(&Utc))
            .map_err(|_| invalid("timestamp must be RFC3339 with an offset"))
    })
    .transpose()
}

fn construct_item(draft: ItemDraft, previous: Option<&WorkItem>) -> Result<WorkItem, PlanError> {
    let mut item = if let Some(old) = previous {
        old.clone()
    } else {
        WorkItem::new(&draft.title, &draft.description, Utc::now())
    };
    item.title = draft.title;
    item.description = draft.description;
    item.parent_id = draft.parent_id.as_deref().map(parse_uuid).transpose()?;
    item.depends_on = draft
        .depends_on
        .iter()
        .map(|dependency| parse_uuid(dependency))
        .collect::<Result<_, _>>()?;
    item.acceptance_criteria = draft.acceptance_criteria;
    item.required_gate_ids = draft.required_gate_ids;
    item.priority = match draft.priority.unwrap_or(DraftPriority::Normal) {
        DraftPriority::Low => Priority::Low,
        DraftPriority::Normal => Priority::Normal,
        DraftPriority::High => Priority::High,
        DraftPriority::Critical => Priority::Critical,
    };
    item.kind = match draft.kind.unwrap_or(DraftKind::Task) {
        DraftKind::Task => WorkItemKind::Task,
        DraftKind::Milestone => WorkItemKind::Milestone,
    };
    item.disposition = match draft.disposition.unwrap_or(DraftDisposition::Proposed) {
        DraftDisposition::Proposed => Disposition::Proposed,
        DraftDisposition::Ready => Disposition::Ready,
        DraftDisposition::Blocked => Disposition::Blocked,
    };
    item.blocker = draft.blocker;
    item.schedule = Schedule {
        not_before: parse_timestamp(draft.not_before)?,
        due_at: parse_timestamp(draft.due_at)?,
        estimate_minutes: draft.estimate_minutes,
    };
    item.metadata.owner = draft.owner;
    item.metadata.labels = draft.labels;
    item.metadata.annotations = draft.annotations;
    Ok(item)
}

pub fn make_edit(plan: &Plan, edit: EditOperation) -> Result<PlanChange, PlanError> {
    Ok(match edit {
        EditOperation::AddItem { item } => PlanChange::Add(construct_item(item, None)?),
        EditOperation::ReplaceItem { id, item } => {
            let id = parse_uuid(&id)?;
            let current = plan.get_item(id).ok_or(PlanError::UnknownItem(id))?;
            PlanChange::Replace(construct_item(item, Some(current))?)
        }
        EditOperation::RemoveItem { id } => PlanChange::Remove(parse_uuid(&id)?),
        EditOperation::SetDetails { title, description } => {
            PlanChange::SetDetails { title, description }
        }
        EditOperation::SetMetadata {
            owner,
            labels,
            annotations,
        } => PlanChange::SetMetadata {
            owner,
            labels,
            annotations,
        },
    })
}
