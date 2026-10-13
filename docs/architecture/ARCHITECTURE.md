# Taiyaki architecture

Taiyaki currently implements a Rust planning model, a local stdio MCP adapter,
an independently authenticated SQLite authority, and an optional separate
Test Gate verifier. The static website remains a distinct component.

## Work orchestration

```text
AI model / MCP host
    |
    | plan_* / work_next / work_report / test_define
    v
MCP adapter (client UID)
    |
    | Unix socket, peer UID + HMAC
    v
Authority (separate UID)
    |
    +--> versioned plan + WorkItem state (SQLite)
    |        |
    |        +--> dependency-aware scheduling and lease ownership
    |
    +--> verifier-only work verification operations
             ^
             | verifier UID + separate HMAC
        Verifier process
             |
             +--> approved Test Gate catalog + pinned test source
             +--> real test command, bounded timeout
             +--> source tree freshness checks and PASS receipts
```

The [planning model](../reference/PLANNING.md) defines work-item structure,
metadata, dependencies and review submission.

The [MCP and verifier reference](../reference/MCP.md) documents the eight
MCP operations, full nine-field Test Gate schema, source hashes,
phase transitions, independent verifier provisioning and SQLite storage.

### Assignment and execution state

The Authority selects an eligible leaf WorkItem only when dependencies are
DONE, readiness and scheduling allow execution, and required gate
definitions exist. The assignment includes a lease; a model can submit an
implementation report with that lease but cannot mark its work DONE.
The separate verifier checks preapproved Test Gate definitions and executes
real tests before an authoritative PASS receipt can be stored.

When a test fails, work returns to READY with feedback. When the verifier
detects a changed checkout, it invalidates stale DONE states and downstream
dependency assignments, requiring fresh verification. Parent milestones
aggregate verified children and may also require their own tests.

## Security boundaries

Linux Unix UIDs for the client, authority and verifier must be different.
Each connection to the authority requires both the expected kernel peer
identity and a fresh HMAC-SHA256 challenge response. The verifier's identity
and key permit verification operations; the MCP client's identity and key
do not. The database belongs only to the authority's Unix identity.

Test definitions can originate from any agent, but actual execution requires
an operator-reviewed catalog pinning both the **complete gate definition**
and the test source file. The verifier runs only fixed catalog commands;
it does not execute arbitrary MCP-supplied commands or source.

**Current limitations:** The verifier executes catalog-approved tests under
its own UID, without a separate untrusted-code sandbox; it must only run
operator-vetted tests. Test semantic relevance is not automatically provable
and requires human review. Reverification of modified source occurs when
the verifier polls, rather than instantly. The coding model itself still
requires an external MCP host/orchestrator. A complete tamper-evident
audit ledger, multi-plan support and fully automated reviewed snapshot
promotion are not yet implemented.

## Verification

Rust unit tests, cross-UID integration tests with real MCP and independent
test processes, formatting, Clippy and static-site checks run in CI.
The cross-UID suite is mandatory on Linux CI and fails if it cannot run.
