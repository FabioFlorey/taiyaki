<div id="top"></div>

<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="./assets/dark-full-logo.png">
  <source media="(prefers-color-scheme: light)" srcset="./assets/full-logo.png">
  <img src="./assets/full-logo.png" alt="Taiyaki" width="760">
</picture>

# 🐡 Taiyaki

**Gated planning state for long-horizon agents.**

*An agent can do the work. It should not be the one declaring that the work is done.*

🐡 [**Why Taiyaki**](#why-taiyaki)　•　🐡 [**How it works**](#how-it-works)　•　🐡 [**Core invariants**](#core-invariants)　•　🐡 [**Documentation**](./docs/INDEX.md)

🐡 [Visit the project website](https://fabioflorey.github.io/taiyaki/)

[![Stars](https://img.shields.io/github/stars/FabioFlorey/taiyaki?style=flat&label=stars)](https://github.com/FabioFlorey/taiyaki/stargazers) [![Forks](https://img.shields.io/github/forks/FabioFlorey/taiyaki?style=flat&label=forks)](https://github.com/FabioFlorey/taiyaki/forks)

</div>

## Why Taiyaki?

Long-running agents can lose track of their plans, skip steps, or report success before the result has been checked. A confident **"done"** is still just a claim.

Taiyaki is designed to give each work item a durable plan, explicit phases, and independent verification. The agent performs the work; Taiyaki is the authority that decides whether the evidence is sufficient to complete it.

**The goal: make completion a verified outcome, not a model opinion.**

## How it works

**Planning → Implementing → Verifying → Done**

1. **Plan.** Define the work, its subtasks, acceptance criteria, and required checks.
2. **Implement.** The agent carries out the plan within the permissions of the current phase.
3. **Verify.** Approved checks run outside the agent's control, producing evidence tied to the current work.
4. **Complete.** Taiyaki permits `Done` only when the required checks have passed and their evidence is still current.

Failed checks return the work to implementation. Unresolved work can be marked **Blocked**, rather than silently treated as complete.

## Core invariants

| | Principle | Meaning |
| --- | --- | --- |
| **I1** | Phase-scoped authority | Operations must be allowed in the current phase, even when a client cached a tool earlier. |
| **I2** | Fresh proof | Completion requires PASS receipts matching the current source tree and plan revision. |
| **I3** | Independent verification | The agent cannot write its own PASS receipts; approved checks run under Taiyaki's authority. |
| **I4** | Provenance | Transitions retain who acted, when, and which evidence justified the change. |

*Taiyaki is under development. This page describes its intended design, not a released server.*

<div align="right"><sub><a href="#top">↑ Back to top</a></sub></div>
