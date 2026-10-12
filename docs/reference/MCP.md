# Local MCP adapter and SQLite authority

Taiyaki's implemented MCP interface is a **local stdio adapter** to a
separate **Unix-socket authority**. The authority alone opens a SQLite
database. Protocol handling uses the official [RMCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk).

The integration is Linux/Unix-only and is not a remotely exposed MCP server.

## Processes and trust boundary

```text
MCP client (approved client UID)
    |
    | stdio
    v
taiyaki-mcp (approved client UID)
    |
    | Unix socket; SO_PEERCRED UID check + fresh HMAC-SHA256 challenge
    v
taiyaki-authority (dedicated, different Unix UID)
    |
    | SQLite transaction: reload, validate, revise, commit
    v
Private plans.sqlite (authority UID, private 0700 directory)
```

The **authority** must run under a dedicated Unix account, distinct from the
**adapter** account. It refuses startup when those UIDs are identical.
The socket owner must be the authority; connecting peers are checked against
one configured client UID. A second, independent check requires proof of
possession of a 32-byte shared secret for each connection.

The authority sends a new unpredictable challenge for each connection. The
client responds with an HMAC-SHA256, using domain separation. The shared
secret is never transmitted across the socket. An expired/reused response
does not authenticate another connection.

The socket is mode `0666` so a potential client can reach it for
authentication. The **socket directory** must be owned by the authority
and not writable by other users, and the authority requires a matching
kernel peer UID **and** a valid challenge response before reading an
operation. Socket filesystem permissions alone do not grant access.

## Build and configure

```sh
cargo build --locked --bins
```

Provision **two separate Unix accounts** and private credentials before
starting the binaries. For example, use the names `taiyaki-auth` (authority)
and `taiyaki-client` (MCP adapter), created by the machine administrator.
These accounts are examples; the implementation does **not** install users,
create directories, or generate secrets automatically.

Provision these paths as an administrator:

| Resource | Requirement |
| --- | --- |
| Authority database directory | Absolute, owned by authority UID, mode `0700` |
| Authority database path | Absolute filename inside database directory; SQLite file mode `0600` |
| Socket directory | Absolute, owned by authority UID, not group/world writable; client must be able to traverse it |
| Authority secret file | Owned by authority UID, regular file mode `0600`, 64 hex characters encoding 32 random bytes |
| Client secret file | Owned by approved client UID, regular file mode `0600`, **identical** secret bytes |
| Client UID | Numeric UID of the account permitted to connect; different from the authority UID |

Generate a random shared key securely (for example with `openssl rand -hex
32`) and place copies in the two accounts' private key files using your
administrator's normal provisioning mechanism. Do not commit either copy or
pass the key through MCP tool arguments.

Launch the **authority** under its own account, with these environment
variables set to absolute paths:

```sh
TAIYAKI_DB_FILE=/private/authority/data/plans.sqlite
TAIYAKI_SOCKET_PATH=/authority/owned/socket/authority.sock
TAIYAKI_SECRET_FILE=/private/authority/data/authority.key
TAIYAKI_CLIENT_UID=1234
/path/to/taiyaki-authority
```

Replace `1234` with the numeric UID of the permitted MCP adapter account.
A pre-existing socket path is rejected rather than silently deleted;
remove stale sockets only after confirming no authority process is running.

Launch **`taiyaki-mcp` as the permitted client UID** with:

```sh
TAIYAKI_SOCKET_PATH=/authority/owned/socket/authority.sock
TAIYAKI_SECRET_FILE=/private/client/credentials/taiyaki.key
/path/to/taiyaki-mcp
```

Configure your MCP client to launch `taiyaki-mcp` using its regular
stdio subprocess configuration. Do not supply database paths or authority
secrets to the MCP server as tool arguments.

## MCP tool surface

| Tool | Input | Behavior |
| --- | --- | --- |
| `plan_get` | None | Read the current plan and revision. |
| `plan_create` | `title`, optional `description` | Create the one configured draft plan, once. |
| `plan_edit` | `expected_revision`, `edit` | Atomically add, replace or remove an item, or update plan details/metadata. |
| `plan_review` | `expected_revision`, `action` | `submit` or `return_to_draft`; not approval. |

`plan_edit.edit` requires an `action`: `add_item`, `replace_item`,
`remove_item`, `set_details`, or `set_metadata`.
Item fields include `title`, `description`, `parent_id`, `depends_on`,
`acceptance_criteria`, `required_gate_ids`, `priority`, `kind`,
`disposition`, `blocker`, `not_before`, `due_at`,
`estimate_minutes`, `owner`, `labels`, and `annotations`.
An item replacement is a complete replacement, not a patch.

Example arguments for `plan_edit`:

```json
{
  "expected_revision": 1,
  "edit": {
    "action": "add_item",
    "item": {
      "title": "Test the plan boundary",
      "priority": "high",
      "disposition": "ready",
      "acceptance_criteria": ["Stale edits are rejected"],
      "required_gate_ids": ["cargo-test"]
    }
  }
}
```

All mutations reload the current SQLite record **inside an immediate
transaction**, compare the expected revision, validate the resulting plan,
and commit only valid changes. A rejected operation does not change the plan.
Successful tool calls return a structured plan; rejected calls return a
structured error code and message.

SQLite currently stores **one plan's latest serialized state**. It does not
yet store a full plan-revision history, verified receipts, or a mandatory
transition audit. Previous JSON-file stores are **not automatically migrated**
into SQLite.

## Security boundaries and limitations

- The MCP adapter cannot read or write the SQLite database directly.
  The database directory must be unreadable to the client OS identity.
- A caller cannot choose filesystem paths, execute shell commands or invoke
  a hidden `approve`, `verify`, or `done` tool through these four tools.
- The authority independently checks the kernel peer UID and a fresh
  HMAC response **on every socket connection**.
- **An unrelated MCP sharing the same permitted Unix UID can potentially
  read that UID's secret and impersonate the adapter.** To isolate different
  MCPs, use separate Unix identities and restrict which identity is allowed.
- Administrator/root access and processes running as the authority UID can
  access the database. Neither SQLite nor the secret protects against that.
- This implements only plan validation and review locking. It does **not**
  implement per-phase execution authorization (I1), verified PASS receipts
  (I2/I3), an independent verification worker, or durable mandatory audit
  provenance (I4).

## Tests

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
```

On Linux, the cross-UID test uses passwordless `sudo` to launch the
authority under the disposable `nobody` identity (test-only), and verifies
database access denial, wrong-key denial, peer-UID denial, replay resistance,
real MCP calls, revision conflicts, and restart persistence. Without sudo,
the local cross-UID test reports a skip. CI sets
`TAIYAKI_REQUIRE_ISOLATION_TEST=1` to make lack of those privileges a test
failure rather than silently skip.
