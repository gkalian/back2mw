# APPA — alchemy apparatus

**Status:** ✅ converted · **Morrowind:** `APPA` · **Code:** `transform::items`

## In Oblivion

Type (mortar and pestle, alembic, calcinator, retort), value, weight, quality in percent (10…100).
`Oblivion.esm` has 23 records.

## In Morrowind

The same four types in the same order; quality is a fractional number (0.5…2.0 in vanilla).

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| type | type | as is |
| quality | quality / 50 | 10…100 % → 0.2…2.0 |
| value, weight | as is | |

## What is lost

Nothing significant. Scripts are not converted.
