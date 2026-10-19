#!/usr/bin/env bash
# Хук Stop: если менялись код или данные, а документация на обоих языках — нет,
# просит Claude выполнить скилл sync-docs. Напоминает один раз на каждое
# состояние изменений, чтобы не повторяться каждый ход.
set -u

input=$(cat)
# Уже продолжаем после напоминания хука — не зацикливаемся.
case "$input" in *'"stop_hook_active":true'* | *'"stop_hook_active": true'*) exit 0 ;; esac

cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
git rev-parse --git-dir >/dev/null 2>&1 || exit 0

code='crates/ data/ docs/notes_en.tsv'
changed=$(git status --porcelain -- $code 2>/dev/null)
[ -z "$changed" ] && exit 0

ru=$(git status --porcelain -- docs/ru 2>/dev/null)
en=$(git status --porcelain -- docs/en 2>/dev/null)
# Обе языковые версии уже тронуты — считаем, что скилл отработал.
[ -n "$ru" ] && [ -n "$en" ] && exit 0

# Одно и то же состояние изменений напоминаем только раз.
state=$( { git status --porcelain -- $code docs; git diff -- $code docs; git diff --cached -- $code docs; } 2>/dev/null | cksum)
marker="$(git rev-parse --git-dir)/sync-docs-seen"
[ "$(cat "$marker" 2>/dev/null)" = "$state" ] && exit 0
echo "$state" >"$marker"

echo '{"decision":"block","reason":"В crates/ или data/ есть изменения, а docs/ru и docs/en обновлены не оба. Выполни скилл sync-docs. Если изменение не влияет на поведение для пользователя (рефакторинг, тесты) — коротко скажи об этом и ничего не правь."}'
exit 0
