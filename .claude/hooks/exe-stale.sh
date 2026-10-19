#!/usr/bin/env bash
# Хук Stop: если исходники новее dist/back2mw.exe, просит Claude выполнить скилл rebuild-exe.
# Напоминает один раз на каждое состояние исходников — не повторяется каждый ход.
set -u

input=$(cat)
# Уже продолжаем после напоминания хука — не зацикливаемся.
case "$input" in *'"stop_hook_active":true'* | *'"stop_hook_active": true'*) exit 0 ;; esac

cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
[ -d dist ] || exit 0

exe=dist/back2mw.exe
if [ -f "$exe" ]; then
  newest=$(find crates data packaging Cargo.toml Cargo.lock -type f -newer "$exe" 2>/dev/null | head -1)
  [ -z "$newest" ] && exit 0
else
  newest="(нет $exe)"
fi

state=$(find crates data packaging Cargo.toml Cargo.lock -type f -printf '%p %T@\n' 2>/dev/null | sort | cksum)
marker=".claude/.exe-stale-seen"
[ "$(cat "$marker" 2>/dev/null)" = "$state" ] && exit 0
echo "$state" >"$marker"

echo '{"decision":"block","reason":"Исходники новее dist/back2mw.exe (например, '"$newest"'). Выполни скилл rebuild-exe: пересобери оба exe и обнови dist/. Если менялись только комментарии или тесты и exe не изменится — всё равно пересобери."}'
exit 0
