#!/bin/bash
set -euo pipefail

VERSION="${1:?Usage: ./scripts/release.sh <version> (e.g. 0.3.0)}"

# Strip leading 'v' if provided
VERSION="${VERSION#v}"

TAG="v${VERSION}"

git pull

# Check for clean working tree
if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "Error: Working tree is not clean. Commit or stash changes first."
  exit 1
fi

# Check if tag already exists
if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "Error: Tag $TAG already exists"
  exit 1
fi

echo "Releasing $VERSION..."

echo "→ Ensuring cargo-edit is installed (force avoids a shadowing cargo-set-version binary)"
cargo install cargo-edit --force --locked

echo "→ Updating Cargo.toml versions to $VERSION"
cargo set-version "$VERSION"

echo "→ Updating Python package version to $VERSION"
python - "$VERSION" <<'PY'
from __future__ import annotations

import re
import sys
from pathlib import Path

version = sys.argv[1]
root = Path.cwd()


def replace_once(path: Path, pattern: str, replacement: str) -> None:
    text = path.read_text(encoding="utf-8")
    text, count = re.subn(pattern, replacement, text, count=1)
    if count != 1:
        raise SystemExit(f"failed to update version in {path}")
    path.write_text(text, encoding="utf-8")


replace_once(
    root / "packages/fff-python/pyproject.toml",
    r'(?m)^version = "[^"]+"$',
    f'version = "{version}"',
)
replace_once(
    root / "packages/fff-python/src/fff/__init__.py",
    r'(?m)^__version__ = "[^"]+"$',
    f'__version__ = "{version}"',
)

lock_path = root / "packages/fff-python/uv.lock"
if lock_path.exists():
    text = lock_path.read_text(encoding="utf-8")
    marker = '[[package]]\nname = "fff-search"\nversion = "'
    start = text.find(marker)
    if start == -1:
        raise SystemExit(f"failed to find fff-search package in {lock_path}")
    version_start = start + len(marker)
    version_end = text.find('"', version_start)
    if version_end == -1:
        raise SystemExit(f"failed to find fff-search version end in {lock_path}")
    text = text[:version_start] + version + text[version_end:]
    lock_path.write_text(text, encoding="utf-8")
PY

git add -A
git commit -m "chore: release $VERSION"

echo "→ Creating tag $TAG"
git tag -a "$TAG" -m "Release $VERSION"

echo "→ Pushing to origin"
git push origin
git push origin "$TAG"

echo ""
echo "Release $VERSION created and pushed."
echo "CI will build and publish: https://github.com/dmtrKovalenko/fff.nvim/actions"
