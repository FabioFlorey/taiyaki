# Planning model reference

The planning model is a Rust library in [`src/planning.rs`](../../src/planning.rs).
It describes work; it does **not** execute it, authorize state transitions, or issue PASS receipts.

## Plan

| Field | Type | Meaning |
| --- | --- | --- |
| `schema_version` | integer | Current schema is `2`; version `1` JSON is accepted and migrated on load. |
| `id` | UUID | Stable plan identity. |
| `revision` | integer | Starts at `1`; increases by one for every accepted edit or batch. |
| `title` / `description` | strings | Scope of the plan. |
| `metadata` | object | Timestamps, optional owner, labels and annotations. |
| `review_state` | `draft` or `submitted` | Submitted plans reject edits until returned to draft; this is not approval. |
| `work_items` | array | Task and milestone records. |

## Work items

Each work item has a UUID, optional `parent_id`, `title`, `description`,
`acceptance_criteria`, `required_gate_ids`, `depends_on`, and `metadata`.

| Field | Values | Meaning |
| --- | --- | --- |
| `kind` | `task`, `milestone` | Planning classification; default `task`. |
| `priority` | `low`, `normal`, `high`, `critical` | Priority; default `normal`. |
| `disposition` | `proposed`, `ready`, `blocked` | Planning readiness; default `proposed`. |
| `blocker` | string or null | Required for blocked items. |
| `schedule.not_before` | timestamp or null | Earliest intended start. |
| `schedule.due_at` | timestamp or null | Desired deadline, not earlier than `not_before`. |
| `schedule.estimate_minutes` | positive integer or null | Estimated work duration. |

Item IDs must be unique, references must resolve, and dependency and parent
graphs must not contain cycles. Acceptance criteria are text descriptions;
gate IDs are **references**, not executable commands or verification receipts.

## Metadata

Plan and work-item metadata include `created_at`, `updated_at`, optional
`owner` text, labels and string-keyed annotations. These fields are declarative:
they do **not** prove an actor's identity, trusted clock time, or audit provenance.

## Revision checks and review submission

`Plan::apply(expected_revision, at, change)` edits one part of a plan.
`Plan::apply_batch(expected_revision, at, changes)` applies related edits under
one revision. Both validate the resulting graph and leave it unchanged on failure.

`SubmitForReview` requires every item to be ready. Leaf items must have at
least one acceptance criterion and gate ID. Submission locks further edits
until `ReturnToDraft`; **it is not an approval by an authenticated reviewer**.

`Plan::from_json` validates imported JSON and migrates version 1 to version 2
while preserving ID and revision. Unknown fields and oversized plans are rejected.

## Checks

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
```

These check the data model, not server-side security enforcement.
