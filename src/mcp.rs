//! Thin MCP adapter over the planning model and local file store.
//! MCP discovery never grants authority to approve work or fabricate receipts.

use crate::planning::{
    Disposition, PlanChange, PlanError, Priority, Schedule, WorkItem, WorkItemKind,
};
use crate::store::{PlanStore, StoreError};
use chrono::Utc;
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, schemars::JsonSchema, tool,
    tool_handler, tool_router, ServerHandler,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Clone)]
pub struct TaiyakiMcp {
    store: PlanStore,
}

impl TaiyakiMcp {
    pub fn new(store: PlanStore) -> Self {
        Self { store }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewPlan {
    /// Name for the one configured local plan.
    title: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    Submit,
    ReturnToDraft,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewInput {
    expected_revision: u64,
    action: ReviewAction,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DraftPriority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DraftKind {
    Task,
    Milestone,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DraftDisposition {
    Proposed,
    Ready,
    Blocked,
}

/// Item is a *complete replacement* in edit.replace_item, not a partial patch.
#[derive(Debug, Deserialize, JsonSchema)]
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

#[derive(Debug, Deserialize, JsonSchema)]
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditInput {
    expected_revision: u64,
    edit: EditOperation,
}

fn invalid(reason: impl Into<String>) -> StoreError {
    StoreError::Plan(PlanError::Invalid(reason.into()))
}

fn parse_uuid(raw: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(raw).map_err(|_| invalid("invalid work-item UUID"))
}

fn parse_timestamp(raw: Option<String>) -> Result<Option<chrono::DateTime<Utc>>, StoreError> {
    raw.map(|value| {
        chrono::DateTime::parse_from_rfc3339(&value)
            .map(|date| date.with_timezone(&Utc))
            .map_err(|_| invalid("timestamp must be RFC3339 with an offset"))
    })
    .transpose()
}

fn construct_item(draft: ItemDraft, previous: Option<&WorkItem>) -> Result<WorkItem, StoreError> {
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

fn tool_result(result: Result<crate::planning::Plan, StoreError>) -> CallToolResult {
    match result {
        Ok(plan) => {
            let value = serde_json::to_value(plan).expect("validated plan serializes");
            CallToolResult::structured(value)
        }
        Err(error) => {
            let kind = match &error {
                StoreError::NotFound => "not_found",
                StoreError::AlreadyExists => "already_exists",
                StoreError::Busy => "busy",
                StoreError::Plan(PlanError::RevisionConflict { .. }) => "revision_conflict",
                StoreError::Plan(PlanError::ReviewLocked) => "review_locked",
                StoreError::Plan(PlanError::NotReady(_)) => "not_ready",
                StoreError::Plan(_) => "invalid_plan",
                StoreError::UnsafePath(_) => "unsafe_storage",
                StoreError::Io(_) => "storage_error",
            };
            CallToolResult::structured_error(json!({
                "code": kind,
                "message": error.to_string()
            }))
        }
    }
}

#[tool_router]
impl TaiyakiMcp {
    /// Read the configured plan and its current revision. No verification is performed.
    #[tool(
        name = "plan_get",
        description = "Read the configured plan and its current revision"
    )]
    fn plan_get(&self) -> CallToolResult {
        tool_result(self.store.get())
    }

    /// Initialize the configured plan file once. No paths are accepted from callers.
    #[tool(
        name = "plan_create",
        description = "Create the single local draft plan if it does not exist"
    )]
    fn plan_create(&self, Parameters(args): Parameters<NewPlan>) -> CallToolResult {
        tool_result(self.store.create(args.title, args.description))
    }

    /// Apply one declarative edit against the current plan revision; never execute gates.
    #[tool(
        name = "plan_edit",
        description = "Edit draft plan items or metadata using an expected revision"
    )]
    fn plan_edit(&self, Parameters(args): Parameters<EditInput>) -> CallToolResult {
        let result = self.store.apply(args.expected_revision, |plan| {
            Ok(match args.edit {
                EditOperation::AddItem { item } => PlanChange::Add(construct_item(item, None)?),
                EditOperation::ReplaceItem { id, item } => {
                    let id = parse_uuid(&id)?;
                    let current = plan
                        .get_item(id)
                        .ok_or(StoreError::Plan(PlanError::UnknownItem(id)))?;
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
        });
        tool_result(result)
    }

    /// Submit draft for review or return to draft; does NOT approve or complete work.
    #[tool(
        name = "plan_review",
        description = "Submit a ready draft for review or return it to draft; never approve"
    )]
    fn plan_review(&self, Parameters(args): Parameters<ReviewInput>) -> CallToolResult {
        let change = match args.action {
            ReviewAction::Submit => PlanChange::SubmitForReview,
            ReviewAction::ReturnToDraft => PlanChange::ReturnToDraft,
        };
        tool_result(self.store.apply(args.expected_revision, |_| Ok(change)))
    }
}

#[tool_handler(
    name = "taiyaki",
    version = "0.1.0",
    instructions = "Local draft planning only. Submission is not approval; no verification or done tools are available."
)]
impl ServerHandler for TaiyakiMcp {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn private_dir() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        dir
    }

    #[test]
    fn only_four_tools_are_advertised() {
        let dir = private_dir();
        let _server = TaiyakiMcp::new(PlanStore::open(dir.path().join("plan.json")).unwrap());
        let tools = TaiyakiMcp::tool_router().list_all();
        let names: BTreeSet<String> = tools.iter().map(|tool| tool.name.to_string()).collect();
        assert_eq!(
            names,
            ["plan_create", "plan_edit", "plan_get", "plan_review"]
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        );
        assert!(!names.contains("approve"));
        assert!(!names.contains("done"));
        assert!(!names.contains("verify"));
        for tool in tools {
            assert!(!tool.input_schema.is_empty());
        }
    }

    #[test]
    fn sdk_tools_persist_edits_and_reject_stale_writes() {
        let dir = private_dir();
        let server = TaiyakiMcp::new(PlanStore::open(dir.path().join("plan.json")).unwrap());
        assert_eq!(server.plan_get().is_error, Some(true));
        let created = server.plan_create(Parameters(NewPlan {
            title: "Small MCP".into(),
            description: "First adapter".into(),
        }));
        assert_eq!(created.is_error, Some(false));
        assert_eq!(created.structured_content.as_ref().unwrap()["revision"], 1);

        let added = server.plan_edit(Parameters(EditInput {
            expected_revision: 1,
            edit: EditOperation::AddItem {
                item: ItemDraft {
                    title: "Test the adapter".into(),
                    description: String::new(),
                    parent_id: None,
                    depends_on: vec![],
                    acceptance_criteria: vec!["Call denied when stale".into()],
                    required_gate_ids: vec!["unit-tests".into()],
                    priority: Some(DraftPriority::High),
                    kind: None,
                    disposition: Some(DraftDisposition::Ready),
                    blocker: None,
                    not_before: None,
                    due_at: None,
                    estimate_minutes: None,
                    owner: None,
                    labels: BTreeSet::new(),
                    annotations: BTreeMap::new(),
                },
            },
        }));
        assert_eq!(added.is_error, Some(false));
        assert_eq!(added.structured_content.as_ref().unwrap()["revision"], 2);
        assert_eq!(
            server.plan_get().structured_content.as_ref().unwrap()["work_items"][0]["priority"],
            "high"
        );
        let stale = server.plan_review(Parameters(ReviewInput {
            expected_revision: 1,
            action: ReviewAction::Submit,
        }));
        assert_eq!(stale.is_error, Some(true));
        assert_eq!(
            stale.structured_content.as_ref().unwrap()["code"],
            "revision_conflict"
        );
        let submitted = server.plan_review(Parameters(ReviewInput {
            expected_revision: 2,
            action: ReviewAction::Submit,
        }));
        assert_eq!(
            submitted.structured_content.as_ref().unwrap()["review_state"],
            "submitted"
        );
        let locked = server.plan_edit(Parameters(EditInput {
            expected_revision: 3,
            edit: EditOperation::SetDetails {
                title: "mutate".into(),
                description: String::new(),
            },
        }));
        assert_eq!(locked.is_error, Some(true));
        assert_eq!(
            locked.structured_content.as_ref().unwrap()["code"],
            "review_locked"
        );
    }
}
