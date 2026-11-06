#!/usr/bin/env bash
# Extract one version's section from CHANGELOG.md.
#
#   scripts/ci/changelog_section.sh <version> [output]   # e.g. 0.1.4
#
# Writes the body of `## [<version>]`, up to the next `## [`, to `output`
# (default /tmp/changelog-section.md) and echoes it. Text after the closing
# bracket is free, so `- 2026-11-06` and an em dash date both match. Exits
# non-zero if CHANGELOG.md is missing, or the section is absent or empty.
#
# release-validate.yml calls this as the release gate. release.yml calls it to
# build the GitHub Release body.
#
# Pass the BASE version: a `v0.1.4-beta.1` tag reads the `[0.1.4]` section.

set -euo pipefail

VERSION="${1:?usage: changelog_section.sh <version> [output]}"
OUTPUT="${2:-/tmp/changelog-section.md}"

if [[ ! -f CHANGELOG.md ]]; then
    echo "::error::CHANGELOG.md is missing in $PWD. Create it, with a '## [$VERSION]' section."
    exit 1
fi

# Trailing blank lines and any `---` rule between sections stay out of the notes.
awk -v heading="## [$VERSION]" '
    index($0, heading) == 1 { found = 1; next }
    found && /^## \[/ { exit }
    found { lines[n++] = $0 }
    END {
        while (n > 0 && (lines[n - 1] ~ /^[[:space:]]*$/ || lines[n - 1] ~ /^-{3,}[[:space:]]*$/)) {
            n--
        }
        for (i = 0; i < n; i++) print lines[i]
    }
' CHANGELOG.md > "$OUTPUT"

if [[ ! -s "$OUTPUT" ]]; then
    echo "::error::CHANGELOG.md has no non-empty '## [$VERSION]' section. Rename the Unreleased heading to [$VERSION] and describe the release."
    exit 1
fi

echo "Changelog section for [$VERSION]:"
cat "$OUTPUT"
