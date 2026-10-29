#!/usr/bin/env bash
# Release changelog: commits from the previous tag to the given one, grouped by
# semantic prefix (`feat:`, `fix(gui):`, `refactor!:` …). The version is set by the tag itself —
# commits do not affect it, the prefixes are only for grouping.
#
#   .github/scripts/changelog.sh v0.3.0 > notes.md
#
# The previous tag is the nearest one reachable from the given tag's parent; if there is none (the first release),
# all commits are taken. Merges are skipped. Commits without a prefix go to "Other".
# Commit links are added if GITHUB_REPOSITORY is set (as on GitHub Actions).
set -euo pipefail

tag="${1:?give a tag, for example v0.3.0}"

if prev=$(git describe --tags --abbrev=0 "${tag}^" 2>/dev/null); then
  range="${prev}..${tag}"
else
  prev=""
  range="${tag}"
fi

server="${GITHUB_SERVER_URL:-https://github.com}"
repo="${GITHUB_REPOSITORY:-}"

# Section order in the notes; type → heading (the notes are in English). An unknown type goes to "Other".
types=(feat fix perf refactor docs test build ci chore revert other)
declare -A title=(
  [feat]="Features"
  [fix]="Fixes"
  [perf]="Performance"
  [refactor]="Refactoring"
  [docs]="Documentation"
  [test]="Tests"
  [build]="Build"
  [ci]="CI"
  [chore]="Chores"
  [revert]="Reverts"
  [other]="Other"
)
declare -A section=()
breaking=""

# %x1f is a field separator that never occurs in a commit subject.
while IFS=$'\x1f' read -r hash short subject; do
  [[ -z "$hash" ]] && continue
  if [[ "$subject" =~ ^([a-zA-Z]+)(\(([^\)]*)\))?(!)?:[[:space:]]*(.*)$ ]]; then
    type="${BASH_REMATCH[1],,}"
    scope="${BASH_REMATCH[3]}"
    bang="${BASH_REMATCH[4]}"
    text="${BASH_REMATCH[5]}"
  else
    type="other"; scope=""; bang=""; text="$subject"
  fi
  [[ -n "${title[$type]:-}" ]] || type="other"

  line="- "
  [[ -n "$scope" ]] && line+="**${scope}:** "
  line+="${text}"
  if [[ -n "$repo" ]]; then
    line+=" ([\`${short}\`](${server}/${repo}/commit/${hash}))"
  else
    line+=" (\`${short}\`)"
  fi
  section[$type]+="${line}"$'\n'
  [[ -n "$bang" ]] && breaking+="${line}"$'\n'
done < <(git log --no-merges --reverse --format='%H%x1f%h%x1f%s' "$range")

if [[ -n "$breaking" ]]; then
  printf '## ⚠ Breaking changes\n\n%s\n' "$breaking"
fi

empty=1
for type in "${types[@]}"; do
  if [[ -n "${section[$type]:-}" ]]; then
    printf '## %s\n\n%s\n' "${title[$type]}" "${section[$type]}"
    empty=0
  fi
done
[[ $empty -eq 1 ]] && printf 'No changes.\n\n'

if [[ -n "$prev" && -n "$repo" ]]; then
  printf '**Full changelog:** %s/%s/compare/%s...%s\n' "$server" "$repo" "$prev" "$tag"
fi
