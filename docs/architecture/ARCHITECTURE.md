<div id="top"></div>

# Repository architecture

## Contents

- [Current repository](#current-repository)
- [Website separation](#website-separation)
- [What can be verified](#what-can-be-verified)

[Documentation index](../INDEX.md)

**Audience:** Developers and maintainers inspecting the repository as it exists.

## Current repository

The tracked Taiyaki repository currently contains a static concept website,
project images, Markdown documentation, and workflow configuration. No
executable Taiyaki service or MCP implementation is tracked.

The public website depicts intended principles; it does not demonstrate that
phase-based tool access, server-enforced transitions, or receipt verification
are available. Those behaviors must be implemented and tested before
describing them as product capabilities.

## Website separation

```text
site/
  index.html
  404.html
  assets/
    css/style.css
    js/main.js
    fonts/
    images/
docs/
  INDEX.md
  architecture/ARCHITECTURE.md
scripts/
  check-site.sh
```

`site/` is the prepared standalone static website source, following the
site-versus-docs split used by Jelly. `docs/` is for Markdown documentation.
Existing static files in `docs/` are retained temporarily because the
currently configured GitHub Pages publishing source has not been independently
verified; removing them could break the existing public URL.

The proposed `.github/workflows/pages.yml` is explicitly manual-only.
Its presence does not mean GitHub Pages has been reconfigured or deployed.

## What can be verified

Run from the repository root:

```sh
bash scripts/check-site.sh
```

This checks the prepared static site's local references and JavaScript syntax
when Node.js is installed. It does not build a server, start processes, prove
security boundaries, test the live website, or deploy GitHub Pages.

The repository's `.github/workflows/ci.yml` runs only the static-site check;
it does not exercise any Rust server or security invariants.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>
