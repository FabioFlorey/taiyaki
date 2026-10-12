//! MCP stdio adapter; all state changes require the independently authenticated authority.
use crate::{
    api::{EditInput, NewPlan, ReviewInput},
    authority::{Client, Request},
};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_handler, tool_router,
    ServerHandler,
};
use serde_json::json;

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
        let result = tokio::task::spawn_blocking(move || client.call(request)).await;
        match result {
            Ok(Ok(plan)) => {
                CallToolResult::structured(serde_json::to_value(plan).expect("plan serializable"))
            }
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
            ["plan_create", "plan_edit", "plan_get", "plan_review"]
                .into_iter()
                .map(String::from)
                .collect()
        );
    }
}
