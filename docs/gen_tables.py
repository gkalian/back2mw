"""Таблицы соответствия из data/*.tsv → markdown для сайта документации (ru и en).

Страницы включают их через pymdownx.snippets (`--8<-- "effects.md"`), поэтому таблица на сайте
всегда та же, что читает конвертер. Запуск — перед `zensical build`/`serve`:

    python docs/gen_tables.py

Пишет в docs/_generated/<язык>/ (каталог в .gitignore). Пояснения в data/*.tsv — русские;
английские лежат в docs/notes_en.tsv. Нет перевода у русского пояснения — в английской таблице
остаётся русское и печатается предупреждение.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "data"
OUT = ROOT / "docs" / "_generated"
NOTES_EN = ROOT / "docs" / "notes_en.tsv"

# файл → заголовки колонок; последняя колонка (пояснение) забирает всё, что осталось в строке.
TABLES = {
    "effects": {
        "ru": ["Код Oblivion", "Эффект Morrowind", "Параметр", "Пояснение"],
        "en": ["Oblivion code", "Morrowind effect", "Parameter", "Note"],
    },
    "skills": {
        "ru": ["Навык Oblivion", "Основной навык Morrowind", "Добавочные", "Пояснение"],
        "en": ["Oblivion skill", "Main Morrowind skill", "Extra", "Note"],
    },
    "races": {
        "ru": ["Раса Oblivion", "Цель", "Донор", "Пояснение"],
        "en": ["Oblivion race", "Target", "Donor", "Note"],
    },
    "creatures": {
        "ru": ["Папка модели Oblivion", "Донор Morrowind", "Пояснение"],
        "en": ["Oblivion model folder", "Morrowind donor", "Note"],
    },
    "creature_groups": {
        "ru": ["Группа Morrowind", "Вид", "Действие Oblivion", "Стойки", "Пояснение"],
        "en": ["Morrowind group", "Kind", "Oblivion action", "Stances", "Note"],
    },
    "creature_souls": {
        "ru": ["Размер души Oblivion", "Душа Morrowind", "Пояснение"],
        "en": ["Oblivion soul size", "Morrowind soul", "Note"],
    },
    "creature_marks": {
        "ru": ["Метка Oblivion", "Метка Morrowind", "Пояснение"],
        "en": ["Oblivion mark", "Morrowind mark", "Note"],
    },
    "weather": {
        "ru": ["Погода Oblivion", "Тип Morrowind", "Пояснение"],
        "en": ["Oblivion weather", "Morrowind type", "Note"],
    },
    "voice_topics": {
        "ru": ["Тема Oblivion", "Тема Morrowind", "Тип", "Пояснение"],
        "en": ["Oblivion topic", "Morrowind topic", "Type", "Note"],
    },
    "globals": {
        "ru": ["Переменная Oblivion", "Morrowind", "Пояснение"],
        "en": ["Oblivion variable", "Morrowind", "Note"],
    },
}

SOURCE = {"ru": "Источник", "en": "Source"}
CYRILLIC = re.compile("[а-яё]", re.IGNORECASE)


def cell(text: str) -> str:
    text = text.strip().replace("|", "\\|")
    return text if text else " "


def rows(name: str, n: int) -> list[list[str]]:
    """Строки таблицы: n − 1 колонок как есть и пояснение одной строкой."""
    out = []
    for raw in (DATA / f"{name}.tsv").read_text(encoding="utf-8").splitlines():
        if not raw.strip() or raw.startswith("#"):
            continue
        cols = raw.split("\t")
        head, rest = cols[: n - 1], cols[n - 1 :]
        head += [""] * (n - 1 - len(head))
        out.append(head + [" ".join(c for c in rest if c)])
    return out


def english_notes() -> dict[tuple[str, str], str]:
    notes = {}
    for raw in NOTES_EN.read_text(encoding="utf-8").splitlines():
        if not raw.strip() or raw.startswith("#"):
            continue
        table, key, note = raw.split("\t", 2)
        notes[(table, key)] = note
    return notes


def table(name: str, lang: str, notes: dict[tuple[str, str], str]) -> str:
    headers = TABLES[name][lang]
    n = len(headers)
    lines = ["| " + " | ".join(headers) + " |", "|" + "---|" * n]
    for row in rows(name, n):
        if lang == "en" and CYRILLIC.search(row[-1]):
            translated = notes.get((name, row[0].strip()))
            if translated is None:
                print(f"нет перевода: {name}\t{row[0]}\t{row[-1]}", file=sys.stderr)
            else:
                row[-1] = translated
        lines.append("| " + " | ".join(cell(c) for c in row) + " |")
    lines.append("")
    lines.append(f"*{SOURCE[lang]}: `data/{name}.tsv`.*")
    return "\n".join(lines) + "\n"


def main() -> None:
    notes = english_notes()
    for lang in ("ru", "en"):
        out = OUT / lang
        out.mkdir(parents=True, exist_ok=True)
        for name in TABLES:
            (out / f"{name}.md").write_text(table(name, lang, notes), encoding="utf-8")


if __name__ == "__main__":
    main()
