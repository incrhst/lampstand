#!/usr/bin/env bash
# Puts a release's "What's new" section into its latest.json, so installed
# copies can show what an update brings before the operator installs it.
#
# The notes are written on the draft release after the build, so latest.json
# (made by the build) has none. Usage: release-notes.sh <tag>
# Needs the gh CLI (GH_TOKEN set) and jq.
set -euo pipefail

tag="${1:?usage: release-notes.sh <tag>}"
dir=$(mktemp -d)

# From the "**What's new ..." line to the end, without that line, as plain
# text: the app shows it as it is, so no Markdown bold or link syntax.
gh release view "$tag" --json body --jq .body | tr -d '\r' | awk '
  /^\*\*What.s new/ { on = 1; next }
  on && (started || NF) { started = 1; print }' | sed -E 's/\*\*//g; s/\[([^]]*)\]\([^)]*\)/\1/g' > "$dir/notes.txt"

if [ ! -s "$dir/notes.txt" ]; then
  echo "::warning::$tag has no \"What's new\" section, so latest.json keeps empty notes."
  exit 0
fi

gh release download "$tag" --pattern latest.json --dir "$dir" --clobber
jq --rawfile n "$dir/notes.txt" '.notes = ($n | sub("\\s+$"; ""))' "$dir/latest.json" > "$dir/latest.new.json"
mv "$dir/latest.new.json" "$dir/latest.json"
gh release upload "$tag" "$dir/latest.json" --clobber
echo "latest.json for $tag now carries:"
jq -r .notes "$dir/latest.json"
