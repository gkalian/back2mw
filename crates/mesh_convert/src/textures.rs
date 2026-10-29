//! Texture names in output meshes and the list of files to copy, T1.6.
//!
//! In a Morrowind NIF a texture is given by a bare file name from `Textures\`: there is no
//! directory (checked on all vanilla NIFs: 22,021 references, 0 with a separator,
//! `docs/en/formats/tes3.md`). Oblivion textures live in a directory tree, so when laying them out
//! into one directory the paths have to be flattened, and the names kept apart so that two
//! different files do not merge into one.
//!
//! **Scheme** (part of output compatibility, like the ID scheme in `transform::ids`): the path
//! relative to `textures\`, lower case, `\` → `_`, prefix `ob_`, extension as in the source.
//! `textures\architecture\anvil\anvilwall01.dds` → `ob_architecture_anvil_anvilwall01.dds`.
//!
//! The scheme is **not injective**: `a_b\c.dds` and `a\b_c.dds` give the same name. So the
//! [`TextureRegistry`] remembers which source path a name belongs to, and a second, different path
//! with the same name is an error, not an overwrite. We do not invent suffixes to separate
//! collisions: such a scheme would become part of compatibility with meshes already built.
//!
//! The extension stays from the source (`.dds` in Oblivion). That the vanilla engine looks for
//! `.dds` when the NIF has `.tga` is established from vanilla but not checked in the game.

use std::collections::HashMap;
use std::fmt;

/// Name prefix of converted textures: no vanilla name starts with it.
pub const PREFIX: &str = "ob_";

/// One file to copy: where to take it from in the source BSA and what to call it in `Textures\`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextureCopy {
    /// Path in the source archive, lower case, with `textures\` at the start.
    pub source_path: String,
    /// File name in Morrowind's `Textures\`.
    pub target_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureError {
    /// The name or path is not ASCII: it has no place in a NIF or in Morrowind's file system.
    NotAscii { path: String },
    /// Nothing is left of the path (empty or just `textures\`).
    Empty { path: String },
    /// The path has a `..` segment.
    ParentSegment { path: String },
    /// Two different source paths gave the same name.
    Collision {
        name: String,
        first: String,
        second: String,
    },
    /// A normal map (`*_n.dds`) in a slot that is carried over: it is not supposed to be copied,
    /// and a reference without a file must not be left behind.
    NormalMap { path: String },
}

impl fmt::Display for TextureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAscii { path } => lang::trw!(
                f,
                "путь текстуры {path:?} не ASCII",
                "texture path {path:?} is not ASCII"
            ),
            Self::Empty { path } => lang::trw!(
                f,
                "путь текстуры {path:?} пуст",
                "texture path {path:?} is empty"
            ),
            Self::ParentSegment { path } => lang::trw!(
                f,
                "путь текстуры {path:?} содержит «..»",
                "texture path {path:?} contains \"..\""
            ),
            Self::Collision {
                name,
                first,
                second,
            } => lang::trw!(
                f,
                "имя {name} получилось у двух разных текстур: {first} и {second}",
                "two different textures got the name {name}: {first} and {second}"
            ),
            Self::NormalMap { path } => {
                lang::trw!(
                    f,
                    "текстура {path:?} — карта нормалей в переносимом слоте",
                    "texture {path:?} is a normal map in a converted slot"
                )
            }
        }
    }
}

impl std::error::Error for TextureError {}

/// Converts a source path into the form `(path in archive, name in Textures\)`. A pure function.
pub fn target_of(source: &str) -> Result<TextureCopy, TextureError> {
    if !source.is_ascii() {
        return Err(TextureError::NotAscii {
            path: source.to_owned(),
        });
    }
    let lower = source.to_ascii_lowercase();
    let mut segments: Vec<&str> = lower.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
    if segments.contains(&"..") {
        return Err(TextureError::ParentSegment {
            path: source.to_owned(),
        });
    }
    if segments.first() == Some(&"textures") {
        segments.remove(0);
    }
    if segments.is_empty() {
        return Err(TextureError::Empty {
            path: source.to_owned(),
        });
    }
    if is_normal_map(&lower) {
        return Err(TextureError::NormalMap {
            path: source.to_owned(),
        });
    }
    Ok(TextureCopy {
        source_path: format!("textures\\{}", segments.join("\\")),
        target_name: format!("{PREFIX}{}", segments.join("_")),
    })
}

/// A normal map by the Oblivion naming convention: `<name>_n.<extension>`.
pub(crate) fn is_normal_map(lower: &str) -> bool {
    let stem = lower.rsplit_once('.').map_or(lower, |(stem, _)| stem);
    stem.ends_with("_n")
}

/// The registry of issued names. One for the whole run: a collision between textures of
/// **different** meshes is a collision too.
#[derive(Debug, Clone, Default)]
pub struct TextureRegistry {
    /// Name in `Textures\` → path in archive.
    by_name: HashMap<String, String>,
}

impl TextureRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The name for a source texture. A repeated request for the same path gives the same name.
    pub fn register(&mut self, source: &str) -> Result<TextureCopy, TextureError> {
        let copy = target_of(source)?;
        match self.by_name.get(&copy.target_name) {
            Some(existing) if *existing == copy.source_path => {}
            Some(existing) => {
                return Err(TextureError::Collision {
                    name: copy.target_name,
                    first: existing.clone(),
                    second: copy.source_path,
                });
            }
            None => {
                self.by_name
                    .insert(copy.target_name.clone(), copy.source_path.clone());
            }
        }
        Ok(copy)
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// All registered textures by file name: the order does not depend on the registration order.
    pub fn copies(&self) -> Vec<TextureCopy> {
        let mut out: Vec<TextureCopy> = self
            .by_name
            .iter()
            .map(|(name, source)| TextureCopy {
                source_path: source.clone(),
                target_name: name.clone(),
            })
            .collect();
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests;
