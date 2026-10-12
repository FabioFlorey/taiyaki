#!/usr/bin/env bash
# Validate the one-and-only static website source used by GitHub Pages.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SITE="$ROOT/site"
checked=0

# Guard against accidentally reviving the retired GitHub Pages copy in docs/.
for legacy in docs/index.html docs/site.css docs/site.js docs/assets docs/fonts; do
  if [[ -e "$ROOT/$legacy" ]]; then
    printf 'Duplicate website found outside site/: %s\n' "$legacy" >&2
    exit 1
  fi
done

for required in index.html 404.html .nojekyll robots.txt sitemap.xml assets/css/style.css assets/js/main.js; do
  [[ -f "$SITE/$required" ]] || { printf 'Missing site file: %s\n' "$required" >&2; exit 1; }
done

check_ref() {
  local origin="$1" ref="$2" target
  ref="${ref%%\?*}"
  ref="${ref%%#*}"
  [[ -n "$ref" ]] || return 0
  case "$ref" in
    http:*|https:*|mailto:*|data:*|javascript:*) return 0 ;;
    /taiyaki/*) target="$SITE/${ref#/taiyaki/}" ;;
    /*) printf 'Unexpected absolute path in %s: %s\n' "$origin" "$ref" >&2; return 1 ;;
    *) target="$(dirname "$origin")/$ref" ;;
  esac
  target="$(realpath -m -- "$target")"
  case "$target" in
    "$SITE"|"$SITE"/*) ;;
    *) printf 'Path escapes site root: %s -> %s\n' "$origin" "$ref" >&2; return 1 ;;
  esac
  [[ -f "$target" || -d "$target" ]] || {
    printf 'Broken site reference: %s -> %s\n' "$origin" "$ref" >&2
    return 1
  }
  checked=$((checked + 1))
}

while IFS= read -r -d '' file; do
  while IFS= read -r ref; do
    ref="${ref#*=\"}"
    ref="${ref%\"}"
    check_ref "$file" "$ref"
  done < <(grep -oE '(href|src)="[^"]+"' "$file" || true)
done < <(find "$SITE" -type f -name '*.html' -print0)

while IFS= read -r -d '' file; do
  while IFS= read -r ref; do
    ref="${ref#url(}"
    ref="${ref%)}"
    ref="${ref#\"}"
    ref="${ref%\"}"
    ref="${ref#\'}"
    ref="${ref%\'}"
    check_ref "$file" "$ref"
  done < <(grep -oE 'url\([^)]*\)' "$file" || true)
done < <(find "$SITE" -type f -name '*.css' -print0)

if find "$SITE" -type l -print -quit | grep -q .; then
  echo "Symlinks must not appear in the published site artifact" >&2
  exit 1
fi

if command -v node >/dev/null 2>&1; then
  while IFS= read -r -d '' file; do node --check "$file"; done < <(find "$SITE" -type f -name '*.js' -print0)
  node "$ROOT/scripts/tests/fish-motion.cjs"
fi

printf 'Site check PASS: %s local references resolved. No deploy performed.\n' "$checked"
