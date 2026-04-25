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
  .claude-plugin/marketplace.json
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
    plugins/fluxmirror/.claude-plugin/plugin.json
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
  fi
fi

if [ "$dry" -eq 1 ]; then
  echo ""
  echo "Dry run only. No commit, no tag."
  exit 0
fi

git add \
  gemini-extension/gemini-extension.json \
  plugins/fluxmirror/.claude-plugin/plugin.json \
  .claude-plugin/marketplace.json

git commit -m "chore: bump version to ${v}"
git tag -a "$tag" -m "${tag}"

echo ""
echo "Done."
echo ""
echo "Now push to trigger the release workflows:"
echo "  git push origin main $tag"
