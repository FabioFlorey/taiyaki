# Taiyaki architecture

Taiyaki currently provides a Rust **planning data model** and a separate static
concept website. A runnable authority server, MCP endpoint and verification
worker are not part of the repository.

## Implemented Rust model

The `taiyaki` crate exposes a planning module in [`src/planning.rs`](../../src/planning.rs)
and basic project, task, subtask and test structures in
[`src/lib.rs`](../../src/lib.rs).

The planning module has versioned plans, work items, descriptive metadata,
dependencies, priorities, schedules, acceptance criteria and verification-gate
references. It checks graph validity, supports atomic revision-checked edits,
and serializes plans to JSON. A plan can be marked **submitted for review**;
this does not constitute approval or verified completion.

See the [planning model reference](../reference/PLANNING.md) for its supported
schema and edit API.

## Trust boundaries

Planning metadata is client-provided data, not authenticated identity or audit
evidence. A gate reference is not a PASS receipt. Model-level review locking
does not prevent an untrusted client from modifying stored JSON directly.

The following are **not implemented**: server-enforced tool access, trusted
execution receipts tied to source digests, isolated gate execution, durable
transition audit, and MCP or CLI authority adapters.

## Checks

`cargo test --locked --all-targets` exercises Rust model validation and
serialization. `bash scripts/check-site.sh` validates website assets and fish
animation logic. GitHub Actions CI runs both sets of checks.

These checks do not establish Taiyaki's intended security invariants.
