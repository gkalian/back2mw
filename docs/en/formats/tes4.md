# Source formats: TES4/TES5 plugins and BSA v103

What `tes4_read` reads. The plugin layout is implemented and pinned by tests on synthetic data
(`crates/tes4_read/src/container.rs`, `schema.rs`); the BSA layout was **measured on 2026-09-24 on the
real `Oblivion - Meshes.bsa`** (and later read by code, T1.2).

---

## Plugin

| | TES4 (Oblivion) | TES5 (Skyrim) |
|---|---|---|
| record header | 20 bytes | 24 bytes (plus `formVersion` u16 and `vcs2` u16) |
| `GRUP` group header | 20 bytes | 24 bytes |
| a record's `dataSize` | **excluding** the header | excluding the header |
| a group's `groupSize` | **including** the header | including the header |
| field header | tag (4) + size u16 | the same |

Record header: tag (4), `dataSize` u32, flags u32, FormID u32, `vcs1` u32, then in TES5 `formVersion` u16
and `vcs2` u16. Group header: `GRUP`, `groupSize` u32, label (4), group type i32, the rest up to 20/24
bytes — a stamp and other things we do not care about.

**Compression:** record flag `0x0004_0000`; the data is a u32 of the unpacked size, then a zlib stream.
The unpacked size is checked; a mismatch is an error.

**A field longer than u16:** it is preceded by `XXXX` of size 4 with a u32 of the real size, and the field
itself has its size written as zero. `XXXX` applies only to the one following field.

**TES4 vs TES5** cannot be told apart by header length. We distinguish them like this: in TES4 the `HEDR`
field begins right after the 20-byte header of the `TES4` record (offset 20). Confirmed on the real
`Oblivion.esm`: `HEDR` at offset 20, version 1.0, followed by `OFST`.

**Strings** are cp1252 with a terminating NUL (`tes4_read::cp1252`).

**Records with empty data (`data_size = 0`).** They occur in Oblivion.esm: at least one `STAT`
(FormID=0008060C, flags=0x00000000). `Fields::parse` on an empty slice returns an empty set without an
error. `Stat::from_record` on such a record returns `MissingField(EDID)` — this is expected; the caller
must skip records with `data.is_empty()`. Documented by the synthetic test
`stat::tests::empty_data_returns_missing_field`.

### Measured on real plugins (2026-09-24, test `real_plugins::oblivion_esm_parses_completely`)

| Parameter | Value |
|---|---|
| Format | TES4 |
| File size | 264 MiB |
| Reading from disk | ~200–570 ms (depends on the cache) |
| Parsing (`Plugin::parse`) | ~30–34 s (debug build, 264 MiB, 1,167,016 records) |
| Top-level groups | 56 |
| Records in total | 1,167,016 |
| Of them compressed | 41,789 |
| Top 3 types | REFR: 1,025,617, CELL: 35,494, LAND: 31,823 |
| STAT | 6,014 |
| STAT with empty data | 1 |

All 10 TES4 plugins of the test data set (Oblivion.esm + 9 DLCs) parse without errors. 3 files are skipped
as TES3 (Morrowind.esm, Tribunal.esm, Bloodmoon.esm). `DLCShiveringIsles.esp` has 0 records (its content
is possibly merged into Oblivion.esm — not verified).

### Measured over STAT (2026-09-24, test `real_plugins::every_stat_reads_through_the_spec`)

| Parameter | Value |
|---|---|
| STAT in total | 6,014 |
| Without MODL | 1 (an empty record, skipped) |
| Maximum EDID length | 53 (`RocksGreatForestLichenRockGreatForest140ClusterLichen`) |
| Maximum MODL length | 93 (`Oblivion\Architecture\Citadel\Interior\CitadelLordsChambers\CitadelDeadraLordsChambers01B.NIF`) |
| EDID or MODL with non-ASCII | 0 |

### What we need from the records (for phase 1)

