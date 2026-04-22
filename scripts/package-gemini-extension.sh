#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <version>"
  echo "Example: $0 v0.2.0"
  exit 1
fi

VERSION="$1"
VERSION_NO_V="${VERSION#v}"

MANIFEST_VERSION=$(jq -r '.version' gemini-extension/gemini-extension.json)

if [[ "$VERSION_NO_V" != "$MANIFEST_VERSION" ]]; then
  echo "Warning: argument version ($VERSION_NO_V) does not match manifest version ($MANIFEST_VERSION)"
  echo "Continuing with manifest version ($MANIFEST_VERSION) as source of truth"
fi

ARCHIVE="fluxmirror-gemini-extension-${VERSION}.tar.gz"

tar czf "$ARCHIVE" -C gemini-extension .

tar tzf "$ARCHIVE" | grep -q '^\./gemini-extension.json$' || {
  echo "Archive structure invalid"
  exit 1
}

echo "Archive created: ${ARCHIVE}"
echo ""
echo "Next steps for manual release:"
echo "  1. git tag ${VERSION} && git push origin ${VERSION}"
echo "  2. https://github.com/OpenFluxGate/fluxmirror/releases/new"
echo "  3. Select tag ${VERSION}, attach the .tar.gz, mark as latest, Publish"
echo ""
echo "Or use GitHub Actions: just push the tag (.github/workflows/release.yml handles the rest)."
