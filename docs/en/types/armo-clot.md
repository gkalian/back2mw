# ARMO, CLOT — armor and clothing

**Status:** ✅ converted · **Morrowind:** `ARMO`/`CLOT` + `BODY` body parts · **Code:**
`transform::worn`, `mesh_convert::skin`

## In Oblivion

An item occupies **a set of slots** (a bit mask): head, hair, upper body, lower body, hands, feet,
rings, amulet, shield, tail. Flags: heavy armor, "not playable". Four models: male worn, male ground,
female worn, female ground; two icons. The worn mesh is **a single skinned mesh for the whole item**,
together with pieces of the body under it (recolored to the wearer's race skin at run time). Armor has
rating, health, weight, value; clothing only value and weight.

Oblivion marks open helmets with the "hair" slot, closed ones with "head + hair", and hoods are
`CLOT` on the head. `Oblivion.esm` has 996 `ARMO` and 604 `CLOT`.

## In Morrowind

An item is **a list of parts** (`INDX`): each part is a body slot (26 of them: head, hair, neck, chest,
groin, skirt, hands, wrists, forearms, upper arms, feet, ankles, knees, upper legs, pauldrons, tail…)
and a `BODY` record with the male and female mesh. Gauntlets are **two different items**, left and
right. The armor class (light/medium/heavy) comes from the weight relative to the type. All parts of
one material have the same rating.

## How back2mw converts it

**Slots:**

| Oblivion | Morrowind |
|---|---|
| upper body | cuirass / shirt (with lower body — robe), part `Chest` |
| lower body | greaves / pants, part `Groin` |
| hand | **two items**: right and left gauntlet; the mesh is split by bone side |
| foot | boots / shoes, right and left foot parts (the mesh is split) |
| head (+ hair) | helmet, part `Head` |
| hair only | helmet, part `Hair` |
| shield | shield, part `Wrist` |
| ring, amulet | ring, amulet (no body parts) |
| clothing on the head (hood) | **helmet** with rating 0, health 100 |
| armor in the amulet slot | not converted |

**Worn mesh.** The Oblivion skinned mesh is moved onto the Morrowind skeleton (the skeleton is read from
`Morrowind.bsa` of your installation). There is no morphing or physics — the mesh is rigid.

**Body under the item.** Oblivion does not draw the body under an occupied slot at all, while
Morrowind draws everything not covered by a part. So an item gets **empty parts** for the body slots it
covers — otherwise arms and legs poke through the cuirass. There are at most 7 empty parts per item:
more crashes the original engine when the plugin loads. If there are more slots, the most visible ones
stay: chest, groin, upper legs, knees, upper arms, forearms, ankles, feet, hands, wrists (a left/right
pair is taken whole).

**Skin inside item meshes.** Body pieces with Imperial skin outside the item's own slot are removed
from the mesh — the vanilla body of the right race shows there. In the item's own slot the skin stays
Imperial: otherwise there would be a hole.

**Armor rating** is brought to the cuirass level (in Oblivion it is proportional to the slot: cuirass
1.0, greaves 0.6, hand/foot/head 0.4, shield 1.2) and translated piecewise linearly over 31 pairs:

| Material | Oblivion (cuirass) | Morrowind |
|---|---|---|
| iron | 10 | 10 |
| steel | 11.25 | 15 |
| orcish | 13.75 | 30 |
| ebony | 15 | 60 |
| daedric | 18.75 | 80 |
| glass (light) | 12.5 | 50 |

**Other:** weight and value as is (a pair of gauntlets — half each); health — median ratio per type;
enchantment capacity — vanilla median; material ambient 0 is replaced with diffuse (otherwise the item
is black in shadow); icons are shrunk to 32×32.

## What is lost

- Scripts.
- Swapping the skin for the race's in the item's own slot: the skin there is Imperial.
- Armor in the amulet slot.
- The "open / closed helmet" distinction survives only as the part slot (`Hair` or `Head`).
- Some empty slots when there are more than 7: the body may show through at the shoulders and shins.
- The race field of armor `BODY` records is written empty (vanilla always fills it).

!!! warning "Not verified in the game"
    Whether the body pokes through at the seams and shoulders; that a cuirass's empty slots do not hide
    other armor (greaves, bracers); whether the Construction Set accepts an empty race on armor `BODY`.
