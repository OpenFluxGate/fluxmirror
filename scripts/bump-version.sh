#!/bin/bash
# Bump the project version in all release manifests, commit the change,
# and create an annotated tag. Push triggers the release workflows.
#
# Usage:
#   ./scripts/bump-version.sh 0.5.1
#   ./scripts/bump-version.sh 0.5.1 --dry-run   # show what would change, no commit
#
# Why this exists:
#   release.yml *can* sync versions in-memory at build time, but the repo
#   itself stays at the previous version, so any tool that reads manifests
#   from the repo (qwen marketplace install, plugin browsers) sees a
#   stale value. This script makes the repo the source of truth.

set -euo pipefail

if [ $# -lt 1 ]; then
  echo "Usage: $0 <version> [--dry-run]" >&2
  exit 2
fi

v="$1"
dry=0
[ "${2:-}" = "--dry-run" ] && dry=1

if ! [[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
  echo "Refusing: '$v' is not a semver (e.g. 0.5.1 or 0.5.1-rc.1)" >&2
  exit 2
fi

tag="v${v}"
if git rev-parse "$tag" >/dev/null 2>&1; then
  echo "Refusing: tag '$tag' already exists" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

# Only the manifests we will actually touch matter. Don't block on
# untracked local-state files (.omc/, .bkit/, build outputs, etc.).
manifest_files=(
  gemini-extension/gemini-extension.json
  plugins/fluxmirror/.claude-plugin/plugin.json
  plugins/fluxmirror/qwen-extension.json
  .claude-plugin/marketplace.json
  Cargo.toml
  Cargo.lock
)

if [ "$dry" -eq 0 ]; then
  if [ -n "$(git status --porcelain -- "${manifest_files[@]}" 2>/dev/null)" ]; then
    echo "Refusing: one of the version manifests has uncommitted changes" >&2
    git status --short -- "${manifest_files[@]}" >&2
    exit 2
  fi

  if [ "$(git symbolic-ref --short HEAD)" != "main" ]; then
    echo "Refusing: not on main (current: $(git symbolic-ref --short HEAD))" >&2
    exit 2
  fi
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

# Top-level .version field
for f in \
    gemini-extension/gemini-extension.json \
    plugins/fluxmirror/.claude-plugin/plugin.json \
    plugins/fluxmirror/qwen-extension.json
do
  if [ -f "$f" ]; then
    jq --arg v "$v" '.version = $v' "$f" > "$tmp"
    if [ "$dry" -eq 1 ]; then
      diff -u "$f" "$tmp" || true
    else
      mv "$tmp" "$f"
      echo "  synced $f -> $v"
      tmp=$(mktemp)
    fi
  fi
done

# marketplace.json: nested .plugins[].version
mp=.claude-plugin/marketplace.json
if [ -f "$mp" ]; then
  jq --arg v "$v" '.plugins |= map(.version = $v)' "$mp" > "$tmp"
  if [ "$dry" -eq 1 ]; then
    diff -u "$mp" "$tmp" || true
  else
    mv "$tmp" "$mp"
    echo "  synced $mp (nested) -> $v"
    tmp=$(mktemp)
  fi
fi

# Workspace Cargo.toml: rewrite the version line inside [workspace.package].
# bash 3.2-safe (macOS default): no associative arrays, no GNU sed quirks.
ws_cargo="Cargo.toml"
if [ -f "$ws_cargo" ]; then
  awk -v new="$v" '
    BEGIN { in_pkg = 0 }
    /^\[workspace\.package\][[:space:]]*$/ { in_pkg = 1; print; next }
    /^\[/ && !/^\[workspace\.package\][[:space:]]*$/ { in_pkg = 0 }
    in_pkg && /^version[[:space:]]*=/ { print "version = \"" new "\""; next }
    { print }
  ' "$ws_cargo" > "$tmp"
  if [ "$dry" -eq 1 ]; then
    diff -u "$ws_cargo" "$tmp" || true
  else
    mv "$tmp" "$ws_cargo"
    echo "  synced $ws_cargo -> $v"
    tmp=$(mktemp)
  fi
fi

# Refresh Cargo.lock so the workspace member versions match.  We try a
# plain build first (offline if possible) and let cargo rewrite the lock
# file.  Failures are tolerated — `cargo build` on the next CI run will
# regenerate it correctly anyway.
if [ "$dry" -eq 0 ] && command -v cargo >/dev/null 2>&1; then
  echo "  refreshing Cargo.lock (best-effort)..."
  if cargo build --workspace --offline >/dev/null 2>&1; then
    :
  elif cargo build --workspace >/dev/null 2>&1; then
    :
  else
    echo "  warning: could not auto-update Cargo.lock; commit will skip it" >&2
  fi
fi

if [ "$dry" -eq 1 ]; then
  echo ""
  echo "Dry run only. No commit, no tag."
  exit 0
fi

# Stage everything we synced.  Cargo.lock is added only if it actually
# changed (some bump scenarios — e.g. patch-only — leave it untouched).
git add \
  gemini-extension/gemini-extension.json \
  plugins/fluxmirror/.claude-plugin/plugin.json \
  plugins/fluxmirror/qwen-extension.json \
  .claude-plugin/marketplace.json \
  Cargo.toml
if [ -n "$(git status --porcelain -- Cargo.lock 2>/dev/null)" ]; then
  git add Cargo.lock
fi

git commit -m "chore: bump version to ${v}"
git tag -a "$tag" -m "${tag}"

echo ""
echo "Done."
echo ""
echo "Now push to trigger the release workflows:"
echo "  git push origin main $tag"
