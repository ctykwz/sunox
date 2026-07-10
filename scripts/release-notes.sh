#!/usr/bin/env bash
set -euo pipefail

tag="${1:?usage: release-notes.sh <tag>}"
version="${tag#v}"

notes="$({
  awk -v heading="## [${version}]" '
    index($0, heading) == 1 { found = 1; next }
    found && /^## \[/ { exit }
    found { print }
  ' CHANGELOG.md
} | sed -e '/./,$!d')"

if [[ -z "${notes}" ]]; then
  echo "CHANGELOG.md has no release section for ${version}" >&2
  exit 1
fi

printf '%s\n' "${notes}"
