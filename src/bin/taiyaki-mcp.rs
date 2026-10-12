//! Local-only stdio MCP adapter; stdout is reserved for MCP messages.
use rmcp::{transport::stdio, ServiceExt};
use std::{env, io};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::var_os("TAIYAKI_PLAN_FILE")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "set TAIYAKI_PLAN_FILE to a filename in a private (0700) directory",
            )
        })?;
    let store = taiyaki::store::PlanStore::open(path)?;
    let service = taiyaki::mcp::TaiyakiMcp::new(store).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
