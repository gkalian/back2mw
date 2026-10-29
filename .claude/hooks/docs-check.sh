#!/usr/bin/env bash
# Stop hook: if code or data changed but the documentation in both languages did not,
# asks Claude to run the sync-docs skill. Reminds once per
# state of the changes, so as not to repeat every turn.
set -u

input=$(cat)
# Already continuing after the hook's reminder — do not loop.
case "$input" in *'"stop_hook_active":true'* | *'"stop_hook_active": true'*) exit 0 ;; esac

cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
git rev-parse --git-dir >/dev/null 2>&1 || exit 0

code='crates/ data/ docs/notes_en.tsv'
changed=$(git status --porcelain -- $code 2>/dev/null)
[ -z "$changed" ] && exit 0

ru=$(git status --porcelain -- docs/ru 2>/dev/null)
en=$(git status --porcelain -- docs/en 2>/dev/null)
# Both language versions are already touched — assume the skill has run.
[ -n "$ru" ] && [ -n "$en" ] && exit 0

# Remind about the same state of changes only once.
state=$( { git status --porcelain -- $code docs; git diff -- $code docs; git diff --cached -- $code docs; } 2>/dev/null | cksum)
marker="$(git rev-parse --git-dir)/sync-docs-seen"
[ "$(cat "$marker" 2>/dev/null)" = "$state" ] && exit 0
echo "$state" >"$marker"

echo '{"decision":"block","reason":"crates/ or data/ have changes, but not both docs/ru and docs/en were updated. Run the sync-docs skill. If the change does not affect behavior for the user (refactoring, tests), say so briefly and change nothing."}'
exit 0
