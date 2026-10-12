<div id="top"></div>

<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="./assets/dark-full-logo.png">
  <source media="(prefers-color-scheme: light)" srcset="./assets/full-logo.png">
  <img src="./assets/full-logo.png" alt="Taiyaki" width="760">
</picture>

**Gated planning state for long-horizon agents.**

🐡 [**Architecture**](./docs/architecture/ARCHITECTURE.md)　•　🐡 [**Deployment**](./docs/reference/DEPLOYMENT.md)　•　🐡 [**Documentation**](./docs/INDEX.md)

🐡 [Visit the project website](https://fabioflorey.github.io/taiyaki/)

[![Stars](https://img.shields.io/github/stars/FabioFlorey/taiyaki?style=flat&label=stars)](https://github.com/FabioFlorey/taiyaki/stargazers) [![Forks](https://img.shields.io/github/forks/FabioFlorey/taiyaki?style=flat&label=forks)](https://github.com/FabioFlorey/taiyaki/forks)

</div>

## Table of Contents

- [1. Project status](#1-project-status)
- [2. Design principles](#2-design-principles)
- [3. Documentation](#3-documentation)

## 1. Project status

**Design-stage project — not a released, runnable MCP/server implementation.**
The public website explains Taiyaki's intended architecture, not functionality
already present in the tracked Rust code. Current CI is a placeholder. A local
untracked Rust scaffold provides basic data models only; it is not a published
release or executable implementation of the invariants.

## 2. Design principles

Taiyaki aims to make agent completion independently verifiable:

- **I1:** Reject a tool call outside its operational phase at the authority boundary.
- **I2:** Mark work `done` only with server-issued PASS receipts for the *current* source tree and plan revision.
- **I3:** Run approved verification gates outside the agent's control, through a restricted worker.
- **I4:** Persist transition provenance including authenticated actor, timestamp and receipt references.

Deployment and transport are **independent**: an MCP or CLI client could use
a local Unix socket, an SSH-forwarded socket to another host, or a configured
connection to Taiyaki running in a container. These are proposals, **not
available adapters today**.

## 3. Documentation

- [Documentation index](./docs/INDEX.md) — status and navigation.
- [Architecture (MVP RFC)](./docs/architecture/ARCHITECTURE.md) — states,
  receipt model, invariants and security boundaries.
- [Deployment and connection adapters](./docs/reference/DEPLOYMENT.md) —
  local/SSH/container topologies, trade-offs and planned verification tests.

No installation, MCP endpoint or quickstart is advertised until executable
integration tests establish that it works.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>
