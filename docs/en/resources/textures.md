# Textures and icons

**Code:** `mesh_convert::textures`, `mesh_convert::icons`, `mesh_convert::face_texture`

## Textures

Both Oblivion and Morrowind read **DDS**, so textures are **copied, not converted**.

| Oblivion | Morrowind | Rule |
|---|---|---|
| `textures\architecture\anvil\anvilwall01.dds` | `Textures\ob_architecture_anvil_anvilwall01.dds` | path relative to `textures\`, `\` → `_`, prefix `ob_` |
| base slot | base | copy |
| glow slot | glow | copy |
| normal map `*_n.dds`, parallax, reflections | — | Morrowind has no such slots |
| land texture `LTEX` | `Textures\ob\<name>_<hash>.dds` | see [Landscape](../types/land.md) |
| book images | `BookArt\ob_<name>_<hash>.dds` | see [Books](../types/book.md) |

Two different sources yielding the same name abort the run, naming both (there are 0 such cases in
`Oblivion.esm`).

**Face skin** is the exception: the converter creates it itself. Each face gets its own `obface_…`
texture (DXT1 DDS with mipmaps, our own encoder): the race texture multiplied by the face detail map.
See [Faces](../types/faces.md).

## Icons

Oblivion icons are 64×64 and vanilla Morrowind ones 32×32, and the engine draws an icon at its own
size: Oblivion icons were twice as large in the inventory. So an icon is **shrunk to 32×32** and placed
in `Icons\ob\<name>_<hash>.dds`.

## What is lost

- Normal maps, parallax, reflections (`*_n.dds`, `*_g.dds`, environment): lighting is flat.
- Age textures of races.

!!! warning "Not verified in the game"
    Long texture names inside NIFs (up to 74 characters; vanilla names are short).
