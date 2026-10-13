# Taiyaki: MCP, WorkItems and independent Test Gates

Taiyaki provides a local MCP adapter, a **separately authenticated SQLite
authority**, and an optional independent Test Gate verifier. The adapter and
authority are Rust binaries; the verifier is another Rust binary running under
its own Linux identity.

## Execution architecture

```text
Model / MCP client (client Unix UID)
  |
  | stdio: 8 typed MCP tools
  v
taiyaki-mcp
  |
  | Unix socket, SO_PEERCRED + fresh HMAC-SHA256 challenge
  v
taiyaki-authority (different authority Unix UID)
  |                                  ^
  | SQLite transactions             | independent verifier UID
  v                                  | + separate HMAC key
plans.sqlite                         |
                           taiyaki-verifier
                              |
                              | operator-owned catalog:
                              | fixed program, args, test source SHA256,
                              | complete Test Gate definition SHA256
                              v
                       Real OS test process
                       (same UID as verifier; see limits)
```

There is no remote MCP endpoint. The authority controls plan mutations, work
assignments, phases, gate status and receipts; an MCP agent cannot directly
write SQLite, submit trusted PASS results or set DONE.

## Build and provision

```sh
cargo build --locked --bins
```

Provision three **distinct** Linux identities for the MCP client, the
authority and the verifier, as well as:

- An authority-owned **0700** directory containing `plans.sqlite` (**0600**).
- A socket directory owned by the authority and not writable by other users.
  The socket itself is **0666** for connection attempts; the authority always
  requires an allowed peer UID and valid HMAC before processing any operation.
- Authority and client private key files containing the same random **32-byte
  secret**, encoded as 64 hex characters, with **0600** permissions.
- A **second, different secret** stored in authority- and verifier-owned
  **0600** files.
- A verifier-owned private **0700** directory containing its **0600** test
  catalog, and a verifier-owned source checkout where the test source files
  cannot be changed by other Unix users.

Key generation (run only when securely provisioning keys):

```sh
openssl rand -hex 32
```

Do not place keys in Git, MCP arguments, or an agent-readable workspace.
The binaries do not create Unix accounts or long-running services.

Configure and start `taiyaki-authority` **under the authority identity**:

```sh
TAIYAKI_SOCKET_PATH=/authority/socket/taiyaki.sock
TAIYAKI_DB_FILE=/authority/private/plans.sqlite
TAIYAKI_SECRET_FILE=/authority/private/client.key
TAIYAKI_CLIENT_UID=1234
TAIYAKI_VERIFIER_UID=2345
TAIYAKI_VERIFIER_SECRET_FILE=/authority/private/verifier.key
/path/to/taiyaki-authority
```

Replace both numeric UIDs and all absolute paths. The authority refuses
an identity collision and refuses an existing socket path instead of
silently deleting it. The verifier configuration is optional for planning
but **required to produce trusted PASS receipts or DONE**.

Start `taiyaki-mcp` under the permitted client identity. Your MCP host
should launch it as a stdio subprocess:

```sh
TAIYAKI_SOCKET_PATH=/authority/socket/taiyaki.sock
TAIYAKI_SECRET_FILE=/client/private/client.key
/path/to/taiyaki-mcp
```

Start `taiyaki-verifier` under the configured **verifier identity**:

```sh
TAIYAKI_SOCKET_PATH=/authority/socket/taiyaki.sock
TAIYAKI_SECRET_FILE=/verifier/private/verifier.key
TAIYAKI_TEST_CATALOG=/verifier/private/catalog.json
/path/to/taiyaki-verifier
```

It polls for WorkItems in `verifying` every two seconds. Use
`taiyaki-verifier --once` to run one verification sweep instead.

## MCP tools

