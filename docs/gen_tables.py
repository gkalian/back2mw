"""Mapping tables from data/*.tsv → markdown for the documentation site (ru and en).

The pages include them through pymdownx.snippets (`--8<-- "effects.md"`), so the table on the site
is always the same one the converter reads. Run it before `zensical build`/`serve`:

    python docs/gen_tables.py

Writes to docs/_generated/<language>/ (the directory is in .gitignore). The notes in data/*.tsv are Russian;
the English ones live in docs/notes_en.tsv. A Russian note without a translation stays Russian in the English
table, and a warning is printed.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "data"
OUT = ROOT / "docs" / "_generated"
NOTES_EN = ROOT / "docs" / "notes_en.tsv"

# file → column headers; the last column (the note) takes everything left in the line.
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
    """Table rows: n − 1 columns as is and the note as one string."""
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
                print(f"no translation: {name}\t{row[0]}\t{row[-1]}", file=sys.stderr)
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
