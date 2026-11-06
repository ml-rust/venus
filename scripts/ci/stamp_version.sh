#!/usr/bin/env bash
# Stamp the workspace version from a release tag.
#
#   scripts/ci/stamp_version.sh <version>       # e.g. 0.1.4, 0.1.4-beta.2
#
# Rewrites `[workspace.package] version` in the root Cargo.toml. Every member
# inherits it through `version.workspace = true`.
# A prerelease also pins every `venus*` workspace dependency with `=`, because
# a prerelease satisfies no requirement on its base version.
#
# Only a prerelease tag changes anything. release-validate.yml checks the tag's
# base version against the manifest, so `v0.1.4` is a no-op and
# `v0.1.4-beta.2` appends the suffix. A re-run is idempotent.

set -euo pipefail

VERSION="${1:?usage: stamp_version.sh <version>}"

CURRENT=$(cargo metadata --no-deps --format-version=1 \
    | jq -r '.packages[] | select(.name == "venus") | .version')

if [[ "$VERSION" == "$CURRENT" ]]; then
    echo "Version already $VERSION, nothing to stamp."
    exit 0
fi

perl -i -pe 'if (/^\[/) { $in = /^\[workspace\.package\]/ } if ($in && !$done && s/^version = ".*"/version = "'"$VERSION"'"/) { $done = 1 }' Cargo.toml

if [[ "$VERSION" == *-* ]]; then
    perl -i -pe 's/^(venus(?:-(?:macros|core|sync|server))? = \{ path = "[^"]*", version = )"[^"]*"/$1"='"$VERSION"'"/' Cargo.toml
    echo "Pinned internal dependencies to =$VERSION"
fi

echo "Stamped workspace: $CURRENT -> $VERSION"