| Tool | Input | Responsibility |
| --- | --- | --- |
| `plan_get` | None | Read the plan and plan revision. |
| `plan_create` | `title`, optional `description` | Create the single draft plan. |
| `plan_edit` | `expected_revision`, `edit` | Edit a draft plan and WorkItem definitions. |
| `plan_review` | `expected_revision`, `action` | `submit` / `return_to_draft`. Submission is not authenticated approval. |
| `work_status` | None | Read workflow revision, phases, Test Gates and receipts. Never reveals assignment lease tokens. |
| `test_define` | `expected_revision`, `gate` | Define a referenced Test Gate **without selecting its status**. |
| `work_next` | `expected_revision`, `agent` | Atomically claim the next eligible leaf task and return its WorkItem and lease. |
| `work_report` | `expected_revision`, `item_id`, `lease`, `notes` | Submit an implementation report and enter `verifying`; cannot mark DONE. |

`work_next` selects work after dependencies are DONE, subject to
`not_before`, readiness, defined gate IDs and critical/high/normal/low
priority. The `agent` label is descriptive; identity at the transport
boundary is the authorized Unix UID. An assignment lease is required to
report implementation; it is returned only in the assignment response.

`plan_edit.edit` supports `add_item`, `replace_item`, `remove_item`,
`set_details` and `set_metadata`. Replacements are complete WorkItems,
not patches. Plan revisions and workflow revisions are **separate**:
`plan_edit` uses the former; `work_next`, `work_report`, and
`test_define` use the latter.

Example assignment request:

```json
{"expected_revision": 3, "agent": "coding-agent"}
```

Example implementation report (UUID values come from the assignment):

```json
{
  "expected_revision": 4,
  "item_id": "00000000-0000-4000-8000-000000000001",
  "lease": "00000000-0000-4000-8000-000000000002",
  "notes": "Implementation submitted for independent verification"
}
```

The plan must first be `submitted`; the workflow initializes when the plan
is submitted. Editing or returning the plan to draft **invalidates the
workflow and its receipts**. To modify definitions, reopen the draft and
resubmit. Parent WorkItems aggregate completed children; a parent with its
own Test Gate must also be independently verified.

## Test Gate: nine fields

| Field | Meaning |
| --- | --- |
| **Test Type** | Fixed enum: `functional`, `integration`, `regression`, `security`, `performance`, `quality_of_life`, `accessibility`, `reliability`. |
| **Test ID** | Unique identifier referenced by the WorkItem. |
| **Test Name** | Human-readable title. |
| **Test Description** | What is tested. |
| **Test Goal** | Behavior that must hold. |
| **Precondition** | Conditions for meaningful execution. |
| **Expected Result** | Concrete expected outcome. |
| **Test Code** | Relative path to the versioned test source in the approved checkout. |
| **Test Status** | Authority-managed `not_run`, `running`, `pass`, `fail`, `error`, or `skipped` (default `not_run`). |

A Test Gate may be authored by any agent or developer. That **does not**
make it a trusted test. A verifier operator must independently review the
full gate specification and source code and then pin both in the catalog.
No MCP tool can change `test_status` to `pass`, execute arbitrary test
source, or approve a gate.

Example `test_define` arguments:

```json
{
  "expected_revision": 1,
  "gate": {
    "test_type": "functional",
    "test_id": "test-phase-denial",
    "test_name": "Reject editing in verification phase",
    "test_description": "A cached editing capability remains denied in verifying.",
    "test_goal": "Prevent phase bypass.",
    "precondition": "A work item has entered verifying.",
    "expected_result": "The tool call returns forbidden and does not mutate state.",
    "test_code": "tests/phase_denial.py"
  }
}
```

## Approved verifier catalog

A catalog is a JSON object with `source_root` and `cases`. Each case
pins `test_id`, `test_code`, `test_code_sha256`,
`test_definition_sha256`, `executable` (absolute), `args` (fixed),
`success_marker` and optional `timeout_seconds` (1–120, default 30).

