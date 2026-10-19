# BOOK — books and scrolls

**Status:** ✅ converted · **Morrowind:** `BOOK` · **Code:** `transform::items`, `transform::paths::book_art_path`

## In Oblivion

"Scroll" flag, teaches a skill (−1 or one of 21 skills), value, weight, text (`DESC`) in HTML-like
markup with `<font face=N>` fonts and `<IMG src="Book/…">` images (path relative to
`Textures\Menus\`). A scroll may carry an enchantment. `Oblivion.esm` has 887 books, 110 skill books,
181 books with images.

## In Morrowind

The same set: scroll flag, skill (of 27), value, weight, text with markup and `<IMG SRC>` images
relative to `BookArt\`. Enchantment capacity, the scroll's enchantment.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| scroll flag, value, weight | as is | |
| skill | skill via the 21 → 27 table | Blade → Long Blade, Blunt → Blunt Weapon (`data/skills.tsv`) |
| `<font face=N>` | `"Magic Cards"`, face 4 → `"Daedric"` | Morrowind fonts |
| `<IMG src>` | `<IMG SRC="ob_<name>_<hash>.dds">` | the file is copied to `BookArt\` as is; the tag's width and height are unchanged |
| scroll enchantment | "cast once" `ENCH` | see [Enchantments](ench-spel.md) |

## What is lost

- The downscaled image copies (`Menus80`, `Menus50`) — not needed.
- An image whose path cannot be represented in cp1252 is removed from the text with a note.
- One image (`Book/Vehk_201x325.dds`) is missing from Oblivion's own archives — the reference stays in
  the text.
- Book scripts.

!!! warning "Not verified in the game"
    How Morrowind draws a 128×64 initial with `width=70 height=61` (crops or scales); whether large
    images (maps) fit; how the text reads with the replaced fonts.
