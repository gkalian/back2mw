# MISC, KEYM, SGST — misc items, keys, sigil stones

**Status:** ✅ converted · **Morrowind:** `MISC` · **Code:** `transform::items`

## In Oblivion

- `MISC` — miscellaneous items: value, weight. Gold is a `MISC` too (`Gold001`).
- `KEYM` — keys: a separate type with the same fields.
- `SGST` — sigil stones: number of uses, value, weight and the list of effects the stone puts on a
  weapon or armor.

## In Morrowind

All of these are `MISC`: name, model, icon, value, weight and a "key" flag. Gold is the vanilla
records `Gold_001`, `Gold_005` … `Gold_100`.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| `MISC` | `MISC` | name, weight, value as is |
| `KEYM` | `MISC` with the "key" flag | a key is always converted if a lock refers to it, even if the key itself is not in the world |
| `SGST` | `MISC` | a plain item |
| `Gold001` | vanilla `Gold_001` | no own record; the count in a container is kept; recognised by `FormID` — works in mods too |

## What is lost

- **Sigil stone effects**: Morrowind has no stone enchanting, it is just an item.
- Scripts.
