# Local MCP planning adapter

Taiyaki has a **local stdio MCP server** for editing one draft plan. Its Rust
implementation uses the [official RMCP SDK](https://github.com/modelcontextprotocol/rust-sdk),
a small set of typed tools, and the existing planning model.

The adapter is intended for local development. It is **not** a separately
authenticated authority, a verification worker, or a remote MCP endpoint.

## Start

From the repository root:

```sh
cargo build --locked --bin taiyaki-mcp
install -d -m 700 "${XDG_STATE_HOME:-$HOME/.local/state}/taiyaki"
export TAIYAKI_PLAN_FILE="${XDG_STATE_HOME:-$HOME/.local/state}/taiyaki/plan.json"
./target/debug/taiyaki-mcp
```

Launch the binary **as a child process of an MCP client**, using the same
environment variable. The binary communicates over stdin/stdout; standard
output is reserved for protocol messages. Starting it without
`TAIYAKI_PLAN_FILE` fails closed.

Example MCP client configuration (replace the absolute paths with yours):

```json
{
  "mcpServers": {
    "taiyaki": {
      "command": "/absolute/path/to/taiyaki/target/debug/taiyaki-mcp",
      "env": {
        "TAIYAKI_PLAN_FILE": "/absolute/private/directory/plan.json"
      }
    }
  }
}
```

The containing directory must already exist and deny group and other access
(Unix mode `0700`). Plan and lock files are stored with mode `0600`.
The adapter rejects symlinks, unsafe file permissions, and paths supplied
relative to the working directory.

## Tool surface

| Tool | Input | Behavior |
| --- | --- | --- |
| `plan_get` | No arguments | Read the current plan, metadata and revision. |
| `plan_create` | `title`, optional `description` | Create the configured draft plan once. |
| `plan_edit` | `expected_revision`, `edit` | Apply one revision-checked declarative edit. |
| `plan_review` | `expected_revision`, `action` | `submit` or `return_to_draft`. Not approval. |

`plan_edit.edit` has a required `action`, one of
`add_item`, `replace_item`, `remove_item`, `set_details`, or
`set_metadata`. The first two actions use an `item` object. For
`replace_item`, supply the item's UUID as `id` and a **complete**
replacement `item` (omitted optional fields reset to their defaults).
The remaining operations use `id`, `title`/`description`, or
`owner`/`labels`/`annotations`, respectively.

An item supports `title`, `description`, `parent_id`, `depends_on`,
`acceptance_criteria`, `required_gate_ids`, `priority`, `kind`,
`disposition`, `blocker`, `not_before`, `due_at`,
`estimate_minutes`, `owner`, `labels`, and `annotations`.
UUID references must already exist in the plan. The server generates the
new work item's UUID and timestamps; it validates the resulting plan using
the Rust model.

Example tool call arguments:

```json
{
  "expected_revision": 1,
  "edit": {
    "action": "add_item",
    "item": {
      "title": "Implement planning checks",
      "priority": "high",
      "disposition": "ready",
      "acceptance_criteria": ["Checks reject invalid plans"],
      "required_gate_ids": ["cargo-test"]
    }
  }
}
```

Tool responses include a JSON `structuredContent` containing the updated
plan, or `isError` and a structured error with `code` and `message`.
Every successful edit changes the plan revision exactly once. A stale
`expected_revision` is rejected without writing to disk.

## Persistence and boundaries

The adapter locks a fixed lock file for each read or mutation and writes
changes by syncing a temporary file, renaming it over the plan, then syncing
the parent directory. This is a local file-store mechanism, **not** an
authenticated audit or receipt system.

An MCP caller cannot specify arbitrary filesystem paths or invoke arbitrary
shell commands through these four tools. However, the stdio process runs
under the **client's OS identity**. Another process with that same identity
and filesystem access may modify the plan file directly and bypass review
locking. Do not treat plan metadata or review submission as trusted proof.

The adapter deliberately provides **no `approve`, `verify`, `done`,
arbitrary command execution, or MCP-over-network transport**. Independent
verification, durable audit, authenticated caller identity, and phase-tool
enforcement remain separate, unimplemented authority capabilities.

## Verification

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
```

`tests/mcp_stdio.rs` exercises the compiled MCP process over a real
stdio connection. It verifies initialization, discovery, calls, rejections,
revision checks, review locking, and persistence across restart.
