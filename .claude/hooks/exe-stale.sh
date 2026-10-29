#!/usr/bin/env bash
# Stop hook: if the sources are newer than dist/back2mw.exe, asks Claude to run the rebuild-exe skill.
# Reminds once per state of the sources — does not repeat every turn.
set -u

input=$(cat)
# Already continuing after the hook's reminder — do not loop.
case "$input" in *'"stop_hook_active":true'* | *'"stop_hook_active": true'*) exit 0 ;; esac

cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
[ -d dist ] || exit 0

exe=dist/back2mw.exe
if [ -f "$exe" ]; then
  newest=$(find crates data packaging Cargo.toml Cargo.lock -type f -newer "$exe" 2>/dev/null | head -1)
  [ -z "$newest" ] && exit 0
else
  newest="(no $exe)"
fi

state=$(find crates data packaging Cargo.toml Cargo.lock -type f -printf '%p %T@\n' 2>/dev/null | sort | cksum)
marker=".claude/.exe-stale-seen"
[ "$(cat "$marker" 2>/dev/null)" = "$state" ] && exit 0
echo "$state" >"$marker"

echo '{"decision":"block","reason":"The sources are newer than dist/back2mw.exe (for example, '"$newest"'). Run the rebuild-exe skill: rebuild both exes and update dist/. If only comments or tests changed and the exe will not change, rebuild anyway."}'
exit 0
