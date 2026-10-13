//! MCP stdio adapter; all state changes require the independently authenticated authority.
use crate::{
    api::{EditInput, NewPlan, ReviewInput},
    authority::{Client, Request},
    workflow::TestGate,
};
use rmcp::schemars::JsonSchema;
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_handler, tool_router,
    ServerHandler,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NextInput {
    expected_revision: u64,
    agent: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReportInput {
    expected_revision: u64,
    #[schemars(with = "String")]
    item_id: Uuid,
    #[schemars(with = "String")]
    lease: Uuid,
    notes: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DefineTestInput {
    expected_revision: u64,
    gate: TestGate,
}

#[derive(Clone)]
pub struct TaiyakiMcp {
    client: Client,
}
impl TaiyakiMcp {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
    async fn invoke(&self, request: Request) -> CallToolResult {
        let client = self.client.clone();
        let result = tokio::task::spawn_blocking(move || client.request(request)).await;
        match result {
            Ok(Ok(data)) => CallToolResult::structured(data),
            Ok(Err((code, message))) => {
                CallToolResult::structured_error(json!({"code":code,"message":message}))
            }
            Err(_) => CallToolResult::structured_error(
                json!({"code":"internal_error","message":"authority request failed"}),
            ),
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
    async fn plan_get(&self) -> CallToolResult {
        self.invoke(Request::Get).await
    }

    /// Initialize the configured plan file once. No paths are accepted from callers.
    #[tool(
        name = "plan_create",
        description = "Create the single local draft plan if it does not exist"
    )]
    async fn plan_create(&self, Parameters(args): Parameters<NewPlan>) -> CallToolResult {
        self.invoke(Request::Create { args }).await
    }

    /// Apply one declarative edit against the current plan revision; never execute gates.
    #[tool(
        name = "plan_edit",
        description = "Edit draft plan items or metadata using an expected revision"
    )]
    async fn plan_edit(&self, Parameters(args): Parameters<EditInput>) -> CallToolResult {
        self.invoke(Request::Edit {
            args: Box::new(args),
        })
        .await
    }

    /// Submit draft for review or return to draft; does NOT approve or complete work.
    #[tool(
        name = "plan_review",
        description = "Submit a ready draft for review or return it to draft; never approve"
    )]
    async fn plan_review(&self, Parameters(args): Parameters<ReviewInput>) -> CallToolResult {
        self.invoke(Request::Review { args }).await
    }

    /// Read current work-item phases, gates and workflow revision (no lease tokens).
    #[tool(
        name = "work_status",
        description = "Read authoritative workflow phases and registered Test Gates"
    )]
    async fn work_status(&self) -> CallToolResult {
        self.invoke(Request::WorkStatus).await
    }

    /// Atomically claim next eligible leaf work item after dependencies finish.
    #[tool(
        name = "work_next",
        description = "Claim one available work item, ordered by dependency and priority"
    )]
    async fn work_next(&self, Parameters(args): Parameters<NextInput>) -> CallToolResult {
        self.invoke(Request::WorkNext {
            expected_revision: args.expected_revision,
            agent: args.agent,
        })
        .await
    }

    /// Report implementation and request independent gate verification.
    #[tool(
        name = "work_report",
        description = "Submit a work item for verification using its lease; cannot declare DONE"
    )]
    async fn work_report(&self, Parameters(args): Parameters<ReportInput>) -> CallToolResult {
        self.invoke(Request::WorkReport {
            expected_revision: args.expected_revision,
            item_id: args.item_id,
            lease: args.lease,
            notes: args.notes,
        })
        .await
    }

    /// Register a Test Gate definition, not a trusted PASS receipt.
    #[tool(
        name = "test_define",
        description = "Register a typed Test Gate. Status must be not_run; verification is independent"
    )]
    async fn test_define(&self, Parameters(args): Parameters<DefineTestInput>) -> CallToolResult {
        self.invoke(Request::TestDefine {
            expected_revision: args.expected_revision,
            gate: args.gate,
        })
        .await
    }
}

#[tool_handler(
    name = "taiyaki",
    version = "0.1.0",
    instructions = "Taiyaki plans and assigns WorkItems. Implementation reports never set Test Gate PASS or mark DONE; only a separate verifier may do that."
)]
impl ServerHandler for TaiyakiMcp {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    #[test]
    fn narrow_discovery() {
        let names: BTreeSet<_> = TaiyakiMcp::tool_router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        assert_eq!(
            names,
            [
                "plan_create",
                "plan_edit",
                "plan_get",
                "plan_review",
                "test_define",
                "work_next",
                "work_report",
                "work_status"
            ]
            .into_iter()
            .map(String::from)
            .collect()
        );
    }
}
