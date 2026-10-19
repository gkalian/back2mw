# DOOR — doors

**Status:** 🟡 partly · **Morrowind:** `DOOR` · **Code:** `transform::world`, `transform::interior`,
`mesh_convert::door`

## In Oblivion

Name, model, flags (Oblivion gate, automatic, hidden, minimal use), open, close and loop sounds. A door
**opens with an animation inside the mesh**: a `NiControllerManager` with `Open`/`Close` sequences
rotates the leaf around its hinge — a node often offset from the origin. There are portcullises
(sliding up), double doors and display cases with a hinged lid. Where a teleport door leads is stored
in the reference (`XTEL`), not in the record. `Oblivion.esm` has 501 records.

## In Morrowind

Name, model, script, open and close sounds. There is no motion in the mesh: the engine **rotates the
reference itself** by 90° around the vertical axis — around the mesh origin, always clockwise (seen
from above), in one second. A teleport door does not rotate at all. The destination is in the
reference (`DODT`/`DNAM`).

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| name, mesh | name, the door's own mesh | the mesh animation is dropped, the mesh is in the closed position |
| the leaf's hinge | the mesh origin | see below |
| open / close sound | `SNAM` / `ANAM` | see [Sounds](soun.md) |
| `XTEL` teleport | `DODT` (+ `DNAM`) | see [Interiors](interior.md) |
| `XLOC` lock | `FLTV`/`KNAM` | see [Placements and locks](refr.md#locks) |

### The hinge

The converter reads the `Open` sequence and looks at what it moves. That decides what the door becomes
in Morrowind:

| The door in Oblivion | Meshes | What the converter does | In Morrowind |
|---|---|---|---|
| one leaf, hinge at the mesh origin | 30 | nothing | opens around its hinge |
| one leaf, hinge offset, only the leaf in the mesh | 13 | moves the mesh origin to the hinge and shifts the references so the door stays in place | opens around its hinge |
| one leaf and a frame (threshold, wall) in the same mesh | 9 | nothing | the frame turns together with the leaf |
| several leaves: double doors, gates | 22 | nothing | both leaves turn as one piece |
| sliding: portcullises, sliding doors | 12 | nothing | the portcullis turns instead of rising |
| tilting: display case lids | 10 | nothing | the lid turns sideways |
| no animation (teleport doors and doors that do not open) | 158 | nothing | the teleport works; an ordinary door without an animation will start turning in Morrowind |

A door gets its own mesh even if a static uses the same model: the origin is shifted for the door only.
The converter writes to the log and the notes how many doors of each kind went into the plugin.

Result for `SEWorld`: 68 door records — no animation 41, hinge in place 8, hinge moved 1 (the Dementia
door, 3 references), with a frame 8, double doors 3, portcullises 7.

## What is lost

- **Doors with a frame in the same mesh, double doors, portcullises, lids** — Morrowind's rotation
  cannot express them; they turn as a whole around the mesh origin.
- **Direction.** Morrowind opens a door clockwise only; about a third of Oblivion's hinged doors open
  counter-clockwise — they will open the other way.
- **Angle and speed.** Oblivion's doors open by 29°…120° in 0.7…8.6 seconds, with overshoot and
  settling; Morrowind's — always by 90° in one second.
- Doors that do not open in Oblivion (neither an animation nor a teleport) turn in Morrowind.
- The door's loop sound (`BNAM`), Oblivion's flags.
- Doors between merged worlds (city ↔ island) are dropped; doors to a world that is not converted have
  no destination.

!!! warning "Not verified in the game"
    That a door with a moved hinge stands in its doorway and opens around the hinge; what doors with
    a frame, double doors and portcullises look like.