`STAT`: `EDID` (zstring), `MODL` (mesh path **relative to `Meshes\`**, zstring), `MODB` (f32, radius) —
in TES4; in TES5 instead of `MODB` there is `OBND` (6 × i16). Spec: `records/stat.rs`.

That `MODL` paths are relative to `Meshes\` is **confirmed** (T1.3a, 2026-09-24): by `meshes\` + `MODL`,
5,176 meshes were found in `Oblivion - Meshes.bsa`, another 784 in `DLCShiveringIsles - Meshes.bsa`, 52
nowhere.

### `STAT` → Morrowind plugin (T1.8, 2026-09-24, test `back2mw/tests/statics::every_stat_of_oblivion_esm_is_written_and_read_by_esmtool`)

| Parameter | Value |
|---|---|
| `STAT` in `Oblivion.esm` | 6,014 |
| Empty records (skipped) | 1 |
| Without `MODL` (skipped, the report keeps the `EDID`; e.g. `ARVinehangUbig`) | 1 |
| Written to the plugin | 6,012 |
| `STAT` in the `esmtool` (0.51.0) dump, exit code | 6,012, 0 |
| The longest ID | 31 (the budget of the `transform::ids` scheme) |
| The longest mesh path in the output | 31 (since T1.14; under the old rule it was 96) |

~~Mesh path rule: `ob\` + `MODL` in lower case.~~ **Cancelled 2026-09-24 (T1.14):** under it, 5,722 of
6,012 paths came out longer than 31 characters, and the Construction Set truncates such a `MODL`
([tes3.md](tes3.md), "`MODL` length limit"). The current rule is below.

### Short `MODL` and `ITEX` paths (T1.14, `transform::paths`)

`ob\<name>_<hash>.<ext>`, never longer than 31 characters:

- **hash** — FNV-1a 64 of the normalised source path (lower case, `\` separator, no empty segments), the
  low 10 base36 digits (51.7 bits). FNV is implemented by us: `DefaultHasher` gives no stability guarantee
  across Rust versions, and the path is part of the output;
- **name** — the source file name without the extension, Latin letters and digits, everything else `_`,
  cut to 13 characters. For humans only; uniqueness comes from the hash;
- the source folders **are not kept**: there is no room for them. The same `Dagger.nif` from different
  folders gets different hashes;
- the path is a pure function of the source path: it does not depend on the traversal order or on which
  other plugins are converted. A clash of two different sources on one short path is not hidden by
  `pipeline` but aborts the run naming both (`claim_short_path`).

Measured 2026-09-24, test `back2mw/tests/statics::short_paths_fit_and_never_collide_on_oblivion_esm`,
all 20 types with a model from `Oblivion.esm` plus weapon icons:

| Parameter | Value |
|---|---|
| Records | 14,807 |
| Distinct source paths (meshes + icons) | 8,165 |
| Distinct short paths | 8,165 — **0 collisions** |
| The longest | 31 |

The estimated probability of even one collision over 20,000 paths is ~5·10⁻⁸.

**Changing paths does not break saved games:** references in cells and inventories go by the record ID,
not by the mesh path. `install` removes old `Meshes\ob\…` files with long paths as obsolete, by the
manifest.

The FormID is taken as in the file (`Oblivion.esm` has master index 0); for plugins with masters the
FormID would first have to be made absolute — this was not done in T1.8.

### Oblivion `WEAP` (T9.1, spec `records/weap.rs`)

Fields: `EDID`, `FULL` (name), `MODL`/`MODB`/`MODT` (model — one for both hand and ground), `ICON`
(relative to `Textures\Menus\Icons\`), `SCRI` (script), `ENAM` (enchantment), `ANAM` (u16, the **charge**
of the ready enchantment, not the capacity), `DATA`.

`DATA` is **30 bytes** in all 1,319 `WEAP` of `Oblivion.esm` (2026-09-24). The reference
(`docs/reference/tes4_record_definitions.md`) says "32" in the table header, but its own fields add up to
30, and its code reads 30.

| Offset | Type | Field |
|---|---|---|
| 0 | u32 | type: 0 blade 1H, 1 blade 2H, 2 blunt 1H, 3 blunt 2H, 4 staff, 5 bow |
| 4 | f32 | speed |
| 8 | f32 | reach |
| 12 | u32 | flags: `0x1` — ignores normal weapon resistance |
| 16 | u32 | value |
| 20 | u32 | health |
| 24 | f32 | weight |
| 28 | u16 | damage |

### `WEAP` → `WEAP` (T9.1, `transform::weapons`)

**How it was calibrated.** A script (one-off, not in the repository) matched the weapons of `Oblivion.esm`
and `Morrowind.esm` + `Tribunal.esm` + `Bloodmoon.esm` by identical `FULL`/`FNAM`: **49 pairs**
(Iron/Steel/Silver/Dwarven/Glass/Ebony/Daedric Dagger, Longsword, Claymore, Mace, War Axe, Battle Axe,
Warhammer, plus Goldbrand, Volendrung, Mace of Molag Bal, bound ones). Separately — medians over all 548
vanilla Morrowind `WEAP`s by type. 2026-09-24.

**Type.** The rule matched vanilla on **49 pairs of 49**:

| Oblivion | Morrowind | How it is told apart |
|---|---|---|
| 0, `reach < 1.0` | 0 short blade | Oblivion encodes it with reach itself: dagger 0.6, shortsword 0.8, longsword 1.0 — 20 of 20 pairs |
| 0, `reach ≥ 1.0` | 1 long blade 1H | |
| 1 | 2 long blade 2H; 6 spear if in the name | a word in the `EDID` or the model's **file name** (not the folder) |
| 2 | 3 blunt 1H; 7 axe 1H if `axe` | the same |
| 3 | 4 blunt 2H; 8 axe 2H (`axe`); 6 spear | the same |
| 4 | 5 blunt 2H wide (Morrowind staves) | |
| 5 | 9 bow | |

Spear words: `spear`, `halberd`, `glaive`, `trident`, `lance` (`pike` is not used — it catches `spike`).
Oblivion does not distinguish axes and spears as types; the reference does the same by mesh path.

**Fields.**

| Morrowind field | Rule | Basis (pairs / vanilla) |
|---|---|---|
| weight | as is | median Morrowind/Oblivion 0.91, iron 1.0 |
| value | as is | spread 0.14–27.8, median 2.07; the ratio grows with value (glass, ebony, daedric — 7–28×). The owner's decision of 2026-09-27 (T9.5): **all prices exactly as in Oblivion**, damage by the coefficients of this table |
| health | × 5.71, up to 65,535 | median over 46 pairs excluding bound ones |
| speed | × type coefficient | 0: 1.79, 1: 1.35, 2: 1.56, 3: 1.44, 4: 1.43, 7: 1.14, 8: 1.25; with no pairs (5, 6) — the overall 1.44; bow 1.0 (1.0 in both games) |
| reach | vanilla median by type | Morrowind sets it by type: 1.0; blunt 2H 1.5; staff and spear 1.8 |
| enchantment capacity | vanilla median by type | 0: 40, 1: 60, 2: 70, 3: 50, 4: 55, 5: 70, 6: 50, 7: 50, 8: 55, 9: 35. `ANAM` is the charge, not the capacity |
| damage | the largest = damage × type k; the six numbers are shares of it | k: 0: 1.08, 1: 1.68, 2: 1.96, 3: 1.19, 4: 2.0, 7: 1.85, 8: 2.62; with no pairs — the overall 1.69. The shares are vanilla medians by type (blades have all three attacks, axes and clubs almost no thrust, a spear's main attack is thrust, a bow has only chop) |
| flag `0x1` | as is | silver and daedric weapons have it in both games |

**Prices and damage against vanilla (T9.5, 2026-09-27).** A one-off census (not in the repository) matched
by name with `Morrowind.esm` + `Tribunal.esm` + `Bloodmoon.esm` 65 `WEAP` pairs and 42 `ARMO` pairs. The
Morrowind/Oblivion price ratio grows with the price: iron and steel 0.7–2.3, silver 1.0–2.5, dwarven
1.4–3.9, glass 7–28, ebony 7–12, daedric 3.7–20 (armor: iron and steel 0.7–1.2, orcish 3–3.8, glass, ebony,
daedric 13.6–22); artifacts are out of line (Bloody Helm ×618, Volendrung ×0.14). Damage at the edges
grows faster than the coefficient: daedric claymore 26 → 60 (the coefficient gives 44), battle axe 26 → 80
(68). The owner's decision: prices as in Oblivion, damage by the coefficients; so Morrowind's economy for
top items is 10–20 times cheaper than vanilla here (glass claymore 1,150 against 32,000).

**Price raising (T9.5a, `transform::prices`, `data/anchors/value.tsv`).** At the player's wish — the
window's "Raise prices to Morrowind" checkbox, the `--morrowind-prices` flag, the `morrowind_prices` key —
the price of weapons and armor is raised up to the curve if it is lower in Oblivion; **it is never lowered
below Oblivion's price** (the owner's request of 2026-09-27). The curve is the medians of the same pairs
without enchantment by material steps, a straight line between points, past the last one — its ratio:

| Weapons (Oblivion → Morrowind) | Armor |
|---|---|
| iron 20 → 40, steel 45 → 80, silver 60 → 120, dwarven 300 → 600, glass and ebony 1,425 → 15,000, daedric 3,100 → 30,000 | iron 45 → 34, steel 95 → 68, orcish 460 → 1,760, glass and ebony 1,000 → 17,000, daedric 2,500 → 34,000 |

For glass and ebony the medians go down (ebony is cheaper than glass), so the points are merged: the curve
must rise. Morrowind's iron and steel armor is cheaper than Oblivion's — the curve does not touch them.
Arrows and bolts are not raised: in Morrowind they are not more expensive (iron 1 → 1 … daedric 35 → 20).
Clothing, items, lights — unchanged: there are no same-name pairs for calibration. Gauntlets: the curve
from the pair's price, the result halved for each.

Measured 2026-09-27, test `back2mw/tests/real_prices::raised_prices_are_closer_to_morrowind_and_never_lower`:
on 88 pairs without enchantment the typical miss against vanilla (median |ln(ours/vanilla)|) is **2.5 times
without raising, 1.25 times with it**; 1,286 weapon prices raised (almost all — Morrowind's iron is already
twice as expensive) and 548 armor prices; 0 lowered. One curve for all materials errs on items between the
steps: orcish cuirass 940 → 15,307 (vanilla 2,800), dwarven war hammer 430 → 2,264 (600), glass longsword
840 → 7,512 (16,000). `SEWorld` with the flag: 30 weapon prices raised, `esmtool`/`niftest` clean.

A check on the Iron Longsword (damage 10): chop 2–12, slash 1–17, thrust 3–15; vanilla has 2–13, 1–18,
4–16.

**What is lost and reported:** the script (`SCRI`) and the enchantment (`ENAM`) — at the time of T9.1 they
were not converted yet (enchantments came in T9.4, see below); damage above 255 and health above 65,535
hit the field limit; a name longer than 31 characters is truncated. The icon is copied from
`textures\menus\icons\<ICON>` to `Icons\ob\…`; if it is not in the archives, the weapon stays, but without
an inventory picture.

Measured 2026-09-24, test `back2mw/tests/statics::every_weapon_of_oblivion_esm_becomes_a_weap_read_by_esmtool`:

| Parameter | Value |
|---|---|
| `WEAP` in `Oblivion.esm` | 1,319 |
| Converted as `WEAP` | 1,319, `esmtool` reads all 1,319 |
| By Morrowind type | short blade 194, long 1H 219, long 2H 117, blunt 1H 148, blunt 2H 105, staff 228, axe 1H 98, axe 2H 79, bow 131 |
| Without an icon | 2 |
| With a script (removed) | 130 |
| With an enchantment | 942 — most of Oblivion's weapons are enchanted |
| Notes | 1 — the name `Nelrene's Cere~monial Shortsword` (32 characters) is truncated |

**Not verified in the game:** how an Oblivion model sits in the hand. Oblivion attaches weapons to its
skeleton's `Weapon` node, Morrowind to `Weapon Bone`; orientation and offset may differ. This is the first
thing to look at in Morrowind (T9.1, ⛔ engine).

### Oblivion items (T2.1, spec `records/item.rs`)

Common fields: `EDID`, `FULL`, `MODL`, `ICON`, `SCRI`, `ENAM`; potions, ingredients and sigil stones also
have an effect list (`EFID`…). `DATA` differs, and for each type it has one size in all records of
`Oblivion.esm` (2026-09-24):

| Type | `DATA` | Layout |
|---|---|---|
| `MISC`, `KEYM`, `SLGM` | 8 | u32 value, f32 weight. In `SLGM` the soul is `SOUL` (u8), the capacity `SLCP` (u8, 0…5) |
| `SGST` | 9 | u8 number of uses, u32 value, f32 weight |
| `BOOK` | 10 | u8 flags (`0x01` scroll), i8 skill (−1 or 0…20 in Oblivion's skill order), u32 value, f32 weight. The text is `DESC` |
| `ALCH`, `INGR` | 4 | f32 weight. Value and flags are in `ENIT` (8 bytes): u32 value, u8 flags (`0x01` no autocalc, `0x02` food) |
| `APPA` | 13 | u8 type (0 mortar, 1 alembic, 2 calcinator, 3 retort — as in Morrowind), u32 value, f32 weight, f32 quality in percent |
| `AMMO` | 18 | f32 speed, u32 flags (`0x01` ignores resistance), u32 value, f32 weight, u16 damage |

### Items → Morrowind (T2.1, `transform::items`)

| Oblivion | Morrowind | Rule |
|---|---|---|
| `MISC`, `SGST` | `MISC` | name, weight, value as is |
| `KEYM` | `MISC` with the key flag | |
| `SLGM` | **no record is written**, references go to a vanilla gem (T2.1a) | capacity `SLCP` 1…5 → `Misc_SoulGem_Petty`, `_Lesser`, `_Common`, `_Greater`, `_Grand`; `AzurasStar` (`FormID` `0x193`) → `Misc_SoulGem_Azura`. Capacity outside 1…5 — an ordinary `MISC` with a note. Details below |
| `BOOK` | `BOOK` | the scroll flag as is; the skill by skill name (Blade → Long Blade, Blunt → Blunt Weapon, the 21 skills map without repeats). In the text the font numbers `<font face=N>` → `"Magic Cards"`, 4 → `"Daedric"`; `<img src>` images are moved to `BookArt\` (T2.1a, below) |
| `ALCH`, `INGR` | `ALCH`, `INGR` | effects through the `data/effects.tsv` table (T2.1b, "Effects" below); a potion's effects are all "on self", an ingredient has up to four |
| `APPA` | `APPA` | type as is; quality / 50 (Oblivion 10…100 % → Morrowind 0.2…2.0, vanilla Morrowind 0.5…2.0) |
| `AMMO` | `WEAP`, type "arrow" | the `chop` max is piecewise linear over the pairs iron 8 → 3, steel 9 → 4, glass 13 → 6, ebony 14 → 10, daedric 15 → 15; the min is `max(1, max − 5)`. Silver (10 → 3 in vanilla) does not fit the monotonic curve; here 1–5. Enchantment capacity 12 — the vanilla median |

Scripts and enchantments were not converted at the time of T2.1 — separate tasks (enchantments — T9.4);
the effects of sigil stones are not converted (in Morrowind it is a simple item). The item stays, with a
note in the report. The icon — like a weapon's, `Icons\ob\…`.

Measured 2026-09-25, test `back2mw/tests/statics::every_item_of_oblivion_esm_becomes_an_item_read_by_esmtool`:

| Parameter | Value |
|---|---|
| Items in `Oblivion.esm` | 2,506 |
| Converted, `esmtool` reads | 2,506 of 2,506 |
| By kind | books 887, misc (`MISC`/`SGST`/`SLGM`) 579, keys 464, potions 252, ingredients 173, arrows 128, apparatus 23 |
| Skill books | 110 |

After T2.1a (2026-09-25, the same test) there are 2,479 of 2,479 records (misc 552): 27 soul gems no longer
produce records.

**Not verified in the game:** inventory icons, how arrows look and fly, whether books and scrolls open,
how the text reads with the replaced fonts.

### Effects (T2.1b, spec `records/effects.rs`, `transform::effects`, `data/effects.tsv`)

In Oblivion potions, ingredients, sigil stones, enchantments and spells an effect is a pair of fields:
`EFID` (the code) and `EFIT`, and a script effect also has `SCIT`. `EFIT` is 24 bytes in all 5,253 effects
of `ALCH`/`INGR`/`ENCH`/`SPEL` in `Oblivion.esm` (2026-09-25):

| Offset | Type | Field |
|---|---|---|
| 0 | char[4] | effect code (the `EDID` of the `MGEF` record: `REHE`, `FOAT`…) |
| 4 | u32 | magnitude |
| 8 | u32 | area |
| 12 | u32 | duration, s |
| 16 | u32 | range: 0 self (3,409), 1 touch (930), 2 target (914) — as in Morrowind |
| 20 | i32 | actor value: attribute 0…7 (the same order as in Morrowind), skill 12…32 (Oblivion skill 0…20) |

Effects per record: potions have 1 (223 of 253), ingredients 4 (161 of 173) or 1 (12), enchantments and
spells up to 12. There are 153 codes: 145 `MGEF` records and 8 that have no record but occur in spells
(`BA01`…`BA06`, `DUMY`, `RSWD`).

Morrowind's set of effects is closed: 137 in `Morrowind.esm` (0…136), 4 more in Tribunal and Bloodmoon.
The engine accepts no new effects, so an Oblivion effect either maps onto an existing one or is dropped.
The table is `data/effects.tsv`, every decision there with an explanation. Rules:

- code → Morrowind effect number 0…136 (effects 137+ need the Tribunal/Bloodmoon masters, which we do not
  have);
- for attribute effects the attribute is taken from the actor value as is; for skill effects the Oblivion
  skill (value − 12) is translated by the skill table (Blade → Long Blade, as for books);
- Oblivion's magnitude is both Morrowind's min and max; duration and area as is;
- a potion's range is always "self"; Morrowind stores only the effect, skill and attribute for an
  ingredient (the engine computes magnitude and duration when it is eaten).

With no counterpart (dropped with a note): script effects `SEFF`, reflect damage `REDG`, reanimate `REAN`,
bound greaves, quest weapons and armor, summoning creatures Morrowind does not have (spider daedra, dark
seducer, flesh atronachs, shambles), summon bear (a Bloodmoon effect in Morrowind). Approximations:
Oblivion's single "calm/frenzy/demoralize/rally" → the "humanoid" variants; summon lich → bonelord, zombie
→ lesser bonewalker, ghosts and wraiths → ancestral ghost, xivilai and dremora lord → dremora.

Measured 2026-09-25, test `statics::potion_and_ingredient_effects_of_oblivion_esm`:

| | records | effects converted | records without effects |
|---|---|---|---|
| Potions | 252 | 320 | 8 (script only) |
| Ingredients | 173 | 645 | 5 |

Dropped: `SEFF` 14, `REDG` 6. `esmtool` reads 425 records of 425. Enchantments and spells (`ENCH`, `SPEL`)
— by the same table, T9.4 (the section below).

**Not verified in the game:** whether potions work (Oblivion magnitudes and durations against Morrowind's
balance), whether potions can be brewed from converted ingredients.

### Enchantments and spells (T9.4, spec `records/magic.rs`, `transform::magic`)

`Oblivion.esm`, measured 2026-09-27 (a census; the numbers are repeated by the test
`statics::every_enchantment_and_spell_of_oblivion_esm_is_written_and_read_by_esmtool`):

| | `ENCH` | `SPEL` |
|---|---|---|
| Records | 1,542 | 1,137 |
| Header | `ENIT`, 16 bytes in all: `u32` type, `u32` charge, `u32` cost, `u8` flags (0x01 — manual cost), 3 bytes of junk | `SPIT`, 16 bytes in all: `u32` type, `u32` cost, `u32` level (0…4), `u8` flags, 3 bytes of junk |
| Types | scroll 341, staff 222, weapon 327, apparel 652 | spell 602, ability 292, lesser power 137, power 74, disease 32; no poison (type 5) |
| Effects | 2,411, 2,336 map to the table | 1,856, 1,697 map |
| Without a single convertible effect | 21 | 97 (mostly scripted `SEFF`, 89 effects) |

References to `ENCH` (`ENAM`): `WEAP` 942 (staff 221, weapon 721), `ARMO` 564, `CLOT` 404, `BOOK` 307
(scrolls), `AMMO` 65. **The charge in `ENCH` equals the cost** for all staves and scrolls; the real charge
is the item's `ANAM` (u16, in all 990 enchanted `WEAP`/`AMMO` with `ANAM`), `ANAM`/cost has a median of 30
(staves) and 37 (weapons), from 0 to 1,111. 17 enchanted items have no `ANAM`.

The Morrowind reference (`Morrowind.esm`, esmtool 2026-09-27): `ENCH` 708 — cast once 138, when strikes
130, when used 350, constant 90; all with the autocalc flag; weapon charge ≈ 10 × cost. No `ENCH`/`SPEL`
in vanilla has more than 8 effects (in all three `.esm`s). The autocalc flag on a `SPEL` also means "hand
out to suitable autocalculated NPCs" (as the CS describes it; not verified by us) — so we do not set it.

Rules (the owner's decisions of 2026-09-27):

| Oblivion | Morrowind | Rule |
|---|---|---|
| `ENCH` scroll / staff / weapon / apparel | cast once / when used / when strikes / constant | cost as is; effects by `data/effects.tsv`, more than 8 — truncated with a note |
| charge | `ENDT` charge | for "when strikes" and "when used" — the item's `ANAM`; one `ENCH` on items with different charges → a variant per charge (`obc1_…`, the first by ascending charge gets the record ID); for scrolls and constant ones — the `ENCH`'s charge |
| `ENCH` without convertible effects | — | not written, the item has no enchantment, a note |
| `ENCH` referenced by no item of the plugin | — | not written |
| `SPEL` spell / power / disease / ability | the same types | cost as is, **no autocalc** |
| lesser power | spell, cost 0, "always succeeds" | the closest in behavior: no limit, no magicka |
| "player start spell" flag | — | not set: the Morrowind player is not given Oblivion spells |
| item script | — | not converted (MWScript — phase 11) |

Measured 2026-09-27, the same test:

| Parameter | Value |
|---|---|
| `ENCH` written, `esmtool` reads | 1,469 of 1,469: constant 610, when strikes 343, cast once 301, when used 215; charge variants 51 |
| Enchanted items | `WEAP` 1,000 (with staves and arrows), `ARMO` 637 (with hood helmets), `CLOT` 397, `BOOK` 304 |
| Items without an enchantment | 35 — they refer to the 21 `ENCH`s without convertible effects |
| Uses on a full charge | median 30, from 0 (the charge is less than the cost — as in Oblivion) to 1,111 |
| `SPEL` written, `esmtool` reads | 1,040 of 1,137: spells 571, abilities 249, lesser powers 122, powers 66, diseases 32; 97 not converted |
| Truncated to 8 effects | `ENCH` 1, `SPEL` 14 |
| Broken item → `ENCH` references | 0 |

`back2mw all --worldspace SEWorld` (2026-09-27): `ENCH` 18, `SPEL` 1,040, `esmtool`/`niftest` clean.
**A loose end:** a mod's item referring to a master's `ENCH` stays without an enchantment (a note) — an
enchantment is built only in the plugin where it is; not verified on mods.

**Not verified in the game:** the balance of Oblivion's costs and magnitudes; whether constant and "when
strikes" effects work; a lesser power as a spell costing 0.

### Soul gems (T2.1a)

In Morrowind a soul gem is not a separate record type but a `MISC`. What makes it a gem is the **ID**: the
engine recognises gems by the IDs `Misc_SoulGem_*` (this is known from community documentation — MWSE,
OpenMW; not verified in the original engine). Capacity = value × `fSoulgemMult`. Our own `MISC` with the
`ob_` prefix would not become a gem, so `SLGM` produces no records. The converter returns a
`ConvertedObjects::vanilla` list (`FormID` → vanilla ID): references to gems in placements and containers
follow it to the vanilla record. At the time of T2.1a there were no such references yet — that came in
phase 4 and later.

Vanilla gems, `Morrowind.esm` (measured 2026-09-25):

| ID | Value | Weight |
|---|---|---|
| `Misc_SoulGem_Petty` | 10 | 0.2 |
| `Misc_SoulGem_Lesser` | 20 | 0.5 |
| `Misc_SoulGem_Common` | 40 | 1 |
| `Misc_SoulGem_Greater` | 60 | 1.5 |
| `Misc_SoulGem_Grand` | 200 | 2 |
| `Misc_SoulGem_Azura` | 5000 | 2 |

Decisions:

- **Black soul gems** (NPC souls) — all four in `Oblivion.esm` have capacity 5 and become grand ones. An
  NPC soul cannot be caught in any gem in Morrowind. The owner's decision, 2026-09-25.
- **Filled gems** (`SOUL` > 0, 16 records) — the same empty gem. In Morrowind the soul is stored in the
  instance (in the world or in an inventory), not in the record. The soul is lost, with a note in the
  report. It could be restored when converting placements and containers.
- **Azura's Star** is recognised by `FormID` `0x193`: `SLGM` has no reusability flag.
- **Capacity outside 1…5** — an ordinary `MISC` with a note. `Oblivion.esm` has two: the quest
  `Mg17BlackSoulGem` and `Mg17BlackSoulGemFilled`, capacity 0.
- A gem's script (`Soul Tomato`, `SE38…`) is not converted, with a note in the report.

Measured 2026-09-25, test `statics::soul_gems_of_oblivion_esm_point_at_vanilla_gems`: of 29 `SLGM`s, 27
mapped to vanilla gems (grand 12, greater 5, common 4, lesser 3, petty 2, Azura's Star 1), 2 stayed items.

**Not verified in the game:** whether the original engine accepts the converted gems as gems (they are
vanilla gems in Morrowind, so it should).

### Images in books (T2.1a)

Oblivion: `<IMG src="Book/fancy_font/a_70x61.dds" width=70 height=61>`, the path relative to
`Textures\Menus\`. They live in `Textures\Menus\Book\…`; next to them `Menus80`, `Menus50` — downscaled
copies we do not take. The `fancy_font` initials are DXT5 128×64, of which a `width`×`height` part is
visible.

Morrowind: `<IMG SRC="…">` — the path relative to `BookArt\`. In vanilla it is always a bare file name (24
different ones in `Morrowind.esm`, 4 in `Tribunal.esm`, 1 in `Bloodmoon.esm`). In `Bloodmoon.esm`
(`bk_BM_StoneMap`) it is `magicstonemap4.dds`, so vanilla already uses DDS in `BookArt`. The largest
vanilla image is 300×390.

Rule: `src` is replaced with `ob_<name>_<hash>.dds` (`transform::paths::book_art_path`, the same scheme as
for meshes but without a subfolder — whether the engine understands a subfolder in `SRC` is unknown). The
file is copied as is into `output/BookArt/`. `width`/`height` and the rest of the tag are unchanged. An
image with an unrepresentable path is removed from the text with a note.

Measured 2026-09-25, test `statics::every_book_image_of_oblivion_esm_is_in_the_source_archives`: 181 books
with images, 54 distinct images. 2 of them are in `DLCShiveringIsles - Textures.bsa` (Shivering Isles is
merged into `Oblivion.esm`), the rest in `Oblivion - Textures - Compressed.bsa`. One image,
`Book/Vehk_201x325.dds`, **is in no archive of the game** — a hole in Oblivion's own data. The run writes a
warning; the reference in the text stays.

**Not verified in the game:**
- how Morrowind draws a 128×64 initial with `width=70 height=61`: crops it like Oblivion, or scales it;
- whether the engine complains about a reference to the missing `vehk_201x325.dds`;
- whether large images (maps, `elderscroll01`) fit on the page.

### World objects (T2.2, spec `records/world.rs`, `transform::world`)

Oblivion layouts, measured on `Oblivion.esm` on 2026-09-25:

| Type | Records | Fields |
|---|---|---|
| `ACTI` | 1,252 (1 without `MODL`, 927 with a script, 540 with a name) | `SNAM` — sound (50) |
| `DOOR` | 501 | `FNAM` 1 byte in all: 0x01 Oblivion gate, 0x02 automatic, 0x04 hidden, 0x08 minimal use. `SNAM` — open sound (417), `ANAM` — close (406), `BNAM` — loop sound (20) |
| `CONT` | 873 | `DATA` 5 bytes in all: u8 flags (0x02 respawns — 436 records), f32 the box's **own weight**. `CNTO` 8 bytes: u32 `FormID`, i32 count. `SNAM`/`QNAM` — open and close sounds |
| `LIGH` | 1,625 (**1,328 without `MODL`**, 2 with `ICON`) | `DATA` 32 bytes (1,064) or 24 (561, no falloff and cone angle): i32 time (−1 forever in 1,623), u32 radius (16…7,000), 4 bytes color, u32 flags, [f32 falloff, f32 angle], u32 value, f32 weight. `FNAM` — falloff (1,559), `SNAM` — sound (75) |
| `FLOR` | 155 | `PFIG` — ingredient (153; two have none) |
| `FURN` | 186 | `MNAM` 4 bytes — sit/sleep markers |
| `GRAS` | 108 | `DATA` 32 bytes — grass scattering parameters |

Contents of `CONT` in `Oblivion.esm` — 9,971 `CNTO`s, by target type: `LVLI` 2,718, `WEAP` 1,781, `BOOK`
1,206, `ARMO` 1,123, `CLOT` 692, `MISC` 584, `ALCH` 549, `KEYM` 485, `INGR` 420, `AMMO` 158, `SGST` 150,
`SLGM` 37, `LIGH` 36, `APPA` 32.

The Morrowind reference, `Morrowind.esm` 2026-09-25: `ACTI` 697, `DOOR` 140, `CONT` 890, `LIGH` 574 (144
without a mesh). **All** 890 containers have the 0x8 flag; plants have 0x9 or 0xB, capacity 0, one item
inside (`flora_muckspunge_01` → `1×random_muck`). Capacity of ordinary containers (769): 200 — 330 times,
both the mode and the median; then 300 (90), 50 (79), 25 (61). Light burn time: −1 in 429, 120 in 76, 150
in 46. Radius 16…4,096.

Rules:

| Oblivion | Morrowind | Rule |
|---|---|---|
| `ACTI`, `FURN` | `ACTI` | name and mesh; Morrowind has no furniture, you cannot sit. Without a name an activator cannot be activated, as in vanilla |
| `DOOR` | `DOOR` | name and mesh. Sounds came in T10.1 (see "Sounds" below). Teleports — phase 6 |
| `CONT` | `CONT` | capacity **200** for all: an Oblivion box's weight is not a capacity. The "respawns" flag 0x02 as is, 0x8 always. The contents are item IDs (see below) |
| `FLOR` | `CONT`, flags 0xB | organic, respawns, capacity 0, `1×` ingredient inside |
| `LIGH` | `LIGH` | time, radius, color, value, weight as is. Flags 0x1…0x8 and 0x20…0x100 match in meaning and are carried; 0x10 (undocumented in Oblivion, "fire" in Morrowind) and the spotlight 0x200/0x400 are removed. A light without a model is a record without `MODL`, like 144 vanilla ones. Falloff (`FNAM`) is not carried |
| `GRAS` | `STAT` | the scattering parameters are lost |

**Container contents.** `FormID`s from `CNTO` are translated into IDs once the whole plugin is read: an
item may come later in the file than the container. The ID is looked up in the allocator (including master
records from the same run), then as a vanilla soul gem (T2.1a). If nothing is found, the item is dropped
with a note. The run then cleans the contents once more of items whose mesh was not converted: a reference
to a non-existent ID in `NPCO` is a broken record. Vanilla IDs (without the `ob_` prefix) are not touched.
Soul gems from **another** plugin of the run are lost for now: the `vanilla` list lives within a plugin.

Measured 2026-09-25, test `statics::every_world_object_of_oblivion_esm_is_written_and_read_by_esmtool`:

| Parameter | Value |
|---|---|
| World objects, `esmtool` reads | 4,591 of 4,591 |
| By kind | activators 1,437 (with furniture), containers 873, plants 155, doors 501, lights 297 with a model + 1,328 without |
| Contents with an ID | 7,397 |
| Contents without an ID | 2,727, of them 2,718 are leveled lists (T2.5) |

With T2.5 (2026-09-27, the same test): 10,080 with an ID (2,718 of them references to `LEVI`), 44 without.
Of the 44, 35 are references to hood `CLOT`s that got an ID but were not converted (22 records); such
references used to be removed only by the whole run, now also by the plugin translation. With hoods as
helmets (T9.2f, 2026-09-27): 10,114 with an ID, 10 without.

A full run `back2mw all --plugin Oblivion.esm` (release, 100 s): written ACTI 526, DOOR 244, CONT 972,
LIGH 1,449 (121 with a mesh). The rest fell away because the mesh could not be read:
`NiControllerManager` (animated doors and levers), NIF 10.2, havok constraints — this is T2.3. There are no
dangling references in `NPCO`: 5,881 items, all records of the plugin or vanilla soul gems (35).

**Not verified in the game:**
- doors: Morrowind rotates a door in code around the mesh origin, while an Oblivion door opens with an
  animation. Where the doors' hinges are and what the conversion does about it was measured in T2.6
  ([nif-oblivion.md](nif-oblivion.md), "Doors: what `Open` moves"); not verified in the game;
- the "dynamic" flag is set on only 63 of 1,625 Oblivion lights, but on 427 of 574 in vanilla. If a
  Morrowind light without it does not light characters, every light needs the flag;
- a radius of 7,000 exceeds the vanilla maximum of 4,096;
- plants: whether they can be harvested and the harvest disappears.

---

### Leveled lists (T2.5, spec `records/leveled.rs`, `transform::leveled`)

`LVLI` of `Oblivion.esm`, measured 2026-09-27, test `tes4_read/tests/real_leveled`:

| Parameter | Value |
|---|---|
| Records | 1,398 (1 empty) |
| Field order | `EDID, LVLD, LVLF, LVLO…` — 1,376; **old format** `EDID, LVLD, LVLO…, DATA` — 22 |
| `LVLD` (chance of "nothing", %) | 1 byte; 0 in 885, then 25, 50, 75, 90; **128** in 5 old-format records (Arena and Dark Brotherhood rewards) |
| `LVLF` | 1 byte: 0x01 all levels ≤ player, 0x02 roll for each unit of the count, 0x04 "use all". Values 0/1/2/3; **`LVLI` has no 0x04** (only `LVSP` does) |
| Old-format `DATA` | 1 byte, 0 in 21, 1 in 1. Read as `LVLF` — **an assumption, not verified** |
| `LVLO` | 12 bytes — 16,181: `i16` level, 2 bytes of junk, `u32` `FormID`, `i16` count, 2 bytes of junk (not zeros); **8 bytes** — 93 (all in old-format records): no count, it is 1 |
| Entries | 16,274; levels 1…30; up to 104 entries in a list |
| Count > 1 | 1,344 entries: `MISC` — 734, of them `Gold001` 728 (2…10,000); nested lists — 522 (almost all arrows, 3…50); arrows — 54 (25…150); soul gems — 30; ingredients — 4 |
| Entries by target type | `LVLI` 3,478, `WEAP` 3,437, `ARMO` 2,719, `BOOK` 1,682, `CLOT` 1,375, `MISC` 1,330, `INGR` 1,002, `ALCH` 736, `AMMO` 155, `SLGM` 151, `SGST` 150, `APPA` 53, `LIGH` 4, `KEYM` 2 |

Gold is the `MISC` `Gold001`, `FormID` 0x0000000F. `LVLC` (682) and `LVSP` (306) have the same layout
(`LVLC` also has `SCRI`, `TNAM`); they are converted in phases 11 and 7.

The Morrowind reference, `Morrowind.esm`, esmtool 2026-09-27: `LEVI` 227; flags 0x1 "Each" in 136, 0x3 in
67, 0x2 "AllLevels" in 11, 0 in 13 — that is, Morrowind's bits are **swapped** relative to Oblivion.
Vanilla itself puts coins in lists: `Gold_001` 6, `Gold_005` 2, `Gold_010` 4, `Gold_025` 5, `Gold_100` 8.

Rules (the owner's decisions of 2026-09-27):

| Oblivion | Morrowind | Rule |
|---|---|---|
| `LVLI` | `LEVI` | ID by the common `ob_…` scheme; entries are translated into IDs once the whole plugin is read |
| `LVLF` 0x01 / 0x02 | flags 0x2 / 0x1 | by meaning, the bits are swapped |
| `LVLD` | `NNAM` | as is; above 100 — 100 with a note (5 lists) |
| entry count > 1 | — | a `LEVI` entry has no count: **1**, a note per list |
| `Gold001` ×N in an entry | `Gold_100`/`025`/`010`/`005`/`001` | the largest coin ≤ N, capped at 100; inexact — a note |
| `Gold001` everywhere | `Gold_001` | no own record is written (like soul gems); in `CNTO` the count is kept exactly. Recognised by `FormID`, so in mods too |
| `LVLF` 0x04 | — | a container referring to the list directly gets its entries (count × entry count, the sign is kept); inside another list — a normal pick, a note |
| entry without a record | — | dropped; an emptied list is not written, references to it go in a cascade (also from other plugins of the run) |

Measured 2026-09-27, test `statics::every_leveled_list_of_oblivion_esm_is_written_and_read_by_esmtool`:

| Parameter | Value |
|---|---|
| `LEVI` written, `esmtool` reads | 1,397 of 1,397 (1 empty one not written) |
| Entries | 16,266, of them nested lists 3,471; without an ID 8 (1 hood, 7 references to an empty list). With hood helmets (T9.2f): 16,267 and 7 |
| Lists where count → 1 | 63 |
| Lists with inexactly rounded gold | 52; coins in entries: `Gold_100` 408, `Gold_025` 230, `Gold_010` 55, `Gold_001` 23, `Gold_005` 19 |
| Chance clamped to 100 | 5 |
| Broken references from lists and containers | 0 |

`back2mw all --plugin Oblivion.esm --worldspace SEWorld` (2026-09-27): `LEVI` 696, `esmtool` and `niftest`
clean. Back then all lists were taken but only items placed in the world, so 12,198 entries had no ID and
702 lists ended up empty. Since 2026-10-29 the lists and items reachable from placed containers and
plants and from NPC and creature inventories are taken (`transform::world::contents_of`): `LEVI` 574,
1 entry without an ID, 1 empty list, nothing removed from containers; `esmtool` and `niftest` clean.

**Not verified in the game:** that a picked-up `Gold_025` becomes 25 × `Gold_001` (as in OpenMW); drops
from lists in chests.

---

### Exterior world (phase 3, specs `records/exterior.rs`, `records/land.rs`, traversal `world_tree.rs`)

Layouts — OpenMW `components/esm4/load{wrld,cell,refr,regn,ltex,land}.cpp`; OpenMW does not parse `WTHR`,
`CLMT` and `RDWT`, their layout was measured on `Oblivion.esm` on 2026-09-25.

**Group order** (checked on `SEWorld`): the top-level `WRLD` group → the `WRLD` record → right after it the
group of its children (type 1, label — the worldspace `FormID`) → the persistent cell and blocks (type 4)
→ sub-blocks (type 5) → a `CELL` and right after it the group of its children (type 6, label — the cell
`FormID`) → persistent (type 8), temporary (type 9) and visible-when-distant (type 10) children. `LAND` is
among the temporary ones, one per cell. We link a cell's children by the group label, not by adjacency.
`SEWorld` groups: type 4 — 8, type 5 — 76, type 6 — 4,022, type 8 — 1, type 9 — 4,022, type 10 — 934.

| Record | Field | Content |
|---|---|---|
| `WRLD` | `WNAM` | parent: whose landscape it uses (cities inside `Tamriel`: `BravilWorld` has its own `LAND` in 47 of 81 cells) |
| `WRLD` | `CNAM` | climate `CLMT`. `Tamriel` has none — the weather there is set by regions |
| `WRLD` | `DNAM` | default land and water levels. `SEWorld` has none, OpenMW then takes 0 |
| `WRLD` | `ICON`, `MNAM` | the drawn world map sheet and the cells it covers — see "World map" below |
| `CELL` | `XCLC` | x, y i32 (+ u32 quadrant hiding flags, 12 bytes). In all 4,187 cells of `SEWorld` |
| `CELL` | `XCLR` | an array of region `FormID`s (1,865 cells of `SEWorld`) |
| `CELL` | `XCLW` | f32 own water level: in 39 cells of `SEWorld` (3,000, 6,600, 5,000 …), 4,148 have none |
| `REFR` | `NAME`, `DATA` | the base; position xyz + angles xyz, f32, radians — the same `ESM::Position` as in TES3 |
| `REFR` | `XSCL`, `XESP`, `XTEL`, `XMRK` | scale; enable parent (`FormID`, u32 flags, 0x1 — inverted); teleport; map marker |
| `REFR` | header flag 0x800 | initially disabled |
| `LTEX` | `ICON` | path relative to `Textures\Landscape\` (in all 229) |
| `REGN` | `RCLR` | map color, RGBA |
| `REGN` | `RDAT` | type (u32), flags (u8, 0x1 "override"), priority (u8), 2 bytes. The meaning of the next field is set by the nearest `RDAT` before it |
| `REGN` | `RDWT` | after an `RDAT` of type 3: an array of **8**-byte items — `WTHR`, u32 chance. The sum need not be 100 (`SENSSouthweather` — 80) |
| `REGN` | `RDMP` | after an `RDAT` of type 4: the map name |
| `WTHR` | `DATA` | 15 bytes in all 37; byte 11 is the class: 0x01 clear, 0x02 cloudy, 0x04 rain, 0x08 snow. Oblivion-plane weathers have 0, `CamoranWeather` 0xC1. Fog and overcast are also 0x02, thunder 0x04: one class is not enough for a Morrowind type, a table is needed |
| `CLMT` | `WLST` | an array of 8-byte items — `WTHR`, i32 chance |

All specs read all records of `Oblivion.esm` without error (2026-09-25, `tes4_read/tests/real_world.rs`): 84
worldspaces, 33,639 `CELL`, 31,823 `LAND` (as many as a flat traversal finds), 706,146 `REFR`, 229 `LTEX`,
211 `REGN`, 37 `WTHR`, 19 `CLMT`.

**`SEWorld`** — the Shivering Isles. In this build they are merged into `Oblivion.esm`, and
`DLCShiveringIsles.esp` (both in the test data and in the owner's Steam folder) is an 85-byte stub: only
the header and the `Oblivion.esm` master, no records (2026-09-25). 4,187 cells x −32…31, y −44…37, 4,021
`LAND`, 61,280 `REFR`, 52 regions. 6 regions have weather, 22 have a map name. Placements by base type:
`STAT` 45,602, `TREE` 8,593, `FLOR` 5,080, `ACTI` 815, `LVLC` 638, `CONT` 187, `MISC` 101, `DOOR` 74,
`FURN` 67, `LIGH` 44, the rest — dozens. Disabled 45, with an enable parent 145, scale 0.06…8.41 (outside
0.5…2.0 — 1,461).

### LAND (T3.5)

`VHGT` — 1096 bytes: f32 offset, 33×33 i8 deltas, 3 bytes. The encoding is the same as in TES3 (a chain
down the left column, then along the row). `VNML`, `VCLR` — 3,267 bytes each (33×33×3). `BTXT` and `ATXT`
— 8 bytes each: `FormID`, quadrant u8, a byte, u16 (in `ATXT` — layer number 0…7). `VTXT` — a multiple of
8: u16 position in the 17×17 grid (`y·17 + x`), 2 bytes, f32 opacity; it belongs to the nearest `ATXT`
before it; an `ATXT` without a `VTXT` occurs. `DATA` in `SEWorld`: 0x19 (2,401), 0x1D (344), 0x1F (1,276);
1,276 have `VCLR`. A quadrant without `BTXT` gets the texture `terrainhddirt01.dds` (OpenMW).

**Orientation — measured, on two independent grounds** (2026-09-25, `transform/tests/real_land.rs`):

- `VHGT` rows grow to the north, `x` to the east. Stitching 2×2 without resampling: on `SEWorld`, 994 of
  1,004 complete quads stitched without a single mismatch; the control experiment (west ↔ east) breaks
  747. On `Tamriel` — 3,626 of 3,628, the control breaks 3,538;
- **quadrants: 0 — south-west, 1 — south-east, 2 — north-west, 3 — north-east.** This is what OpenMW says
  (`loadland.hpp`), and the data say the same: the opacity of one texture on the shared edge of
  neighboring quadrants matches at 54,489 samples against 4,785 (`SEWorld`) and 486,531 against 65,777
  (`Tamriel`); the best of the other 23 permutations — 28,749 against 45,861.

**Cracks in the source.** The edges of neighboring cells do not always match: on `SEWorld` 26 pairs of
neighbors of 7,914 diverge, on `Tamriel` 12 of 29,109. On `Tamriel` it is 1–8 vertices by 8…56 units. On
`SEWorld` it is a diagonal of cells at the sea floor in the south-west corner of the world (x −32…−9, y
−32…−17), a divergence of exactly 1,032 units. There a pit cell at −2,056 with edges raised to −1,040 sits
next to a floor at −8: a drop of 2,048 units across the edge. TES3 has one height per shared vertex and a
step limited to ±1,016 units, so (`transform::land`):

- at a divergence the cell written first wins — the southern one, then the western one; no averaging;
- if the heights still cannot be encoded after that, encoding saturates: the delta is clamped to ±127,
  the next vertex is computed from the restored one, the cliff becomes the steepest possible slope and
  catches up with the source within a few vertices. On `SEWorld` this is 202 deltas.

### Exterior world → Morrowind (`transform::exterior`, `transform::weather`, `data/weather.tsv`)

| Oblivion | Morrowind | Rule |
|---|---|---|
| 2×2 `CELL` + `LAND` | `CELL` + `LAND` | target `(X, Y)` from `(2X…2X+1, 2Y…2Y+1)`, division rounding down; plus the offset. Cells without `LAND` at the edge: vertices continue the nearest edge |
| `VNML`, `VCLR` | `VNML`, `VCLR` | stitched the same way; no normals — (0, 0, 127), no colors in a part — white; `VCLR` is written if at least one of the four has it |
| `BTXT`/`ATXT`/`VTXT` | `VTEX` | per 512-unit patch — the texture with the largest visible share (`dominant_texture`): a layer with opacity `a` covers the ones below by `a`, the base's share is what stays uncovered, one texture in the base and in layers is one sum, on a tie — the one painted later. Blending is lost. `SEWorld` measured 2026-10-02 (`real_land`, `land_texture_patchiness_of_seworld`): 7,215 patches of 258,304 match none of their neighbors, 54 cells have ten or more textures; with the plain sum of a layer's opacity (the rule before 2026-10-02: the base lost to any stroke) it was 8,791 and 141 |
| single patch | `VTEX` | after the textures are chosen — a pass over the whole world (`land::absorb_lonely_patches`, the owner's decision of 2026-10-02): a patch whose texture matches none of its four side neighbors (over cell borders; a neighbor without landscape does not count) takes the most common texture of those neighbors, on a tie the one more common among the eight neighbors, then the first in the order west, east, south, north. Cells are walked in key order, rows from the south, and replacements are visible to the following patches — after one pass no single patches remain. `SEWorld` (`real_land`, 2026-10-02): 6,014 patches replaced; borders between different textures 61,321 → 45,120 of 484,320 pairs, cells with ten or more textures 54 → 16; inside cells 688 patches match no neighbor — they continue a spot of the next cell. The `LTEX` list does not change (43) |
| `LTEX` | `LTEX` | only the used ones; index 0 — `ob_DefaultLand` (`terrainhddirt01.dds`), then by `FormID`. Path `ob\<name>_<hash>.dds` relative to `Textures\` |
| `REGN` with weather | `REGN` | a cell has one region: with weather, the highest `RDAT` priority, then the number of cells, then the lower `FormID`. The region's name is the worldspace name, the color is `RCLR` |
| worldspace climate | `REGN` | a region for cells without a region with weather |
| `WTHR` | weather type | by `data/weather.tsv` (all 37 of `Oblivion.esm`), otherwise by class. Chances are brought to a sum of 100 (largest remainder). The blight storm is not used: it infects the player |
| `REGN` `RDMP` / `CELL` `FULL` | cell name | the most common among the four |
| `REFR` | a reference in a cell | only converted objects; position + `dx·8192`, `dy·8192`; angles as is; scale clamped to 0.5…2.0. Not placed: disabled ones (themselves or through a parent, as in OpenMW), markers (`XMRK`; a `marker*` or `*lod.nif` model). The reference number — by `FormID` order, unique per plugin |
| `XCLW` | — | Morrowind's exterior water is always at zero |
| child `WRLD`s (`WNAM` on this one) | placements in the same cells | cities; see "Cities: child worldspaces" below |
| `XTEL` doors | `DOOR` with `DODT`/`DNAM` | see "Interiors" below; those leading to another worldspace — no destination |

A run `back2mw all --worldspace SEWorld` (release, 6 s with an unpacked cache, 2026-09-25): offset (48, 3),
Morrowind cells x 32…63, y −19…21. `SEWorld.esp`: `CELL` 1,065, `LAND` 1,009, `LTEX` 43, `REGN` 7,
references 51,543; STAT objects 301, items 65, world objects 167 and so on, 458 meshes, 38 land textures.
`esmtool` and `niftest` clean. Not placed: `TREE` 8,593 (T2.4), `LVLC` 634 (phase 11), objects with an
unconverted mesh — `STAT` 152, `ACTI` 96, `CONT` 9, `LIGH` 7 (particles, NIF 10.x — T2.3b), `SBSP` 6, `SOUN`
1; disabled 48, markers 189; scale clamped in 1,434; teleports without a destination 65; the water level
lost in 39 cells; cracks at the edges 215 vertices, of them saturation — 202 deltas. Density: at most 493
references per cell, 95 % of cells up to 289; `Morrowind.esm` has 420 and 230.

### World map (`MapSize` in `records/exterior.rs`, `transform::world_map`, `back2mw::world_map`)

Oblivion's drawn world map is an ordinary texture named in the `WRLD` record (measured on 2026-10-02).

| Field | Content |
|---|---|
| `ICON` | the sheet's path relative to `Textures\`. In `Oblivion.esm` five worlds have it: `Tamriel` (`Menus\Map\world\Cyrodiil_Resized.dds`) and four Shivering Isles worlds sharing one sheet `Menus\Map\world\TES4SEmap.dds` and the same `MNAM` — `SEWorld`, `SETheFringeOrdered`, `SEManiaGarden`, `SEDementiaGarden` |
| `MNAM` | 16 bytes: the "usable" width and height of the sheet in pixels (u32, u32), the north-west cell x, y (i16, i16), the south-east cell x, y (i16, i16). Test worlds hold all zeros; child worlds (cities) have no such field |

| World | `MNAM` | Sheet file |
|---|---|---|
| `Tamriel` | 2055×1830, cells (−59, 47) … (59, −58) | DXT5 2048×2048, no mipmaps; the bottom ≈ 380 rows are black |
| `SEWorld` | 1024×1024, cells (−32, 33) … (31, −29) | DXT5 1024×1024, no mipmaps |

**How the sheet lies on the cells.** Measured by overlaying: the world's `LAND` vertices above and below
zero (land and water) are projected onto the sheet, and the mapping is searched for that makes the
sheet's brightness under land and under water differ the most (with a local script that is not in the repository).

| World | Best mapping | Search step |
|---|---|---|
| `SEWorld` | left edge x = −32.0, 64.0 cells wide; top edge y = 34.9, 64.0 cells high | 0.1 cell |
| `Tamriel` | left edge x = −58.75, top y = 47.75, 17.3 file pixels per cell on both axes | 0.25 cell, 0.1 pixel |

Hence the rule (`transform::world_map::sheet_rect`):

- cells on the sheet are **square**; pixels per cell is the "usable" width divided by the number of cells
  from the western to the eastern one **inclusive** (`SEWorld`: 1024 / 64 = 16; `Tamriel`: 2055 / 119 =
  17.27);
- the left edge of the file is the western edge of the north-west cell;
- the "usable" height is counted upwards from the **southern edge of the south-east cell** — that gives
  the top edge of the file. The northern cell from `MNAM` does not work for this: for `SEWorld` it is
  y = 33 (the top edge would be 34), while the sheet ends at y = 35 — read "by `MNAM`", the coast on the
  sheet and in the world is a cell apart; for `Tamriel` both ways give 47.97 ≈ 48;
- the file is **not stretched** to the "usable" size: `Tamriel`'s is 2048×2048, and the same 17.27 pixels
  per cell continue right and down past the "usable" 2055×1830 (fitting with the file stretched to
  2055×1830 matches noticeably worse).

How Oblivion itself places the player marker on the sheet was not investigated: what is needed is the
mapping to the drawing, not the engine's formula.

**What the converter writes** (`--lua-map`): the sheet is copied as is into
`Textures\ob_<path joined by _>.dds` (the same naming scheme as mesh textures), the description goes to
`MWSE\mods\back2mw\map\worlds\<EDID>.json`:

```json
{
  "format": 1,
  "world": "SEWorld",
  "name": "Realm of Sheogorath",
  "plugin": "SEWorld.esp",
  "texture": "Textures\\ob_menus_map_world_tes4semap.dds",
  "width": 1024,
  "height": 1024,
  "cells": { "minX": 32, "minY": -19, "maxX": 63, "maxY": 21 },
  "sheet": { "left": 262144, "bottom": -94208, "right": 524288, "top": 167936 }
}
```

`width`, `height` — the file's size in pixels; `cells` — the Morrowind cells the world occupies (the mod
uses them to decide whether to show the sheet); `sheet` — the rectangle of the Morrowind world, in units,
that the file covers, offset already applied. The mod (`MWSE\mods\back2mw\map\main.lua`, source —
`crates/back2mw/src/world_map/main.lua`) reads only `format = 1`. How it works and the names of
Morrowind's map window elements — [tes3.md](tes3.md), "Map window".

### Cities: child worldspaces (`transform::exterior::merge_children`, test `transform/tests/real_cities.rs`)

An Oblivion city is a separate `WRLD` with `WNAM` pointing to the parent, at the same coordinates; the
engine switches between them through teleport gates (`XTEL`: the first 4 bytes are the `FormID` of the
door at the other end). Morrowind cannot switch, so `--worldspace` merges the children's placements into
the parent's cells.

Measured on `Oblivion.esm` (2026-09-26, `real_cities::seworld_cities`):

- `SEWorld`'s children: `SETheFringe`, `SENSBliss`, `SENSCrucible`, `SENSPalace`, `SEVitharnWorld`. Without
  a parent and therefore not merged: `SETheFringeOrdered`, the gardens `SEManiaGarden`/`SEDementiaGarden`,
  the test `SETest*`. `Tamriel` has 25 children — besides cities these are quest variants (`*MQ16`,
  `E3Kvatch`, `KvatchEast`/`Entrance`/`Plaza`, `TGTempleOfTheEmperorZero`, `EmptyWorld`): for `Tamriel`
  merging all of them is not allowed, the variants would stack on top of each other.
- **Duplicates.** 244 city placements are bit-exact copies of island ones (the same object, position,
  angles); another 18 are copies between cities (Bliss, Crucible, the Palace stand in the same cells). The
  nearest pair of one object with the same rotation that is not a copy is 18.3 units apart; the tolerance
  is 1 unit. 17 pairs stand at one point but are rotated by π (the halves of `0001ACFB`) — these are not
  copies, both stay. Nothing is dropped within one worldspace during the merge; what the author
  placed twice at one point of a cell (84 references in `SEWorld` with its interiors) is removed
  later, when references are placed — [tes3.md](tes3.md), "Repeated references".
- **Gates.** There are 16 doors between the merged worldspaces: 10 between the island and the cities
  (Bliss, Crucible, Vitharn — 2 each, the Fringe — 4) and 6 between Bliss, Crucible and the Palace (the
  Palace has no exit to the island). They are dropped at both ends. Doors to the outside (to `Tamriel`,
  `SETheFringeOrdered`, interiors) stay as doors without a destination.
- The city's **landscape** is not used. Cities have 39 `LAND`s, 37 of them with heights unlike the
  island's, up to 2,104 units at the edges (the Fringe), 608…912 in the others. City placements buried by
  100+ units are 62 under the island's ground and 72 under their own: the city's own land is no better,
  and the island's at least has no seams. Cells, regions, names and water also come from the parent.

A run `back2mw all --plugin Oblivion.esm --worldspace SEWorld` (release, 7 s with the cache, 2026-09-26):
53,387 references (262 duplicates dropped, 16 gates), `CELL` 1,065, `LAND` 1,009; `esmtool` and `niftest`
clean (621 meshes). Density: at most 594 references in cell (53, 5) — the center of New Sheoth, 95 % of
cells up to 292 (counting `FRMR` in `SEWorld.esp`).

### Interiors (`transform::interior`, traversal `world_tree::interior_cells`, test `transform/tests/real_interiors.rs`)

Interior cells live in the top-level `CELL` group: block (type 2) → sub-block (type 3) → a `CELL` and the
group of its children (type 6), as for exterior ones. `XTEL` in all 3,324 doors of `Oblivion.esm` is 28
bytes: the `FormID` of the door at the other end, the position and angles where the player appears.

| Oblivion | Morrowind | Rule |
|---|---|---|
| which `CELL`s | interior `CELL`s | reachable through doors from the worldspace (with cities) and further along the door chain |
| `FULL` (otherwise `EDID`) | `NAME` — the cell ID | unique case-insensitively: a repeat gets " 2", " 3" by `FormID`; at most 50 (the longest in vanilla is 50, `Bloodmoon.esm`) |
| `XCLL` ambient, directional, fog color | `AMBI` ambient, sunlight, fog color | as is, the fourth byte is 0 |
| `XCLL` fog distance | `AMBI` density | `1 − far/8192`, clamped to 0…1; `far` ≤ 0 — 0 (not verified in the game) |
| `DATA` 0x02 + `XCLW` | `HAS_WATER` + `WHGT` | without `XCLW` — 0 |
| `REFR` | reference | the same rules as outside, without the offset; the reference number continues the exterior ones |
| `XTEL` | `DODT` (+ `DNAM`) | if the door at the other end is outside — the exit position + offset, no `DNAM`; in an interior — as is, `DNAM` = the cell name; otherwise no destination |

Measured on `Oblivion.esm` (2026-09-26): 1,855 interiors, `XCLL` in 1,770, `REFR` 319,471, doors 3,324
(`real_world::every_exterior_record_of_oblivion_parses`). 162 are reachable from `SEWorld`
(`real_interiors::seworld_interiors`): `REFR` 45,965, the most in `XPXavara01`, 1,017; water in 21; `XCLL`
in all 162, fog off (`far` 0) in 62, median density 0.60. The names of 4 pairs of houses in Split (the
Mania and Dementia versions) coincided — the second ones became "… 2". Doors, if all objects were
converted: 451 with a destination, 8 without (to `Tamriel` and `SETheFringeOrdered`).

A run `back2mw all --plugin Oblivion.esm --worldspace SEWorld` (release, 9 s with the cache, 2026-09-26):
162 interiors, 94,523 references in total (41,137 in interiors), 439 doors with a destination, 7 without
(some doors were not placed: the mesh was not converted). 1,595 meshes, `esmtool` and `niftest` clean. In
`SEWorld.esp`: all 325 `DNAM`s lead to existing cells, every interior has a door, the names are unique;
the most references are in "Xavara" — 961.

**Not converted yet:** owners (`XOWN`, phase 7), NPCs and creatures (`ACHR`/`ACRE`, phases 7 and 11),
sounds (`SOUN`, 1,481 placements), "behave like exterior" — the interiors of `SEWorld` do not have the
0x80 flag. (NPCs came in T7.6 and placed sounds in T10.3, see below.)

### Actors (T7.1, spec `records/actor.rs`, test `tes4_read/tests/real_actors.rs`, table `data/skills.tsv`)

Layouts — UESP ("Mod File Format/CLAS, FACT, NPC_") and OpenMW (`esm4/loadnpc.cpp`, `actor.hpp`,
`loadrace.cpp`), checked on `Oblivion.esm` on 2026-09-27: all 15 `RACE`, 111 `CLAS`, 476 `FACT`, 2,482
`NPC_` are read (`real_actors::every_actor_record_of_oblivion_parses`).

| Record | Field | Layout |
|---|---|---|
| `RACE` | `DATA` 36 | 7 pairs (skill code u8, bonus i8; 255 — empty), 2 bytes, height m/f, weight m/f (f32), flags u32 (0x1 playable: the 10 ordinary races; GoldenSaint, DarkSeducer, Dremora, Sheogorath, VampireRace have 0) |
| `RACE` | `ATTR` 16 | 8 male attributes, 8 female (Strength … Luck) |
| `CLAS` | `DATA` 48 (31) / 52 (80) | major attributes 2 × u32 (0…7), specialization u32 (0 combat 53, 1 magic 30, 2 stealth 28), 7 skills u32 (codes 12…32), flags u32 (0x1 playable — 21, 0x2 guard — 6), services u32; the 52-byte one also has a training skill and limit u8 + 2 bytes |
| `FACT` | `DATA` 1, `XNAM` 8, `RNAM` 4 + `MNAM`/`FNAM` | flags (0x1 hidden — 336 of 476), relations (`FormID`, i32), ranks with titles; 392 have no ranks, the most is 10 |
| `NPC_` | `ACBS` 16 | flags u32, magicka u16, fatigue u16, barter gold u16, level/offset i16, calcMin u16, calcMax u16 |
| `NPC_` | `AIDT` 12 | aggression, confidence, energy, responsibility (u8), services u32, training skill and limit (u8), 2 bytes |
| `NPC_` | `DATA` 33 | 21 skills u8, health u32, 8 attributes u8 |
| `NPC_` | `SNAM` 8, `CNTO` 8 | faction + rank i8 + 3 bytes; item + count i32 |

Skill codes ("actor values") are 12…32 in the order of `SKILL_NAMES` (Armorer … Speechcraft), attributes
0…7; classes and races use no other codes (the same test).

`NPC_` numbers: women 778, essential 249, autocalc 2,008, "relative to the player's level" (0x80) 1,568 —
of them **1,436 have calcMax = 0** (no upper limit); offsets −10…+40, most often +10 (323), 0 (268), −2
(209). A fixed level in 914: 1…50 (and one 254). In several factions — 1,446; without a class — 0; with
barter or training services — 238.

**Races** (T7.2, `transform::races`, `data/races.tsv`): the ten common ones → vanilla records; GoldenSaint
(donor High Elf), DarkSeducer and Dremora (Dark Elf), Sheogorath and VampireRace (Imperial) — own `RACE`s
with copies of the donor's 24 body parts, 120 `BODY`s in total, 10 of them first-person hands
(2026-09-27, `real_races`). Skill bonuses of the own ones: 7 for the three daedric ones, 0 for Sheogorath
and VampireRace (all 7 slots are empty); one innate spell, in Dremora.

**Classes** (T7.3, `transform::classes`): fields as is, 7 major skills → 5 major + 5 minor via
`data/skills.tsv`. **For autocalculated NPCs the engine takes services from the class** (OpenMW
`mwclass/npc.cpp`, `getServices`; the vanilla engine — not verified), while in Oblivion they are in the
NPC's `AIDT` — so a class is written in a variant per NPC service set (`obs4001_Commoner_…`). The service
bits of both games match (weapons 0x1 … training 0x4000, repair 0x20000), except Oblivion's recharge
0x10000 — in Morrowind it is the enchanting bit, so recharge is dropped (11 NPCs). Over all 2,482 NPCs of
`Oblivion.esm`: 39 service sets, 2,246 NPCs without services; 172 classes in the output, 83 of them
service variants (2026-09-27, `transform/tests/real_actors.rs::classes_of_all_npcs`).

**Factions** (T7.4, `transform::factions`): only those a converted NPC belongs to; the name is `FULL`
(without it — `EDID`, and the faction is hidden); ranks — the male title, otherwise the female one,
otherwise the faction name, a faction without ranks gets one; relations ±100 → ±3 linearly with clamping
— vanilla keeps −3…3 (`Morrowind.esm`: 255 relations, only −8 and −4 are out of range; Oblivion — −200…100,
100 in 214 and −100 in 131 of 698). Over all NPCs of `Oblivion.esm` (2026-09-27,
`real_actors::factions_of_all_npcs`): 4,594 memberships, no negative ranks; 379 of 476 factions in the
output, 296 hidden, 26 with several ranks, 449 relations between them.

**NPCs** (T7.5, `transform::npcs`; the owner's decisions are in the internal planning notes): autocalc;
level — as is, "relative to the player" — the middle of calcMin…calcMax, without calcMax — calcMin +
offset, at least 1; faction — the first visible one with rank ≥ 0; head and hair — of the vanilla race (the
donor's for an own race) and sex, a variant by `FormID`; `base_animkna.nif` for beast races. `AIDT` →
fight 90 with aggression ≥ 50, otherwise 30; flee `80 − confidence`; alarm = responsibility; hello 30.
Wander — the radius of the first wander package (`PACK` type 5, radius `PLDT`), up to 2,000, otherwise 0;
duration 5 and idle — the most common in vanilla. **`PACK`**: `PKDT` 8 bytes (type — byte 4) in 6,648 of
7,209, 4 bytes (type — byte 2) in 561; `PLDT` 12 bytes (location type, target, radius). **`LVSP`**
(leveled spell lists) — the same layout as `LVLI`; in the NPCs of `Oblivion.esm` 1,430 of 3,687 `SPLO`s
lead to them; they are expanded by the NPC's level: with flag 0x4 — all entries not above the level,
otherwise one with the highest level (the lower `FormID`).

Over all 2,482 NPCs of `Oblivion.esm` (2026-09-27, `real_actors::every_npc_of_oblivion_converts`): all
build, all have a head, 129 distinct heads; levels — 1,408 below 5, 510 in 10…14; fight 30 in 1,808, 90 in
674; standing (wander 0) 1,120, wandering up to 2,000 — the rest.

**NPC placements** (T7.6): `ACHR` is read by the `REFR` spec (`Placement`) and placed by the same rules.
With `--worldspace`, the NPCs placed in the world and interiors are converted together with what they
refer to; the inventory is converted together with everything reachable through leveled lists — Oblivion's
equipment is almost all in lists (`LL0NPCArmorHeavyBoot` …), and without their contents NPCs would be naked
(275 inventory entries in `SEWorld`). A `SEWorld` run (release, 16 s with the cache, 2026-09-27): `NPC_`
307, own `RACE`s 3 (72 `BODY`), `CLAS` 46, `FACT` 45; 285 references placed for 216 NPCs (the others are
initially disabled); `WEAP` 418, `ARMO` 187, `CLOT` 320 (were 59, 58, 100 — equipment); 54 spells cleaned
out of NPCs (scripted effects), 0 items; `esmtool` reads all records (an `NPC_` check was added to
`validate`), `niftest` clean on 2,270 meshes.

**Skills 21 → 27** — `data/skills.tsv`: a main Morrowind skill for each Oblivion skill plus the additional
Short Blade, Spear (from Blade), Axe (Blunt), Medium Armor (Heavy Armor), Enchant (Mysticism), Unarmored
(Acrobatics). The same table translates the skill of skill books and effects (previously a constant in
`transform::items`, which matched the main skills of the table).

### Locks (T6.4, `Lock` in `records/exterior.rs`, `transform::exterior::lock_level`)

`XLOC` on door and container `REFR`s: level (u8, 0…100, **100 — key only**), 3 bytes of junk, the key's
`FormID` (`KEYM`, 0 — none), flags — the last 4 bytes, 0x4 — "leveled" (difficulty scales with the
player's level). Source — UESP "Oblivion Mod:Mod File Format/REFR"; OpenMW (`loadrefr.cpp`) reads the same
level and key. From `Oblivion.esm` (2026-09-27, a script over the file):

- 2,866 locks: 12 bytes in 2,841, 16 in 25 (bytes 8 and 12 are zero in them — the data does not tell where
  the flags are); on `CONT` 1,547, on `DOOR` 1,319; 646 with a key, all keys are `KEYM`.
- Leveled 1,790; 1,119 of them have a base level of 0…9. Not leveled 1,076: level 100 in 351.
- Any levels 0…100 occur, most often 0 (871), 100 (353), 7 (347), 20 (334).

| Oblivion | Morrowind | Rule |
|---|---|---|
| level | `FLTV` | as is; 0 → 1 (in Morrowind 0 without a key means unlocked, OpenMW `esm3/cellref.cpp`); leveled — plus 30, clamped to 1…100 (the owner's decision of 2026-09-27); 100 stays 100 — the hardest vanilla level (the owner's decision) |
| key | `KNAM` | the ID of the converted `KEYM` (a `MISC` with the "key" flag); the key is converted even if it is not placed in the world. Not converted — a lock without a key, with a note |

Vanilla (`Morrowind.esm`, 2026-09-27): `FLTV` on 1,681 references, `KNAM` on 420; levels 0…100, 0 in 226
(with a key — locked, without — an open lock), 100 in 43.

A `SEWorld` run (2026-09-27, release): 178 references locked, 72 with a key, 0 keys without translation;
by `esmtool dump -C`: levels 1…100, 49 distinct `KNAM`s, all are `MISC`s of the plugin with "Is Key: 1".

### Path grids (T4.1, spec `records/pathgrid.rs`, `transform::pathgrid`, test `transform/tests/real_pathgrid.rs`)

A `PGRD` is a child of a cell (in the group of its children, like a `REFR`), at most one per cell (in
which subgroup — persistent or temporary — was not checked). The layout was measured on all 8,228 `PGRD`s
of `Oblivion.esm` (2026-09-27, a script over the file) and matches OpenMW
(`components/esm4/loadpgrd.cpp`):

| Field | Size | Content |
|---|---|---|
| `DATA` | 2 | the number of points (u16); in all 8,228 |
| `PGRP` | 16 × points | x, y, z (f32) — **world** coordinates (for an interior — the cell's); number of links (u8); 3 bytes of junk (not zeros) |
| `PGAG` | 1…118 | a bit mask; OpenMW skips it, and so do we |
| `PGRR` | 2 × sum of links | neighbor indices (u16) in a row, each point as many as in its `PGRP`; **0xFFFF — an empty slot** |
| `PGRI` | 16 × n | a link to a point of a neighboring cell: own point (u16), 2 bytes of junk, x, y, z of the foreign point (f32, world) |
| `PGRL` | 4 + 4 × n | links of points to an object (the reference `FormID` and point numbers) — Morrowind has no counterpart |

Numbers from `Oblivion.esm`: up to 943 points in one grid (an interior); 47 grids have no `PGRR` (no
links), 6,509 have `PGRI`, 480 have `PGRL`. Empty 0xFFFF slots — 92,115 of 2,887,675 in 4,351 grids;
there are no other out-of-grid indices, no loops, and apart from empty slots **all links are written at
both ends** and are not repeated. OpenMW skips 0xFFFF (`endNode == -1`) the same way.

**The edge.** Points also sit exactly on cell borders (x = 4096·k), and a neighbor's `PGRI` points to such
a point. By coordinate (`floor`) it belongs to the eastern/northern cell, but it is owned by the
western/southern one — so the point at the other end is searched for in all grids of the worldspace, not in
the cell by coordinate. "By coordinate" gives the wrong cell for 19,219 links of `Tamriel` and 3,008 of
`SEWorld` (`real_pathgrid`).

**Conversion** (`transform::pathgrid`): the grids of four cells (and city grids, T3.9) are added into one;
coordinates — world minus the south-west corner of the target cell before the offset, rounded to an
integer; a `PGRI` link becomes an edge if the point at the other end is found within 1 unit and its grid is
in the same Morrowind cell, otherwise it is lost; edges at both ends, no repeats. Interiors — as is.

Measured 2026-09-27 (`real_pathgrid::path_grids_of_seworld_and_tamriel`, offset (48, 3)):

| | `SEWorld` | `Tamriel` |
|---|---|---|
| source `PGRD` outside (with cities) / in interiors | 1,040 / 162 | 5,018 / 1,271 |
| output `PGRD` outside | 275 | 1,324 |
| points per grid outside — maximum; grids above 246 (the vanilla maximum) | 567; 133 | 1,220; 880 |
| points per interior — maximum; above 246 | 557; 33 | 943; 258 |
| points in total | 86,034 | 536,286 |
| `PGRI` links → edges / into another Morrowind cell (lost) / without a pair | 33,754 / 35,762 / 1,460 | 101,239 / 105,362 / 295 |
| target matched a point bit for bit / within 1 | 69,511 / 5 | 206,507 / 94 |
| outside points out of 0…8192 after translation | 0 of 64,036 | 0 of 373,824 |

The control experiment: the source world coordinates fall into 0…8192 for 356 and 437 points — the range
check fails on untranslated data. The links without a pair in `SEWorld` were checked by a script: no
worldspace of the file has a point within 1 unit of the target (in the examples the nearest is 44…91
away), that is, the links dangle in the data itself; this was not checked for `Tamriel`. No point has more
than 255 neighbors.

A run `back2mw all --config dist/config.toml --worldspace SEWorld` (plugin `Oblivion.esm`, release, 26 s
with the cache, 2026-09-27): `PGRD` 437, points 86,034; `validate` clean (`esmtool`, `niftest`, 1,748
meshes); `esmtool dump -t PGRD` reads all 437: `Granularity` 1024, all points have `Auto-Generated` 1, no
empty names.

### Sounds (T10.1, spec `records/sound.rs`, `transform::sounds`, `mesh_convert::audio`, test `back2mw/tests/real_sounds.rs`)

`SOUN`: `EDID`, `FNAM` — a path relative to `Sound\`, `SNDX` 12 bytes (or the old `SNDD` 8 bytes):

| Offset | Type | Field |
|---|---|---|
| 0 | u8 | min — distance without attenuation, ×5 units |
| 1 | u8 | max — audibility limit, ×100 units |
| 2 | i8 | frequency shift, % |
| 3 | u8 | unused |
| 4 | u32 | flags: 0x1 random frequency, 0x2 play at random, 0x4 no environment, 0x8 random location, 0x10 loop, 0x20 menu, 0x40 2D, 0x80 LFE |
| 8 | u16 | static attenuation, hundredths of a dB (`SNDX` only) |
| 10 | u8, u8 | stop and start time (`SNDX` only) |

Measured on `Oblivion.esm` (2026-09-28): 1,140 `SOUN`, `SNDX` in 1,138, `SNDD` in 2; all have `FNAM`: in
618 — a `.wav` file, in **522 — a folder** (ending with `\`, the engine takes a random file from it).
Attenuation — from 0 to 3,480 in steps of about 44 (≈0.44 dB), two have 10,000 (silence). min 0 in 241,
max 0 in 239. Loop (0x10) — in 194.

`SOUN` is referenced by: `DOOR.SNAM` 417 (open), `DOOR.ANAM` 406 (close), `DOOR.BNAM` 20 (loop),
`LIGH.SNAM` 75, `ACTI.SNAM` 50 (loop), `CONT.SNAM`/`QNAM` 866/865, `REGN.RDSD` 2,395, `WATR.SNAM` 18,
`CREA.CSDI` 669, scripts 133 and **placements** — `REFR` to a `SOUN`, 5,938 (1,481 in `SEWorld`, T10.3).

Files (`real_sounds`, 2026-09-28, `Oblivion - Sounds.bsa` + `DLCShiveringIsles - Sounds.bsa`, 2,093 `.wav`):
**all 16-bit PCM**; mono 1,860 (44,100 Hz 1,377, 22,050 280, 32,000 170, 11,025 30, 16,000 3), stereo 233
(44,100 122, 22,050 96, others 15). Every `SOUN` folder has at least one `.wav`. **33 `SOUN`s have no file**
in the archives or loose — leftovers in `Oblivion.esm` itself (`E3DremoraDeathSound`, `BlockLight` →
`FX\Combat\TES4-Combat-BlockLight.wav` and the like).

**Conversion** (`transform::sounds`, the decisions are in the internal planning notes):

- Only `SOUN`s referenced by an object of the same plugin are written (like `ENCH`); the ID by the scheme
  `ob_<EDID>_<base36>`. A reference to another plugin's sound — the object has no sound, a note.
- Volume = 255 · 10^(−attenuation/2000); min = min·5/20, max = max·100/50 (the multipliers of
  `Morrowind.esm`, `formats/tes3.md`, "Sounds"), 0/0 → 0/0 (the engine defaults). The frequency shift and
  the flags are not carried.
- The path — `ob\<name>_<hash>.wav` by `FNAM` (`paths::sound_path`); a folder — the first `.wav` by name
  (loose files and all archives, `extract::list_directory`).
- Stereo → mono by averaging the channels, sample rate as is; mono is copied byte for byte.
- `DOOR.SNAM`/`ANAM` → open/close sounds, `LIGH.SNAM` → the light's sound. `CONT` — Morrowind has no
  field (`sounds_dropped`). A `SOUN` without a file is not written; doors and lights with it stay without
  a sound.
- **Loops and placed sounds** (T10.3, decision 3 and the owner's decision of 2026-09-28): `ACTI.SNAM` → the
  activator's own `SCRI` with a loop script `obls_<…>` (`PlayLoopSound3DVP`, volume and pitch 1.0; the
  template and the byte code check — `formats/tes3.md`, "Scripts"), one script per sound. A `REFR` to a
  `SOUN` → a reference to an invisible `ACTI` `obsm_<…>` (no name, the vanilla `EditorMarker.nif`) with
  the same script. A sound without a file — the script is not written, the activator and the marker stay
  without `SCRI`.

A run `back2mw all --config dist/config.toml --plugin Oblivion.esm --worldspace SEWorld` (release,
2026-09-28): `SOUN` 38, files 38 (stereo — 0), 50 of 68 `DOOR`s have an open sound, 49 a close sound, 12 of
213 `LIGH`s have a sound; `CONT`/`ACTI` sounds without a field — 425; `validate` clean (`esmtool`,
`niftest`, 2,856 meshes).

The same run after T10.3 (2026-09-28): `SOUN` 216 (was 152 with regions), files 178 (stereo 41); a sound
loop on 19 `ACTI`s (9 scripts); placed `SOUN`s — 65, that is 65 markers and 1,481 references (1,442 in
interiors, in 100 cells in total); `SCPT` 70 (4 sounds shared by markers and activators); `CONT` sounds
without a field — 406; `validate` clean. All `SCPT`s were checked by parsing the file: `SCDT` length = 46 +
2·sound ID length, the sound is among the plugin's `SOUN`s, every `SCRI` leads to an `SCPT`.

### Region sounds (T10.2, `RegionSound` in `records/exterior.rs`, `transform::sounds::region_sounds`)

`RDSD` comes after an `RDAT` of type 7; an array of 12-byte items: `SOUN` (u32), weather flags (u32: 0x1
clear, 0x2 cloudy, 0x4 rain, 0x8 snow), chance (u32, **hundred-thousandths of a percent**).

Measured on `Oblivion.esm` (2026-09-28, a script over the file): 127 `RDSD`s, all lengths multiples of 12,
2,395 sounds (0…32 per region, most often 14…26). Flags: 15 (any weather) in 1,467, 11 (except rain) in
876, the rest — single ones. **A chance of 10,000,000 (100 %) — exactly 520 sounds, and all of them are
loops** (`SOUN` with 0x10); another 34 loops have chance 0. Random sounds have a chance of 0…4,500,000
(0…45 %), and **their sum per region is about 100 %** (median 99 %, from 0 to 195 %): these are selection
weights, not probabilities. In vanilla Morrowind the sum of chances per region is 20…103, most often 55
(`formats/tes3.md`, "Sounds"). Region sounds are almost always a folder (2,314 of 2,395): 4…214 files per
region, median 126.

**Conversion** (the owner's decisions of 2026-09-28):

- A Morrowind region takes the sounds of its source region (the one chosen by weather priority); a
  climate region has no sounds.
- Chance 0 and a folder without files are not converted. A loop is an ordinary sound with a weight equal
  to the average weight of the region's random sounds. Weights are normalised to a sum of 55, each at
  least 1.
- A folder gives no more variants than the sound's chance: the first is the main `SOUN` (the folder path,
  the first file), the following are `obr<k>_…` for the next files by name, the chance split equally, the
  remainder to the first. The `SOUN`'s weather flags and time of day cannot be expressed in Morrowind.
- A `SOUN` without a file (see above) also leaves the region lists.

A run `back2mw all --config dist/config.toml --plugin Oblivion.esm --worldspace SEWorld` (release,
2026-09-28): sounds in 3 of 7 `REGN`s (the other four have none in the source either), entries 52 / 52 /
56, chance sums 56 / 54 / 56; `SOUN` 152 in total, files 116 (different Oblivion `SOUN`s share a folder),
32 converted from stereo to mono; `validate` clean. **Vanilla has at most 27 entries per region** — a long
list is to be checked in the game and the CS.

### Dialogue (T10.5, spec `records/dialogue.rs`, traversal `dialogue_tree.rs`, `transform::dialogue`, `data/conditions.tsv`, test `transform/tests/real_dialogue.rs`)

**The tree.** The top-level `DIAL` group → a `DIAL` record → the group of its lines (type 7, label — the
topic `FormID`) with `INFO`s. Lines are linked to a topic by the group label. 3,053 line groups for 3,817
topics: some topics have no lines.

**Layout** (measured on `Oblivion.esm` 2026-09-28, a one-off script over the file):

- `DIAL`: `EDID` (missing in 3), `FULL` (missing in 4), `QSTI` ×N (4,372 for 3,817 topics), `DATA` 1 byte —
  type: 0 Topic 3,184, 1 Conversation 555, 2 Combat 16, 3 Persuasion 39, 4 Detection 4, 5 Service 14,
  6 Misc 5.
- `INFO` (19,278): `DATA` 3 bytes — type, next speaker, flags (2 records have 2 bytes, no flags); exactly
  one `QSTI`; `NAME` ×N — add topics; a response is `TRDT` 16 bytes (emotion u32, strength i32, 4 empty,
  number u8, 3 empty) + `NAM1` text + `NAM2` actor note, 23,877 responses; `CTDA` 48,531 (24 bytes) and the
  old `CTDT` 72 (20 bytes); `TCLT` 9,698, `TCLF` 4,141; the result is `SCHR` 20 (the old `SCHD` 24/28 in 47)
  + `SCDA` + `SCTX` (5,718) + `SCRO`. **No line has a `PNAM`** — the order is set by the file order.
- `QUST` (390, 208 start with the game): `DATA` 2 bytes — flags (0x01 start game enabled), priority; the
  `CTDA`s before the first `INDX` are the quest's conditions, they apply to all its lines; the `CTDA`s after
  `INDX` belong to the stage journal entries.

**A condition** `CTDA`: byte 0 — the high 3 bits are the operator (0 `==`, 1 `!=`, 2 `>`, 3 `>=`, 4 `<`,
5 `<=`; 6 and 7 do not occur — a parse error), the low bits are flags (0x01 "or" with the next, 0x02 on the
target, 0x04 compare with a `GLOB`); 4 — an f32 number or a `GLOB` `FormID`; 8 — the function number; 12,
16 — parameters. **The function number is checked against byte code**: in scripts a call is opcode
`0x1000 + number`, and over 9,646 `SCDA` ↔ `SCTX` pairs the word and the opcode match in all scripts for 1,
14, 18, 45, 47, 48, 50, 56, 58, 59, 67, 69, 71–73, 76, 77, 80, 84, 116, 125, 130, 161, 193, 249, 365 and
others. Numbers without calls in scripts are confirmed by parameter type (68 → `CLAS`, 74 → `GLOB`, 132 →
`FACT`). The nine numbers with neither check (60–63, 66, 75, 133–135) and all other rows of
`data/conditions.tsv` were checked on 2026-10-01 against the engine's function table extracted from
`Oblivion.exe` (tes4skyrim, `tes4_export/oblivion_engine_tables.json`): the number, name, opcode and
parameter types matched in all 43 rows.

**"Or"** binds tighter than "and": `A or B, C` = `(A or B) and C`. The quest's condition list and the line's
list are grouped separately.

**Selection** (decision 1 of phase 10 and the owner's decisions of 2026-09-28): NPC-to-NPC conversations —
no; the line's quest starts with the game; conditions on the quest (`GetStage` 58, `GetStageDone` 59,
`GetQuestVariable` 79, `GetQuestRunning` 56) — no; comparing with a `GLOB` instead of a number — no; a
function without a filter — no. **`GetPlayerInSEWorld` (365)** is evaluated against the converted world
(`SEWorld` → 1, otherwise 0); **`GetIsPlayableRace` (254)** — "Not Race" filters; **"or"** is unfolded into
`INFO` copies (at most 8), an alternative without a filter falls away, and one evaluated as true makes the
whole group true. With `--worldspace`, a `GetIsID` on an NPC that is not converted rejects the alternative.

By the letter of decision 1 (without evaluating 365 and the 254 filters, "or" discards) only **194 lines**
remained — the 365 and 254 quest conditions are on `Generic`, `MQ00`, all `NQD*`, `ArenaDialogue`,
`MG00General` and others; the card's estimate (~2,170) did not take quest conditions into account.

Result (test `real_dialogue::dialogue_of_oblivion_parses_and_is_selected`, 2026-09-28; the Conversation row
and the total — 2026-10-01, after decision 16 of phase 10: `HELLO` → the `Hello` voice), lines / `INFO`
copies selected from all:

| Type | Total | Cyrodiil (no world) | `SEWorld` (307 NPCs) |
|---|---|---|---|
| Topic | 10,311 | 2,543 / 2,757 | 502 / 517 |
| Conversation | 7,772 | 448 / 512 (only `HELLO`) | 126 / 145 (only `HELLO`) |
| Combat | 632 | 230 / 271 | 51 / 68 |
| Persuasion | 130 | 95 / 95 | 0 |
| Detection | 65 | 10 / 10 | 2 / 2 |
| Service | 248 | 189 / 220 | 89 / 120 |
| Misc | 120 | 108 / 108 | 0 |
| **Total** | 19,278 | **3,623 / 3,973** | **770 / 852** |

For Topic without a world, these fell away: the quest does not start 4,702, conditions on the quest
2,429, another world 148, `GLOB` 62, more than 8 copies 6, no filter 421. For `SEWorld`, another world —
2,178 Topic, 318 Combat and all Persuasion/Misc: Oblivion's persuasion and service phrases belong only to
Cyrodiil's quests, the islands have none of their own. Frequent functions without a filter (lines without a
world / `SEWorld`): `IsGuard` 125 — 159 / 145, `GetScriptVariable` 53 — 130 / 35, `GetRandomPercent` 77 —
68 / 18, `GetTrespassWarningLevel` 144 — 61 / 40, `GetIsID` on the target — 56 / 8, `IsTrespassing` 145 —
39 / 38, `IsInMyOwnedCell` 146 — 33 / 33, `IsPlayerInJail` 171 — 30 / 28.

**Writing** (T10.6): `formats/tes3.md`, "Dialogue" — for `SEWorld`, 335 of the 502 Topic lines were
converted; "Not Race" does not use slots: the speaker is computed from the NPCs.

**Voice and service topics** (T10.7) are recognised by the topic's `EDID` (`Attack`, `PowerAttack`,
`Steal`, `InfoRefusal`, …; for persuasion the lines are in the minigame reactions `ADMIRELOVE`…`JOKEHATE`,
while the topics `AdmireSuccess`, `TauntFail` and 18 more persuasion topics are empty). Voice lines that
are shouted without words are written as an actor note in square brackets (`[Knight Attack Sound]`). The
table is `data/voice_topics.tsv`.

**Voice files** (T10.8, measured on 2026-09-30 over three voice archives) —
`sound\voice\<plugin>\<voice race>\<m|f>\<quest>_<topic>_<FormID 8 hex>_<response number>.mp3`, SI also uses
`oblivion.esm`. The voice race is the `RACE` `VNAM` (8 bytes: the `FormID` of the race for men, then for
women; 0 — its own), the folder name is its `FULL` in lower case. Every mp3 has a `.lip` (lip movement;
Morrowind does not read it).

**Faces: `.egt` and precomputed maps** (T8.8, measured 2026-09-30). The engine takes the texture of a head
part from the part's `ICON` in the race (`NAM0`), not from the NIF; the eyes — from the NPC's `EYES`.
`.egt` rows go bottom to top, planes R, G, B. The face detail map = `128 + ½ · Σ (FGTS of the NPC + FGTS of
the race) · scale · morph`; such precomputed maps lie in `Oblivion - Textures - Compressed.bsa` as
`textures\faces\oblivion.esm\<FormID>_N.dds` (1,977 NPCs with `_0`). The check against them gave a
correlation of 0.961 over 1,971 NPCs.

**Faces: geometry is an offset from the race too** (measured 2026-10-02 over the `FGGS` of every race and
NPC of `Oblivion.esm`). The length of a race's `FGGS` vector: Imperial, Breton, Orc — 0; Redguard 6.3,
Nord 7.8, Dark Seducer 11.3, High Elf 12.1, Dark Elf 14.5, Wood Elf 15.4, Golden Saint 15.5, Argonian 28.7,
Khajiit 30.4, Dremora 41.3. An NPC's mean `FGGS` length is 5.9…13.6 for **any** race (Dremora 5.9,
Khajiit 11.2, Imperial 10.6), and an NPC is farther from its race's `FGGS` than from zero (Dremora 42.1,
Khajiit 34.5). So an NPC stores not a face "around" the race's face but an offset from it: the face =
the race's `FGGS`/`FGGA` + the NPC's, as with `FGTS`. Before 2026-10-02 only the NPC's coefficients went
into the morph, and heads of races with a non-zero `FGGS` came out "human". Not verified in the game.

**Not verified:** that `GetPlayerInSEWorld` does not occur meaning "the player is on the islands right
now" within one world (in `Oblivion.esm` — only in quest and line conditions).

### Global variables (T11.1, spec `records/global.rs`, `transform::globals`, `data/globals.tsv`, test `transform/tests/real_globals.rs`)

`GLOB` of `Oblivion.esm`, measured 2026-10-01: 94 records, all with exactly `EDID`, `FNAM` (1 byte — `s`,
`l` or `f`), `FLTV` (`f32`). The number is stored as `f32` for any type — as in Morrowind, so the type and
the value are carried as is; integer types lose the fraction, and going beyond the type is clamped (`long`
— to ±2^24: `f32` cannot hold more exactly, and `tes3` does not write such a record), with a note. There
are no notes for `Oblivion.esm`.

Three paths (`transform::globals::resolve`):

| Path | Rule | `Oblivion.esm` |
|---|---|---|
| vanilla by table | engine variables (time, date) — the engine runs them, an own copy would stand in for them: `GameHour` → `GameHour`, `GameDay` → `Day`, `GameMonth` → `Month`, `GameYear` → `Year`, `TimeScale` → `TimeScale` | 5 |
| vanilla by name | the `EDID` matches a `Morrowind.esm` variable case-insensitively (as dialogue conditions do since T10.6) | 1 (`PCVampire`) |
| no counterpart | a table row with "-" or a table ID the master does not have | 1 (`GameDaysPassed`: `DaysPassed` exists only in Tribunal) |
| own `ob_…` | everything else | 87: `short` 80, `long` 3, `float` 4 |

Oblivion's engine variables have type `s` in the file (`GameHour` = 1, `TimeScale` = 30), while
Morrowind's `GameHour` and `TimeScale` are `f`: the value is taken by the vanilla record, ours is not
written. The `Global` filter of dialogue conditions leads where the variable itself goes
(`back2mw::dialogue`), to an own one only if it was written in this run. `SEWorld.esp` (2026-10-01): 87
`GLOB`, `esmtool` clean; the number of lines did not change — `SEWorld` had no rejections because of `GLOB`.

**Not verified:** that `PCVampire` means the same in both games (in Morrowind — 1 a vampire, −1 cured).

### Creatures (T11.2, spec `records/creature.rs`, `LeveledCreatures` in `records/leveled.rs`, test `tes4_read/tests/real_creatures.rs`)

`CREA` of `Oblivion.esm`, measured 2026-10-01 — all 914 are read by the spec:

| Field | Layout | Present in |
|---|---|---|
| `ACBS` | 16 bytes, as in `NPC_`: flags (u32), base spell points, fatigue, barter gold (u16), level (i16; with flag 0x80 — an offset from the player), min and max level (u16) | 914 |
| `AIDT` | 12 bytes, as in `NPC_` | 914 |
| `DATA` | 20 bytes: type (u8), combat / magic / stealth skills (u8), soul (u8), a byte, health (u16), 2 bytes, attack damage (u16), 8 attributes Strength … Luck (u8) | 914 |
| `RNAM` | 1 byte — attack reach | 914 |
| `BNAM`, `TNAM`, `WNAM` | `f32`: model scale, turning speed, foot weight | 914 |
| `MODL` (+ `MODB`, `MODT`) | the skeleton `creatures\<folder>\skeleton.nif` | 909 |
| `NIFZ` | body part names in a row, each with a null; up to 13, 4,043 in total | 903 |
| `KFFZ` | names of its own `.kf` files | 114 |
| `CNTO` (8), `SPLO` (4), `PKID` (4), `SNAM` (8: faction, rank i8, 3 bytes) | as in `NPC_` | 678, 600, 878, 805 |
| `SCRI`, `INAM` (corpse item), `ZNAM` (combat style) | `FormID` | 256, 9, 883 |
| `CSCR` | the creature whose sounds are used; all 817 lead to a `CREA` | 817 |
| `CSDT` / `CSDI` / `CSDC` | a group: sound type (u32), `SOUN` (all lead to a `SOUN`), chance (u8) — one sound per type in all | 92 records, 669 groups |
| `NAM0`, `NAM1` | blood spray and decal textures | 69, 10 |
| `NIFT` | 4 bytes, meaning unknown — not read | 338 |

`ACBS` flags (meaning from UESP): biped (0x1) 0, essential (0x2) 10, swims (0x10) 318, flies (0x20) 4, walks
(0x40) 900. Type `DATA[0]`: 0 Creature 595, 1 Daedra 97, 2 Undead 168, 3 Humanoid 2, 4 Horse 51, 5 Giant 1.
Soul: 0 — 6, 1 — 214, 2 — 188, 3 — 376, 4 — 79, 5 — 51. Sounds by `CSDT` type (0 left foot, 1 right, 2/3
back feet, 4 idle, 5 aware, 6 attack, 7 hit, 8 death): 89, 89, 19, 19, 90, 90, 91, 92, 90; there is no type
9 (weapon). Record flag 0x400 (persistent?) in 96 `CREA`s, 0x80000 in 48 — not used.

`LVLC` — 682: the same list as `LVLI` (`LVLD`, `LVLF` in 645, `LVLO` — 12 bytes in 4,117 entries and 8 bytes
in 1), plus `TNAM` — a template creature (in 176) and `SCRI` (in 27). Entries with a count above 1 — 8.
**The entries' targets are not only creatures:** `CREA` 2,088, nested `LVLC` 1,163, **`NPC_` 867**. A
vanilla Morrowind `LEVC` holds both NPCs and nested lists in its entries (T11.4, below).

`ACRE` (creature placements) — 1,473: `NAME` (base), `DATA` (24 — position and rotation, as in `ACHR`),
`XSCL` in 36, `XESP` in 146, `XOWN` in 90, `XRGD` (ragdoll pose) in 618; record flags 0x400 in 495
(persistent), 0x800 (disabled) in 219. Read by the placement spec (T11.4, below).

### Creatures on donors (T11.3, `transform::creatures`, `data/creatures.tsv`)

The owner's decisions of 2026-10-01: for a creature whose model folder is in `data/creatures.tsv` (13 exact
pairs of kinds), from the vanilla donor come the model, `CNAM` (the donor's `SNDG` sounds), the soul, blood
and movement flags (biped, weapon and shield, swims, flies, walks: the model's animation can do these);
from Oblivion — the name, type (0…3; horse and giant → ordinary), level (as for NPCs), attributes, skills,
`DATA` health, magicka = the `ACBS` base spell points, `ACBS` fatigue, all three attacks `(⌈D/3⌉, D)`,
essential and respawn, scale `BNAM` (if not 1), inventory and spells (`LVSP` expanded, as for NPCs), AI and
wander radius — as for NPCs; services 0, the script is not converted.

A `SEWorld` run (2026-10-01, `back2mw all --plugin Oblivion.esm --worldspace SEWorld`, `esmtool` and
`niftest` clean): 914 `CREA` → **239** records on donors, 675 without a donor (waiting for T11.5);
creatures are all converted, not only those placed in the world. 95 inventory entries without items in
the plugin (with `--worldspace` only the world's items are converted), 9 items and 29 spells removed in
the check; 49 with a script. Since T11.4, with `--worldspace` only the creatures of the world are
converted, together with their inventory — the numbers are below.

### Creatures with their own model (T11.5e, `transform::creatures`, `data/creature_souls.tsv`)

A creature whose folder has no donor is converted with its own model if the record has `NIFZ` body
parts. The record is the same as for creatures on a donor (Oblivion's numbers), but what the donor
used to give comes from elsewhere:

| Field | From |
|---|---|
| model | the path `ob\<folder>_<hash>.nif` (`paths::creature_mesh_path`): the hash is over the `MODL` skeleton and the set of `NIFZ` parts in order, so records with the same set share a model. The path is at most 30 characters: next to it the engine looks for `ob\x<folder>_<hash>.nif` and `.kf`, and those must fit into 31 |
| `CNAM` (whose sounds) | the creature itself if it has at least one Morrowind sound (T11.6, below); otherwise empty |
| soul | by the `DATA` soul size (0…5) — `data/creature_souls.tsv`: 0, 30, 60, 120, 180, 400. The owner's decision of 2026-10-02: the creature is caught in the same Morrowind gem |
| walks / swims / flies | `ACBS` flags 0x40 / 0x10 / 0x20; none set — "walks" |
| biped, weapon and shield | no |
| blood | 0 (red) |

The model files themselves are built by the mesh stage (`back2mw::creature_models`,
`mesh_convert::creature`) — `formats/nif-oblivion.md`, "Creatures: skeleton and clips", "Body",
"Tape". A creature whose model failed to build does not go into the plugin; list entries and
references to it are removed, as for any record that was not written.

Measured 2026-10-02 (`transform/tests/real_creatures.rs`): of the 914 `CREA` of `Oblivion.esm`, 239 are
on donors, 664 get their own model (181 distinct models: 59 goblins, 36 grummites, 16 horses), 11 are
not converted — neither a donor nor parts (5 will-o-the-wisps, 1 dog, 5 records without `MODL`).
Souls of the records with their own model: 0 — 5, 30 — 167, 60 — 130, 120 — 266, 180 — 48, 400 — 48;
all 664 walk, 264 of them also swim, 4 fly.

A run of `back2mw all --plugin Oblivion.esm` (2026-10-02, `esmtool` and `niftest` clean): 179 models
built of 181, **881** `CREA` written (239 + 642); 22 records dropped — the files of their body parts
are not in the archives (21 grummite templates, 1 xivilai); 9,536 meshes, 0 rejects. A run with
`--worldspace SEWorld`: 178 `CREA` — every creature of the world (29 on donors, 149 with their own
model, 49 models, all built), 170 `LEVC` of 170, 140 creature references, 1,244 list references; 3,133
meshes, 0 rejects. Before own models (T11.4) it was 29 `CREA`, 58 `LEVC`, 109 and 512 references.

`--only <EDID>` now finds creatures too: before, a creature's name counted as "no such record with a
MODL" (`back2mw/tests/real_creature_models.rs`).

### Creature placements and creature lists (T11.4, `transform::creature_lists`, `transform::exterior`, tests `tes4_read/tests/real_creatures.rs`, `tes3_out/tests/real_vanilla_leveled.rs`)

Measured 2026-10-01 on `Oblivion.esm` (`creature_placements_read_through_the_placement_spec`):

| What | Number |
|---|---|
| `ACRE` | 1,473, the base is always a `CREA`; `XSCL` in 36, disabled 219, an enable parent in 146 |
| `ACHR` | 2,190, the base is always an `NPC_` |
| `REFR` to an `LVLC` | 8,005: a creature list is placed in the world by an ordinary reference, not by `ACRE` |
| `LVLF` flags of `LVLC` | 0 — 222, 1 — 81, 2 — 114, 3 — 265; no 0x4 ("use all") |
| "none" chance above 100 | 6 lists |
| entries with a count above 1 | 8, all in two lists: `LL0Horse100` (3, 3, 3, 2, 2, 2) and `LL0Deer100` (3, 2) |
| entry level | 1…28, none below 1 |
| empty lists | 1 |

Conversion rules:

- **`ACRE` → a persistent reference** to the converted creature. It is read by the same spec as `REFR`
  and `ACHR` (`NAME`, `DATA`, `XSCL`, `XESP`, flag 0x800); the owner `XOWN` and the pose `XRGD` are not
  converted. Disabled references and those disabled through a parent are not placed, like any other
  reference. A creature without a donor is not placed (the notes say "`CREA` N").
- **`LVLC` → `LEVC`.** An entry stays if its target was converted: a creature on a donor, an NPC or a
  nested list — a vanilla `LEVC` holds all three (`formats/tes3.md`, "Leveled creature lists"). The
  "all levels" flag 0x01 → 0x1 in `LEVC`; "each item" 0x02 is dropped (`LEVC` has no such flag, and
  without a count it means nothing); an entry count above 1 → 1 — the same decision as for `LVLI`; a
  "none" chance above 100 → 100; the template `TNAM` and the script `SCRI` are not converted; an entry
  of a list pointing at itself is dropped. A list without entries is not written and references to it
  are removed — up to a fixed point, across the plugins of the run as well.
- **`REFR` to an `LVLC` → a temporary reference to the `LEVC`** — like all 7,478 references to lists
  in vanilla; the engine creates the creature in its place.
- **With `--worldspace`** only the creatures and lists of that world are converted: the bases of
  `ACRE`; the lists placed by references and those nested in them; every creature and **NPC** in
  their entries (an NPC gets everything a world NPC gets: face, inventory, dialogue); the creatures'
  inventory. Without `--worldspace` — every creature on a donor and every list; NPCs are not
  converted without a world, so the entries pointing at them are dropped.

A `SEWorld` run (2026-10-01, `back2mw all --plugin Oblivion.esm --worldspace SEWorld`, `esmtool` and
`niftest` clean):

| What | Number |
|---|---|
| creatures of the world (`ACRE` and list entries) | 178: on donors **29**, without a donor 149 — `grummite` 52, `elytra` 25, `gnarl` 22, `fleshatronach` 10, `murkdweller` 9, `shambles` 9, `gatekeeper` 8, `baliwog` 7, `dog` 7 |
| `LVLC` of the world | 170 → **58** `LEVC`; 112 ended up empty, 615 entries removed; 86 had a `TNAM` template |
| entries in the 58 `LEVC` | 311: NPCs 211, creatures 60, nested lists 40 |
| NPCs | 372 (was 307: 65 came from list entries) |
| actor references (persistent) | 394, creatures among them 109 |
| references to lists (temporary) | 512 |
| not placed | `ACRE` 31 (creature without a donor), references to lists 732 (the list ended up empty) |

Oblivion's corpse creatures (`DeadSkeleton`, `DeadZombieCorpse`) are records with health 0; health is
carried over as is.

**Not verified** in game: whether the vanilla engine creates a creature from a temporary reference to
our `LEVC`, including an NPC from an entry and a nested list (`Morrowind.esm` has no nested ones —
only Bloodmoon does); whether a creature with health 0 lies as a corpse.

## BSA v103 (Oblivion)

All numbers are little-endian. Checked on `Oblivion - Meshes.bsa` (20,182 files, 524 folders): the
header, the folder records, the first file block, the start of the name block and unpacking the first
file.

### Header, 36 bytes

| Offset | Type | Field | In `Oblivion - Meshes.bsa` |
|---|---|---|---|
| 0 | char[4] | `BSA\0` | |
| 4 | u32 | version | 103 (`0x67`) |
| 8 | u32 | offset of the folder records | 36 |
| 12 | u32 | archive flags | `0x787` |
| 16 | u32 | number of folders | 524 |
| 20 | u32 | number of files | 20,182 |
| 24 | u32 | `totalFolderNameLength` | 15,924 |
| 28 | u32 | `totalFileNameLength` | 396,723 |
| 32 | u32 | content type flags | `0x41` |

The archive flags that matter:

| Bit | Meaning |
|---|---|
| `0x1` | folder names are present (in the file blocks) |
| `0x2` | the file name block is present |
| `0x4` | files are compressed by default |
| `0x100` | **means nothing to us in v103.** In v104+ it is "the file name is embedded before the data", but in `Oblivion - Meshes.bsa` the bit is set while the data of the first file starts right with a u32 size and zlib. Checked |

### Folder records: number of folders × 16 bytes, right after the header

| Type | Field |
|---|---|
| u64 | folder name hash |
| u32 | number of files in the folder |
| u32 | the offset of this folder's file block **plus `totalFileNameLength`** |

The real position of the block is `offset - totalFileNameLength`. Checked: for the first folder it gives
exactly 36 + 16 × 524 = 8420, the end of the folder records.

### File blocks: one per folder, one after another

- if `0x1` is set: the folder name — a u8 length **including the NUL**, then the name bytes and the NUL
  (`trees\0`, length 6);
- then number of files × 16 bytes:

| Type | Field |
|---|---|
| u64 | file name hash |
| u32 | size; bit `0x4000_0000` **inverts** the archive's compression flag for this file; the real size is `size & 0x3FFF_FFFF` |
| u32 | absolute data offset from the start of the archive |

### File name block

Right after the file blocks, if `0x2` is set: number-of-files zstrings **in the same order** as the file
records of all blocks in a row. The block starts at
`36 + 16 × folders + totalFolderNameLength + folders + 16 × files`.
**`totalFolderNameLength` does not include the length bytes** — hence the "+ folders" term.
Checked: by this formula the first name is `treecottonwoodsu.spt`, which matches the first file
`bsatool list` shows inside `trees\`.

The full path is `folder name` + `\` + `file name`. Hashes are not needed for reading: the names are
there, and the name-to-record mapping is given by the order. They are only needed for lookup by path
without loading the names — which we do not need.

### File data

Whether a file is compressed: `(archive flags & 0x4 != 0) XOR (size & 0x4000_0000 != 0)`.
Compressed: a u32 of the unpacked size, then a zlib stream (`78 9C`). Uncompressed: raw bytes of size
`size & 0x3FFF_FFFF`. Checked on the first file: 6329 bytes declared, zlib unpacked it.

### Case

Names in the archive are lower case (`trees\treecottonwoodsu.spt`). Paths in plugin records (`MODL`) are in
arbitrary case. Path comparison is **case-insensitive**, with the separator normalised to `\`.

### Oracle

`bsatool list <archive>` from OpenMW 0.51.0 lists the files of a v103 archive (see the measurement of the
OpenMW tools); `bsatool extract` gives a file's bytes. Our reader must match it in the set of paths and in
content.

### Measured on real archives (2026-09-24, tests `real_bsa`)

| Parameter | Value |
|---|---|
| Oblivion BSA v103 in the data set | 16 (3 TES3 archives skipped by magic `[00, 01, 00, 00]`) |
| Files in the Oblivion BSAs in total | 142,819 |
| Errors found reading the table of contents | 0 |
| Paths in `Oblivion - Meshes.bsa` compared with `bsatool list` | 20,182 paths, **a complete match** |
| A sample (every 500th file) compared with `bsatool extract` | 41 files, **a complete byte-for-byte match** |
| All files of `Oblivion - Meshes.bsa` unpacked without errors | 20,182 files, 0 errors, ~70 s (debug) |

TES3 BSAs (Morrowind.bsa, Tribunal.bsa, Bloodmoon.bsa) use the format with magic `0x00000100` — a
different format, handled by `tes3::bsa` on the output side. `tes4_read::bsa` does not read them and
returns `NotABsa`.

---

## Oblivion NIF: 20.0.0.4, 20.0.0.5 and 10.x

Which versions and block types are really in the archive, and the header layout — in
[nif-oblivion.md](nif-oblivion.md). The main point from there: **the main version is 20.0.0.4 (90.7%)**,
not 20.0.0.5; the header has no block sizes, so an unknown block cannot be skipped.

The block layouts are described in [nif-oblivion.md](nif-oblivion.md) (T1.3). The primary sources, in
order of decreasing reliability:

1. real meshes from `Oblivion - Meshes.bsa` — the final authority;
2. the niftools project's `nif.xml` (github.com/niftools/nifxml) — a machine-readable description of all
   blocks of all versions; version conditions are in the `since`/`until`/`vercond` attributes. **Check**
   its license before generating code from it; format facts can be taken from it in any case;
3. `components/nif/` in OpenMW — a working reader of exactly these versions (GPL: reading is fine, copying
   the code is not).

OpenMW calls `0x14000005` (20.0.0.5) `VER_OB` (`components/nif/niffile.hpp`), but such files are 1.2% of
Oblivion's archive. The stream starts with the string `Gamebryo File Format, Version 20.0.0.4\n` (or
`…20.0.0.5\n`, `NetImmerse File Format, …` for old ones).

### Oblivion `ARMO`/`CLOT` (T9.2c, spec `records/worn.rs`)

`BMDT` is 4 bytes in all 996 `ARMO` and 604 `CLOT`: u16 slots (0 head, 1 hair, 2 upper body, 3 lower body,
4 hand, 5 foot, 6/7 rings, 8 amulet, 13 shield, 15 tail), u8 flags (`0x80` — heavy armor, `0x40` — not
playable), u8 unused. `DATA` is 14 bytes for armor (u16 rating ×100, u32 value, u32 health, f32 weight), 8
for clothing (u32 value, f32 weight). Models: `MODL` male worn, `MOD2` male world, `MOD3` female worn,
`MOD4` female world; icons `ICON` and `ICO2`.

Slots in `Oblivion.esm`: `ARMO` — shield 205, **hair 171 (this is how Oblivion marks open helmets)**, upper
152, hand 135, lower 119, foot 119, head+hair 42 (closed helmets), full suit upper+lower+hand+foot 36;
`CLOT` — right ring 217, amulet 106, upper+lower 64 (robes), upper 51, left ring 43, lower 32, foot 30, full
27, hair 21 (hoods).

### `ARMO`/`CLOT` → `ARMO`/`CLOT` + `BODY` (T9.2c–d, `transform::worn`)

| Oblivion | Morrowind |
|---|---|
| upper body (with anything) | cuirass / shirt (with lower — robe), part `Chest` |
| lower body | greaves / pants, part `Groin` |
| hand | **two items**: right and left gauntlet, the mesh is split by bone side |
| foot | boots / shoes, right and left foot parts (the mesh is split) |
| head (+hair) | helmet, part `Head` |
| hair only | helmet, part `Hair` |
| shield | shield, part `Wrist` |
| ring, amulet | ring, amulet, without body parts — **even together with body slots** (for clothing they are checked first) |
| clothing on the head (hoods) | **helmet**, part `Head` or `Hair`; rating 0, health 100 (like the vanilla hat `gondolier_helm`: weight 4, health 100, rating 1), weight and value as is (T9.2f, the owner's decision of 2026-09-27) |
| armor in the amulet slot | **not converted**, a note |

Hoods of `Oblivion.esm`, 2026-09-27, test `statics::every_hood_of_oblivion_esm_becomes_a_helmet_read_by_esmtool`:
22 `CLOT` → 22 helmets and 24 body parts, `esmtool` reads all. The Morrowind armor class is set by the
weight (a helmet is light up to 5 × 0.6 = 3, medium up to 4.5 — GMST `iHelmWeight`, `fLightMaxMod`,
`fMedMaxMod`): 18 light (weight 0…3), **4 medium** (weight 4 — as in Oblivion).

One body part per mesh: both Oblivion's armor and the vanilla skinned cuirass are one mesh per item. The IDs
of the parts and of the second gauntlet are `transform::ids::variant_id` (`obm_…`, `obf_…`, `obmr_…`,
`obl_…`): a mark in place of the prefix's `_` keeps them from clashing with the main IDs. The same
mechanism gives tree variants by seed (`obt1_…`, `obt2_…`, T2.4g, [spt.md](spt.md)).

**Armor rating.** 31 `ARMO` pairs by name with `Morrowind.esm`. Oblivion's rating (×100) is proportional to
the slot in the same way for all materials: cuirass 1.0, greaves 0.6, hand/foot/head 0.4, shield 1.2 (iron:
1000, 600, 400, 400, 1200). In Morrowind all parts of a material have one rating. The rating brought to the
cuirass level is translated piecewise linearly:

| Material | Oblivion (cuirass) | Morrowind |
|---|---|---|
| iron | 10 | 10 |
| steel | 11.25 | 15 |
| orcish | 13.75 | 30 |
| ebony | 15 | 60 |
| daedric | 18.75 | 80 |
| glass (light) | 12.5 | 50 — light armor correction ×7/6 |

**Other:** weight and value as is (the weight sets the Morrowind armor class: an iron cuirass of 30 is
heavy, a glass one of 15 is light); a pair of gauntlets — half each. Health — the median ratio over the
pairs of a type (cuirass 0.947, greaves 0.411, boots 0.632, shield 1.067, others 0.867). Enchantment
capacity — the vanilla median by type. Scripts are removed with a note; enchantments are converted since
T9.4 (see "Enchantments and spells").
