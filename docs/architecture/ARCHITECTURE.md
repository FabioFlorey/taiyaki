# Taiyaki architecture

Taiyaki currently provides a Rust planning model, a **local stdio MCP adapter**,
and a separate static website. A separately authenticated authority server
and verification worker are not yet implemented.

## Implemented components

The [planning model](../reference/PLANNING.md) defines versioned plans,
work items, metadata, dependency graphs, acceptance criteria, and
revision-checked edits.

The [local MCP adapter](../reference/MCP.md) exposes four typed tools:
`plan_get`, `plan_create`, `plan_edit`, and `plan_review`.
It uses the official RMCP SDK to handle protocol negotiation and schemas.

```text
Local MCP client
      |
      | stdio
      v
taiyaki-mcp (client's OS identity)
      |
      | declarative plan operations
      v
Rust planning model
      |
      | revision checks and filesystem lock
      v
Private JSON plan file (atomic replacement)
```

No tool accepts arbitrary filesystem paths or executes shell commands.

## Trust boundaries

The current adapter runs under its client's OS identity. Another process
with that identity may modify the plan file directly. Plan submission is
**not authenticated approval**. Metadata and timestamps are declarative;
verification gate identifiers are not trusted PASS receipts.

An independent authority, server-enforced phase gates, an isolated verifier,
receipt freshness checks, and mandatory durable transition auditing remain
unimplemented.

## Verification

`cargo test --locked --all-targets` covers the planning model, private
file store and actual MCP stdio exchanges. `bash scripts/check-site.sh`
validates the static website. These tests do not establish the intended
independent security invariants.
