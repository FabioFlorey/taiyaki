//! Unprivileged MCP adapter; the local authority owns all persistent state.
use rmcp::{transport::stdio, ServiceExt};
use std::{env, io, path::PathBuf};

fn required(name: &str) -> io::Result<PathBuf> {
    env::var_os(name)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("missing {name}")))
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket = required("TAIYAKI_SOCKET_PATH")?;
    let key = required("TAIYAKI_SECRET_FILE")?;
    let client = taiyaki::authority::Client::open(socket, &key)?;
    let service = taiyaki::mcp::TaiyakiMcp::new(client).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
