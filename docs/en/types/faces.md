# HAIR, EYES, FaceGen — faces, hair, eyes

**Status:** ✅ converted · **Morrowind:** own head and hair `BODY` per face · **Code:**
`mesh_convert::facegen`, `mesh_convert::head`, `mesh_convert::face_texture`, `back2mw::faces`

## In Oblivion

A face is **computed at run time** by the FaceGen library. An NPC stores coefficients: `FGGS`
(symmetric geometry, 50), `FGGA` (asymmetric, 30), `FGTS` (texture, 50). The race provides the head
parts (head, ears, mouth, teeth, tongue, eyes) with morph bases `.egm` (geometry) and `.egt` (texture);
facial animation is `.tri`. `HAIR` is a hair mesh with a texture from `ICON`; the hair color is the
NPC's `HCLR`. `EYES` is the eye texture.

## In Morrowind

**No morphing.** A head is a static NIF referenced by a `BODY` with the `Head` part; hair is a `BODY`
with the `Hair` part. An NPC refers to its head and hair directly by ID, and each can have its own.
Facial animation is the `Talk` and `Blink` morphs inside the head NIF.

## How back2mw converts it

The face is **baked**: its own head mesh and its own texture per face.

| Oblivion | Morrowind | Rule |
|---|---|---|
| race head parts + `.egm` × `FGGS`/`FGGA` (NPC + race) | one head NIF | an NPC's coefficients are an offset from the race's face, the two are added; all parts merged in the `Head` node, axes as in vanilla; normals welded |
| `.tri` | `Talk`/`Blink` morphs | "talks" — `BigAah`, "blinks" — `BlinkLeft`/`BlinkRight`; on the head, mouth, teeth, tongue |
| `.egt` × `FGTS` (NPC + race) | own DXT1 DDS face texture | map = 128 + ½·Σ coefficient·morph; face = race texture × map / 128 |
| part textures | from the race's `ICON` | as Oblivion itself does |
| `EYES` | eye texture | |
| `HAIR` | hair `BODY` | mesh with the same face's morph, texture from `ICON`, color `HCLR` as the material color |
| head material ambient | = diffuse | otherwise the face is darker than the neck in shadow |

Identical faces share one mesh. The baked map was checked against Oblivion's precomputed maps for
1,971 NPCs (correlation 0.961). Result for `SEWorld`: 372 heads, 362 hair meshes, 628 skin textures
(about 25 MB).

## What is lost

- Age textures of races.
- `.lip` lip sync (Morrowind moves the mouth with loudness).
- Facial animation other than talking and blinking (emotions).

!!! warning "Not verified in the game"
    The Oblivion head is 17 % wider than the vanilla one — how it sits on a Morrowind body; the look of
    skin and eyes.