Example **template** (replace the placeholder hashes with actual reviewed
SHA256 digests before deployment):

```json
{
  "source_root": "/verifier/approved-checkout",
  "cases": [{
    "test_id": "test-phase-denial",
    "test_code": "tests/phase_denial.py",
    "test_code_sha256": "<64 hex characters>",
    "test_definition_sha256": "<64 hex characters>",
    "executable": "/usr/bin/python3",
    "args": ["tests/phase_denial.py"],
    "success_marker": "PHASE_DENIAL_TEST_PASSED",
    "timeout_seconds": 20
  }]
}
```

`test_code_sha256` is SHA256 of the reviewed test file. The definition
digest is SHA256 of canonical compact JSON (UTF-8, sorted keys) for the **full
Test Gate definition with `test_status: "not_run"`**. Compute it from the
gate reviewed and returned by `work_status`, not from unaudited input.
The catalog file must be an owner-private regular file inside a
verifier-owned **0700** directory; the checkout root and test source must
be owned by the verifier and not group/world writable. Only an operator
can curate the catalog; it is never generated from agent claims.

Before each execution the verifier checks that Git reports a clean checkout.
It confirms both approved hashes, runs the catalog executable with its
fixed arguments (without a shell), enforces a timeout and requires both
exit status zero **and** the expected output marker. A no-test execution
without the marker does not yield PASS. It checks the source file and tree
again afterward.

**Test validity = operator-reviewed definition and pinned source match.**
Matching an approved file and marker does not mathematically prove that the
test exercises its stated goal; that semantic judgment remains part of
the review. Tests can be written by other agents, but an operator must vet
their source and semantics before the verifier runs them.

## Phases and evidence

```text
Submitted planning definition
  -> READY -> IMPLEMENTING (lease)
  -> VERIFYING (agent implementation report)
  -> DONE     (only if all approved Test Gates are executed and PASS)
        |
        +-> READY with failure feedback when any test fails
```

The independent verifier has a **different Unix UID and its own HMAC key**.
The authority rejects any attempt by the MCP identity to invoke the
internal verifier operations. A successful verification commits Test
Statuses, receipt hashes, the plan revision and work phase in one SQLite
transaction.

Receipts include work item, gate, plan revision, source tree fingerprint,
test file SHA256, observed timestamp, and verified outcome. When the
verifier detects a changed or dirty source tree, it invalidates old
DONE phases and downstream dependency assignments; a fresh verification
is required. This check occurs during verifier polling, not instantly
when arbitrary files change.

## Security and current limits

- This is a **local** workflow. Taiyaki exposes WorkItems over MCP but
  does not itself host or invoke a general-purpose language model.
  An MCP-capable agent/orchestrator calls `work_next`, edits source, and
  submits `work_report`.
- The test process runs as the verifier Unix UID, **not in a separate
  container or sandbox**. Operator-vetted tests are required. A malicious
  catalog-approved executable can access files available to that UID,
  including its secret. For untrusted generated test code, another
  restricted execution identity/sandbox remains necessary.
- The verifier must run and poll to maintain freshness. Stopping it
  suspends actual verification and delayed invalidation of old receipts.
- The source checkout used to verify is a verifier-owned reviewed
  snapshot. Taiyaki does not yet automatically import agent edits
  into that snapshot or approve test changes.
- Only one plan is stored. SQLite includes schema migrations from v1 to
  v2; JSON-file plans are not automatically imported.
- The workflow stores receipts, but not yet a complete tamper-evident
  audit ledger. Some intended security invariants remain unimplemented.

## Real checks

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
```

On Linux, the real integration test provisions **three separate Unix UIDs**
for the MCP client, authority and verifier; executes an actual positive
and negative test, observes fail/retry, rejects a forged MCP PASS request,
checks dependency ordering, and invalidates stale receipts after a source
change. CI requires this test; without passwordless `sudo`, the local
cross-UID case explicitly reports a skip.
