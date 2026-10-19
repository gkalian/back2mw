# GLOB — global variables

**Status:** ✅ converted · **Morrowind:** `GLOB` · **Code:** `transform::globals`, `data/globals.tsv`

## In Oblivion

`EDID`, type (`short`, `long`, `float`), value (always stored as `f32`). Some variables are run by the
engine itself: time, date, time scale. `Oblivion.esm` has 94 records.

## In Morrowind

The same layout: name, type, `f32` value. The engine variables are its own vanilla ones (`GameHour`,
`Day`, `Month`, `Year`, `TimeScale`…), 73 in `Morrowind.esm`.

## How back2mw converts it

| Path | Rule | `Oblivion.esm` |
|---|---|---|
| vanilla by table | engine variables: an own copy would stand in for the real one | 5 |
| vanilla by name | the `EDID` matches a `Morrowind.esm` variable case-insensitively | 1 (`PCVampire`) |
| no counterpart | a table row with "-" or the variable is missing from the master | 1 (`GameDaysPassed`: `DaysPassed` exists only in Tribunal) |
| own `ob_…` | everything else | 87 |

Type and value are carried as is; integer types lose the fraction, `long` is clamped to ±2^24. The
`Global` filter in dialogue conditions leads where the variable itself goes.

--8<-- "globals.md"

!!! warning "Not verified"
    That `PCVampire` means the same in both games.
