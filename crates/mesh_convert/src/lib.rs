//! Mesh conversion: NIF 20.0.0.x (Oblivion) → 4.0.0.2 (Morrowind).
//!
//! What must not be forgotten in this layer:
//!
//! * **We write the source NIF parser ourselves.** This is the only layer where Rust is weaker than
//!   Python: there is no ready library. The `nif` crate (amPerl) is abandoned, "super-primitive",
//!   and good only as a reference.
//! * **Textures are not converted but copied.** Morrowind understands Oblivion DDS. Only the slots
//!   that survive the replacement of shader properties need to be carried over — diffuse and glow;
//!   the vanilla engine does not use normal and specular maps, and dragging them along would bloat
//!   the mod with dead weight.
//! * **Collision: havok units are multiplied by 7.** Confirmed in the reference's sources
//!   (`asset_convert/collision/collision_extract.py:43`). `bhk*` is unrolled into triangles inside
//!   `RootCollisionNode`.

/// Game units in one Oblivion havok unit.
pub const HAVOK_UNITS_TO_GAME: f32 = 7.0;

pub mod audio;
pub mod collision;
pub mod creature;
pub mod door;
pub mod face_texture;
pub mod facegen;
pub mod geometry;
pub mod head;
pub mod icons;
pub mod material;
pub mod particles;
pub mod read;
pub mod skin;
pub mod speedtree;
pub mod textures;

/// The NIF version Morrowind understands.
pub const NIF_VERSION_TES3: u32 = 0x0400_0002;

/// The NIF version of most of Oblivion: 90.7% of the archive and all 4,712 readable `STAT` meshes.
pub const NIF_VERSION_TES4: u32 = 0x1400_0004;

/// A rare Oblivion version (1.2% of the archive); not met in `STAT` meshes, the layout is not
/// checked on data.
pub const NIF_VERSION_TES4_ALT: u32 = 0x1400_0005;

/// The node in which Morrowind looks for collision. The engine treats an empty node as "camera-only
/// collision".
pub const ROOT_COLLISION_NODE: &str = "RootCollisionNode";

/// A node name prefix taken by the Construction Set: a node whose name starts with `Collision`
/// (nine characters, case-insensitive) is moved by the editor, when loading the model, into a
/// separate collision node of the object that the editor itself hangs under the mesh root. If the
/// root is named like that, the collision node becomes its own descendant, and the editor crashes
/// with a stack overflow when loading the model of such an object (dump 2026-10-02: Oblivion's
/// `CollisionBox01` and `CollisionBoxStatic` — 363 `SEWorld` references; analysis in
/// `docs/en/formats/tes3.md`, "A node named `Collision…`"). Whether the game engine does the same
/// is not verified.
pub const RESERVED_NODE_PREFIX: &str = "collision";

/// What is put in front of a node name starting with [`RESERVED_NODE_PREFIX`].
pub const NODE_NAME_GUARD: &str = "ob_";

/// The output node name from the source node name: as is, except names starting with
/// [`RESERVED_NODE_PREFIX`] — [`NODE_NAME_GUARD`] is put in front of them. Morrowind does not look
/// anything up by the node names of world objects, so renaming breaks nothing; the skeletons of
/// creatures and bodies bypass this — their bones are looked up by name.
pub fn node_name(name: &str) -> String {
    let reserved = name
        .as_bytes()
        .get(..RESERVED_NODE_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(RESERVED_NODE_PREFIX.as_bytes()));
    if reserved {
        format!("{NODE_NAME_GUARD}{name}")
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::node_name;

    #[test]
    fn a_node_name_never_starts_with_collision() {
        assert_eq!(node_name("CollisionBox01"), "ob_CollisionBox01");
        assert_eq!(node_name("COLLISIONgarlic"), "ob_COLLISIONgarlic");
        assert_eq!(node_name("collision"), "ob_collision");
        // Control: other names are not touched, including Morrowind's collision node and a name
        // shorter than nine characters.
        assert_eq!(node_name("RootCollisionNode"), "RootCollisionNode");
        assert_eq!(node_name("Collisio"), "Collisio");
        assert_eq!(node_name("c_col"), "c_col");
        assert_eq!(node_name(""), "");
    }
}
