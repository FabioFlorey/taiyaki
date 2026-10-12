<div id="top"></div>

# Taiyaki Documentation

## Table of Contents

- [Current status](#current-status)
- [Architecture](#architecture)
- [Reference](#reference)
- [Getting started and development](#getting-started-and-development)

**Audience:** Maintainers, potential adopters, and reviewers distinguishing the product design from implemented software.

**Using this documentation:** Architecture records proposed invariants and
design decisions; reference explains connection/deployment choices and their
security constraints. Getting-started instructions will be published only
after a tested server exists.

## Current status

Taiyaki is at **architecture/RFC stage**. The public website describes the
intended gated verification product. The tracked repository does not yet
include a working Taiyaki server, MCP adapter, state machine, gate runner or
PASS receipts. A local untracked Rust scaffold is not a published release.

## Architecture

- [Architecture (MVP RFC)](./architecture/ARCHITECTURE.md): Threat model,
  state transitions, invariant contracts, working-tree digest and evidence
  requirements.

## Reference

- [Deployment and connection adapters](./reference/DEPLOYMENT.md): Local
  Unix socket, remote SSH and container placements; authority identities,
  transport boundaries, trade-offs and acceptance tests.

## Getting started and development

- [Testing strategy and evidence](./development/TESTING.md): Jelly-inspired
  test governance, isolated Rust/process verification, I1–I4 acceptance
  contracts and future CI sequencing.

No runnable Taiyaki installation or MCP quickstart is available yet.
The repository's existing CI job is a placeholder rather than an
end-to-end implementation check. Executable guides and development
checks will be added after the core and adapters have been implemented.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>
