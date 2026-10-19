# ACTI, FURN — activators and furniture

**Status:** ✅ converted · **Morrowind:** `ACTI` · **Code:** `transform::world`, `transform::sounds`

## In Oblivion

**`ACTI`** — an object you can use: levers, altars, mechanisms. Name, model, script (927 of 1,252
records in `Oblivion.esm`), looping sound (`SNAM`, in 50).

**`FURN`** — furniture you can sit or lie on: a model plus sit and sleep markers (`MNAM`). 186 records.

## In Morrowind

`ACTI` — name, model, script. Furniture as a mechanic does not exist: you cannot sit or lie down;
vanilla beds are `ACTI`s with a sleep script.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| `ACTI` | `ACTI` | name and mesh |
| `FURN` | `ACTI` | name and mesh |
| looping sound `SNAM` | an `obls_…` script on the activator | `PlayLoopSound3DVP` — a template checked byte for byte against vanilla `Sound_*` scripts; see [Sounds](soun.md) |

Without a name an activator cannot be activated — the same as in vanilla.

## What is lost

- Activator **scripts**: levers, buttons, quest objects do nothing.
- **Sitting and lying**: furniture is just decoration.
- Animation inside the mesh (lever, winch) — the mesh freezes in the pose stored in the file.

!!! warning "Not verified in the game"
    Sound loops on activators.
