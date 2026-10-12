<div id="top"></div>

# Testing strategy and evidence (design proposal)

## Table of Contents

- [Status and source material](#status-and-source-material)
- [Test model](#test-model)
- [Scope and execution boundaries](#scope-and-execution-boundaries)
- [First acceptance contracts](#first-acceptance-contracts)
- [Adapter and isolation matrix](#adapter-and-isolation-matrix)
- [Governance and test-run evidence](#governance-and-test-run-evidence)
- [Development and CI sequencing](#development-and-ci-sequencing)
- [What has actually been verified](#what-has-actually-been-verified)

[Documentation index](../INDEX.md) | [Architecture](../architecture/ARCHITECTURE.md) | [Deployment](../reference/DEPLOYMENT.md)

**Audience:** Developers, maintainers and security reviewers building and validating Taiyaki's gated completion authority.

**Status:** Proposed test strategy, **not a claim of implemented tests**. No commands, scenario IDs, directories or helpers introduced below are available merely because they appear in this document.

## Status and source material

This proposal is grounded in **existing Jelly source files**, not just its documentation:

- `/data/github/jelly/tests/INDEX.md`: two execution layers (Rust and scenario runner), named groups/batches and test run artifacts.
- `/data/github/jelly/tests/TAXONOMY.md`: separation of test contracts, suites and acceptance signoff; governance against duplicate tests.
- `/data/github/jelly/tests/suite/groups/agent-api.sh` and `tests/suite/lib.sh`: `jt_register` with stable IDs, preconditions, input, expected output and executable assertion functions.
- `/data/github/jelly/tests/suite/run.sh` and `tests/suite/README.md`: catalog, selection, locking, logs and machine-readable results.
- `/data/github/jelly/scripts/tests/fcis-isolated.sh`: copies the working tree into an isolated sandbox, omits `.env`, builds separate binaries, guards cleanup and does not touch installed services.
- `/data/github/jelly/.github/workflows/ci.yml` and `scripts/dev.sh`: one canonical quality command plus one Rust test command, without rerunning the same tests under several labels.
- `/data/github/jelly/tests/acceptance/criteria.tsv` and `links.tsv`: product-facing criteria and evidence links with signoff independent from test PASS.

These are **patterns to adapt**, not Jelly scripts to copy or execute on Taiyaki. Jelly's browser/CDP fixtures, browser lifecycle, configured systemd units, `test-jelly` Cargo frontend and large shell registry are specific to Jelly. In particular, Jelly's scenario taxonomy currently labels its migrated cases provisional; a source-level review of test code is **not runtime proof** of a successful scenario.

Taiyaki's current tracked repository has no executable core. Its local untracked Rust scaffold includes seven basic constructor tests (observed previously), not enforcement tests. The published CI workflow is a placeholder. The private source scaffold is intentionally left untouched.

## Test model

Keep these three entities **separate**:

| Entity | Contract | Initial Taiyaki implementation |
| --- | --- | --- |
| **Test case** | Executable stimulus, boundary, assertion, preconditions and evidence | Rust `#[test]` for pure policy; Rust integration tests for real server/worker; optional numbered scenarios if more than one runtime topology must be selected |
| **Test suite** | Selection of existing executable cases (no duplicated assertion bodies) | Initially `cargo test` filters and standard Cargo targets; named suites only when they are necessary |
| **Acceptance criterion** | Product requirement with named owner and explicit signoff | Reviewable I1–I4 criteria mapped to executed test results; a passing test does **not** automatically mean acceptance |

The goal is **independent failure boundaries**, not a high test count. One scenario can assert multiple necessary observations for one observable contract. Do not duplicate a pure transition assertion in a shell test unless the latter exercises a new, actual process or authorization boundary.

## Scope and execution boundaries

| Layer | Real boundary | Examples | Environment and side effects |
| --- | --- | --- | --- |
| Unit | Pure Rust lifecycle policy without I/O | Phase transition table, allowed tools, receipt/digest validation rules, stale revision handling | Small, deterministic, no process or network |
| Component | A single adapter or persistence component with controlled inputs | Working-tree digest test vectors, persisted log round-trip, gate-command argument validation, atomic store failure | Temporary directory, no installed service |
| System | **Real** authority + constrained verification worker + filesystem | Unix-socket denial, real gate execution, server-only receipts, restart recovery, race/freshness checks | Dedicated identities and disposable state; never a shared installed Taiyaki instance |
| Cross-system | Independent remote boundary | SSH host-key/identity forwarding, remote workspace selection, rootless Docker/Podman UID/volume isolation | Explicit opt-in on prepared hosts; no automatic remote or container startup |

`system` is a scope boundary, not a synonym for the number of source files.
`smoke` is a purpose/selection; `E2E` describes complete execution through
a chosen boundary; `UAT` is human acceptance; none replaces evidence of
the exact actual environment.

For nondeterministic risks, use bounded timeouts, controlled fixtures and
fault injection where possible. Record seed/platform/runtime settings when
they materially affect outcomes. No real host/engine security property can
be certified using a mocked CLI invocation alone.

## First acceptance contracts

**Proposed IDs below are not registered executable tests.** Assign permanent
IDs when implementing distinct cases, after searching existing test coverage.

| Proposed case | Invariant | Scope | Stimulus and observable assertion |
| --- | --- | --- | --- |
| `GATE-001` | I1 | Unit | For every operational/terminal state, deny out-of-phase operations and leave state unchanged. |
| `GATE-002` | I1 | System | Discover a tool while allowed, change phase, then invoke its cached name through the real MCP/API boundary; reject the call even if discovery remains stale. |
| `DONE-001` | I2 | Unit | Refuse `done` with missing/failed/stale receipt, mismatched tree digest or plan revision; accept only complete current PASS evidence. |
| `DONE-002` | I2 | System | Execute an approved PASS gate on a real workspace, modify a tracked or nonignored untracked file, then attempt/currently query `done`; completion is refused or invalidated, with durable provenance. |
| `HASH-001` | I2 | Component | Compare frozen, reproducible digest vectors; changing relevant content, path or executable bit changes the result. Ignore only the explicitly documented file set. |
| `EXEC-001` | I3 | System | Send a forged PASS receipt and fake stdout to the public interface; neither creates persisted proof nor advances completion. |
| `EXEC-002` | I3 | System | Run a real approved gate as the restricted worker; nonzero exit, timeout and source-tree change cannot produce PASS. Attempt to write protected state from the worker and assert OS denial. |
| `AUDIT-001` | I4 | Component/system | Execute authorized transitions, restart the authority, inspect persisted order, authenticated actor, UTC time, from/to states, plan revision and receipt references. |
| `RACE-001` | I2, I4 | System | Mutate workspace while a gate is running or before `done` is committed; no accepted stale receipt and no unlogged transition. |

**Acceptance criteria** should name the owner, requirement, evidence case
IDs, and decision (`proposed`, `accepted`, `rejected`). A test that was
not actually run provides *planned coverage*, not evidence.

Before `HASH-001` can be implemented, freeze `taiyaki-tree-v1` field
encoding and empty-tree/multi-file vectors in the architecture specification;
the digest proposal currently lacks these implementation-ready details.

## Adapter and isolation matrix

| Scenario | Scope | Required proof | What a passing result cannot imply |
| --- | --- | --- | --- |
| Unix local | System | Disallowed UID cannot connect; allowed client cannot write authority state; real gate runs under restricted worker | No guarantee of SSH authentication or container security |
| SSH-forwarded Unix | Cross-system | Host key validation, explicit principal-to-actor mapping, remote workspace identity, broken-tunnel failure, no fabricated PASS | An SSH connection alone does not prove original MCP caller identity |
| Containerized Taiyaki | Cross-system | Observed UID maps, protected state volume, approved project mount, no agent access to privileged engine socket, gate-worker isolation | `docker ps` success is not a security attestation |
| SSH to remote container | Cross-system | Combined transport identity and container workspace binding; origin and verifier trees consistent | Separate passing SSH and container tests alone do not prove their composition |

Run remote/container cases only with authorized disposable resources. A
test harness **must never** start/stop another user's systemd services,
delete an existing volume, or reset their Git worktree. Any cleanup acts
only on resources it created, after verifying resource identity.

## Governance and test-run evidence

Before creating or changing a test:

1. State the **observable contract**: input, system boundary, expected output,
   security regression it would catch and relevant acceptance criterion.
2. Search existing Rust inline tests, integration tests, registered scenarios,
   and fixtures for the **same contract at the same boundary**.
3. Prefer extending a parameterized case or shared fixture over adding a
   duplicate runner. Add another case only if it exercises a genuinely
   separate failure boundary (policy vs real socket vs real SSH/container).
4. Declare prerequisites, operating principal, isolated state/workspace,
   required network/runtime, side effects, cleanup strategy and expected
   failure behavior **before executing** any system test.
5. Run only the smallest pertinent test set during development. Never run a
   whole installed-service suite by default. Record skipped/unverified
   environments explicitly rather than counting them as PASS.

When the scenario catalog becomes useful, follow Jelly's single-source
principle: stable scenario ID and executable implementation in one registry;
classification and suite selection are references to IDs, not copies of test
bodies. Do **not** introduce Jelly's 91-case TSV system or custom
`cargo test-jelly` frontend into this initial scaffold.

**Planned evidence per actual run:** unique run ID, repository commit and
dirty-tree marker/digest, test ID/name, command, actor, environment identity,
start/finish timestamps, outcome (`PASS`, `FAIL`, `SKIP`, `UNVERIFIED`),
duration, relevant stdout/stderr logs and receipt IDs. Store logs outside
the repository, except stable sanitised fixtures or intentional committed
regression evidence. Provenance may include a host/container image digest
where such data is observable. Redact secrets and never record private keys.

Evidence should include **negative assertions**: expected rejection *and*
no mutation of server state, receipts or transition history. A Rust mock
cannot prove Unix ACLs or verifier separation; those require distinct
real OS principals.

## Development and CI sequencing

**Stage 0 (current):** write test contracts, freeze architecture decisions,
preserve the pre-existing untracked Rust scaffold. Do not replace the
placeholder CI with a fictitious all-green security gate.

**Stage 1 (when Rust core is tracked):** start with focused Rust tests and
normal toolchain commands, not a custom wrapper:

```sh
cargo test --locked --lib <filter>          # replace <filter> with real test name
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

Commands above are **future developer examples**, not evidence that a
particular Taiyaki test or command wrapper exists now. Only run the broad
`--all-targets` suite when appropriate (e.g. before release/merge), not
after every documentation or single-function change.

**Stage 2:** add deterministic integration tests that spawn a disposable
authority and verification worker with separated identities, a temporary
Git workspace and protected state directory. Guard the harness so it
refuses an installation config, active runtime path or shared service.
Provisioning privileges and any install/service startup require operator
authorization. A test-only fake identity is **not** enough for I3.

**Stage 3:** add opt-in SSH/container compatibility runs and a small,
nonduplicative selection mechanism; independent acceptance review may then
use their saved evidence.

As in Jelly, CI should have **one canonical static/quality check stage and
one Rust test stage**, without repeating those same tests inside a separate
`quality` scenario suite. Add a real CI pipeline only after the tracked
Cargo project and checks are available. Keep privileged deployment/security
probes out of ordinary untrusted pull-request CI.

## What has actually been verified

| Item | Status at documentation time |
| --- | --- |
| Jelly test harness layout, Rust/CI command sources and example test registrations | Inspected as source |
| Taiyaki scenario catalog or process-isolation harness | Not implemented |
| Taiyaki I1–I4 tests listed above | **Not implemented and not executed** |
| Local, SSH, Docker/Podman authority/worker isolation | Not tested |
| Acceptance owner signoff | Not requested or granted |
| Taiyaki Rust constructor tests | Seven pass from prior local audit; unrelated to I1–I4 |
| Taiyaki CI | Placeholder only |

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>
