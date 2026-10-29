//! Material: Oblivion properties → Morrowind properties, T1.5.
//!
//! Unlike Skyrim, Oblivion has no `BSShader*`: the material is set by the same properties as in
//! Morrowind (`NiMaterialProperty`, `NiTexturingProperty`, `NiAlphaProperty`,
//! `NiVertexColorProperty`), so here there is only a mapping of 20.0.0.x → 4.0.0.2 fields, whose
//! table is in `docs/en/formats/nif-oblivion.md`. Linking blocks to each other is done by
//! [`crate::geometry`]; the functions here work on one block.
//!
//! What is deliberately missing from the output:
//!
//! * `NiTexturingProperty`: only the base and glow slots remain. Vanilla Morrowind does not use
//!   dark, detail, gloss, bump and decal.
//! * UV transform (`Has Texture Transform`): a Morrowind slot has no field for it.
//! * Controllers and extra data on properties (`NiObjectNET`) — as for nodes.

use tes3::nif::glam::Vec3;
use tes3::nif::{
    Action, AlphaFormat, ApplyMode, ClampMode, DrawMode, FilterMode, LightingMode, Map,
    NiAlphaProperty, NiLink, NiMaterialProperty, NiObjectNET, NiProperty, NiSourceTexture,
    NiSpecularProperty, NiStencilProperty, NiTexture, NiTexturingProperty, NiVertexColorProperty,
    PixelLayout, SourceVertexMode, StencilTestFunction, TextureMap, TextureSource, UseMipMaps,
};

use crate::geometry::ConvertError;
use crate::read::properties::TexDesc;
use crate::read::shapes::{NiSpecularProperty as SrcSpecular, NiStencilProperty as SrcStencil};
use crate::read::{
    NiAlphaProperty as SrcAlpha, NiMaterialProperty as SrcMaterial,
    NiSourceTexture as SrcSourceTexture, NiTexturingProperty as SrcTexturing,
    NiVertexColorProperty as SrcVertexColor,
};

/// `AlphaFlags` bits that exist in 4.0.0.2: blending (0), source and destination modes (1–8), test
/// (9) and test function (10–12). Bits 13–15 (`No Sorter`, `Clone Unique`,
/// `Editor Alpha Threshold`) are Bethesda-only, Morrowind does not have them.
const ALPHA_FLAGS_MASK: u16 = 0x1FFF;

/// Number of slots in the output `NiTexturingProperty`: base, dark, detail, gloss, glow, bump,
/// decal 0. Only base and (rarely) glow are used. That vanilla Morrowind properties have exactly
/// this many slots is **not verified** on the meshes of `Morrowind.bsa`.
pub(crate) const OUT_SLOTS: usize = 7;
pub(crate) const SLOT_BASE: usize = 0;
const SLOT_GLOW: usize = 4;

fn net(name: &str) -> NiObjectNET {
    NiObjectNET {
        name: name.to_owned(),
        ..NiObjectNET::default()
    }
}

fn unknown(block: usize, field: &'static str, value: u32) -> ConvertError {
    ConvertError::UnknownValue {
        block,
        field,
        value,
    }
}

/// `NiMaterialProperty`: colors, glossiness and alpha — as is, except a zero ambient. In 20.0.0.4
/// this block has no flags; in 4.0.0.2 it does, and they stay zero.
///
/// **Zero ambient → diffuse** (owner's decision 2026-09-30). Morrowind lights a material with
/// ambient light × ambient plus direct light × diffuse, and everything turned away from the lamps
/// comes out pure black with ambient 0: the shirt sleeves of `SEWorld` NPCs in dark houses (game,
/// 2026-09-30). Judging by how the same items look in it, Oblivion does not use the material's
/// ambient. `SEWorld` has 956 such shapes; for 371 of them (184 meshes: clothing, effects, trees)
/// vertex colors do not replace ambient either.
pub fn material(src: &SrcMaterial) -> NiMaterialProperty {
    let ambient = if src.ambient == [0.0; 3] {
        src.diffuse
    } else {
        src.ambient
    };
    NiMaterialProperty {
        base: NiProperty {
            base: net(&src.net.name),
            flags: 0,
        },
        ambient_color: Vec3::from_array(ambient),
        diffuse_color: Vec3::from_array(src.diffuse),
        specular_color: Vec3::from_array(src.specular),
        emissive_color: Vec3::from_array(src.emissive),
        shine: src.glossiness,
        alpha: src.alpha,
    }
}

/// `NiVertexColorProperty`: the flags and both modes are carried over; a value outside the known
/// ones is an error, not a silent replacement.
pub fn vertex_color(
    block: usize,
    src: &SrcVertexColor,
) -> Result<NiVertexColorProperty, ConvertError> {
    let source_vertex_mode = match src.vertex_mode {
        0 => SourceVertexMode::Ignore,
        1 => SourceVertexMode::Emissive,
        2 => SourceVertexMode::AmbientDiffuse,
        v => return Err(unknown(block, "Vertex Mode", v)),
    };
    let lighting_mode = match src.lighting_mode {
        0 => LightingMode::Emissive,
        1 => LightingMode::EmissiveAmbientDiffuse,
        v => return Err(unknown(block, "Lighting Mode", v)),
    };
    Ok(NiVertexColorProperty {
        base: NiProperty {
            base: net(&src.net.name),
            flags: src.flags,
        },
        source_vertex_mode,
        lighting_mode,
    })
}

/// `NiSpecularProperty`: flags as is.
pub fn specular(src: &SrcSpecular) -> NiSpecularProperty {
    NiSpecularProperty {
        base: NiProperty {
            base: net(&src.net.name),
            flags: src.flags,
        },
    }
}

/// `NiStencilProperty`: the fields and their values are the same in both versions; 20.0.0.4 lacks
/// only `flags`, which are zero in the output. The main thing here is the two-sided mode
/// (`draw_mode`).
pub fn stencil(src: &SrcStencil) -> NiStencilProperty {
    // Exporters (3ds Max and others) sometimes write junk into these fields, for example `1.0f` as
    // an integer (1065353216). Of the stencil, Morrowind cares only about the two-sided mode, so an
    // unclear value is replaced by the default instead of failing the whole mesh.
    let function = match src.function {
        0 => StencilTestFunction::Never,
        1 => StencilTestFunction::Less,
        2 => StencilTestFunction::Equal,
        3 => StencilTestFunction::LessEqual,
        4 => StencilTestFunction::Greater,
        5 => StencilTestFunction::NotEqual,
        6 => StencilTestFunction::GreaterEqual,
        7 => StencilTestFunction::Always,
        _ => StencilTestFunction::default(),
    };
    let action = |v: u32| match v {
        0 => Action::Keep,
        1 => Action::Zero,
        2 => Action::Replace,
        3 => Action::Increment,
        4 => Action::Decrement,
        5 => Action::Invert,
        _ => Action::default(),
    };
    let draw_mode = match src.draw_mode {
        0 => DrawMode::Default,
        1 => DrawMode::CounterClockwise,
        2 => DrawMode::Clockwise,
        3 => DrawMode::Both,
        _ => DrawMode::default(),
    };
    NiStencilProperty {
        base: NiProperty {
            base: net(&src.net.name),
            flags: 0,
        },
        stencil_enabled: src.enabled,
        stencil_function: function,
        stencil_ref: src.reference,
        stencil_mask: src.mask,
        fail_action: action(src.fail_action),
        pass_z_fail_action: action(src.z_fail_action),
        pass_action: action(src.pass_action),
        draw_mode,
    }
}

/// `NiAlphaProperty`: flags (without the Bethesda-only bits) and the test threshold.
pub fn alpha(src: &SrcAlpha) -> NiAlphaProperty {
    NiAlphaProperty {
        base: NiProperty {
            base: net(&src.net.name),
            flags: src.flags & ALPHA_FLAGS_MASK,
        },
        test_ref: src.threshold,
    }
}

/// A `NiSourceTexture` with an external file. Internal textures (pixels in the NIF itself) do not
/// occur in `STAT` meshes, and there is nothing to carry them over with — an error. `file_name` is
/// already the final name (the caller chooses it).
pub fn source_texture(
    block: usize,
    src: &SrcSourceTexture,
    file_name: String,
) -> Result<NiSourceTexture, ConvertError> {
    if !src.use_external {
        return Err(ConvertError::InternalTexture { index: block });
    }
    let [layout, mipmaps, alpha_format] = src.format_prefs;
    let pixel_layout = match layout {
        0 => PixelLayout::Palettized8,
        1 => PixelLayout::HighColor16,
        2 => PixelLayout::TrueColor32,
        3 => PixelLayout::Compressed,
        4 => PixelLayout::BumpMap,
        5 => PixelLayout::Palettized4,
        6 => PixelLayout::Default,
        v => return Err(unknown(block, "Pixel Layout", v)),
    };
    let use_mipmaps = match mipmaps {
        0 => UseMipMaps::No,
        1 => UseMipMaps::Yes,
        2 => UseMipMaps::Default,
        v => return Err(unknown(block, "Use Mipmaps", v)),
    };
    let alpha_format = match alpha_format {
        0 => AlphaFormat::None,
        1 => AlphaFormat::Binary,
        2 => AlphaFormat::Smooth,
        3 => AlphaFormat::Default,
        v => return Err(unknown(block, "Alpha Format", v)),
    };
    Ok(NiSourceTexture {
        base: NiTexture {
            base: net(&src.net.name),
        },
        source: TextureSource::External(file_name),
        pixel_layout,
        use_mipmaps,
        alpha_format,
        is_static: src.is_static != 0,
    })
}

/// Slots that survive the conversion: base and glow.
pub fn kept_slots(src: &SrcTexturing) -> (Option<&TexDesc>, Option<&TexDesc>) {
    (src.base.as_ref(), src.glow.as_ref())
}

/// What was lost in `NiTexturingProperty` during the mapping; for the report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TexturingLoss {
    /// Used slots missing from the output (dark, detail, gloss, bump, decals, shader slots).
    pub dropped_slots: usize,
    /// Slots with a UV transform that cannot be represented in the output.
    pub dropped_transforms: usize,
}

fn clamp_mode(block: usize, v: u32) -> Result<ClampMode, ConvertError> {
    Ok(match v {
        0 => ClampMode::ClampSClampT,
        1 => ClampMode::ClampSWrapT,
        2 => ClampMode::WrapSClampT,
        3 => ClampMode::WrapSWrapT,
        v => return Err(unknown(block, "Clamp Mode", v)),
    })
}

/// Filter 6 (`FILTER_ANISOTROPIC`) does not exist in Morrowind: `Trilerp`, the closest in meaning,
/// is taken. The other values are the same in both versions.
fn filter_mode(block: usize, v: u32) -> Result<FilterMode, ConvertError> {
    Ok(match v {
        0 => FilterMode::Nearest,
        1 => FilterMode::Bilerp,
        2 | 6 => FilterMode::Trilerp,
        3 => FilterMode::NearestMipNearest,
        4 => FilterMode::NearestMipLerp,
        5 => FilterMode::BilerpMipNearest,
        v => return Err(unknown(block, "Filter Mode", v)),
    })
}

fn apply_mode(block: usize, v: u32) -> Result<ApplyMode, ConvertError> {
    Ok(match v {
        0 => ApplyMode::Replace,
        1 => ApplyMode::Decal,
        2 => ApplyMode::Modulate,
        3 => ApplyMode::Hilight,
        4 => ApplyMode::Hilight2,
        v => return Err(unknown(block, "Apply Mode", v)),
    })
}

fn map(
    block: usize,
    desc: &TexDesc,
    texture: NiLink<NiSourceTexture>,
) -> Result<TextureMap, ConvertError> {
    Ok(TextureMap::Map(Map {
        texture,
        clamp_mode: clamp_mode(block, desc.clamp_mode)?,
        filter_mode: filter_mode(block, desc.filter_mode)?,
        // In Morrowind the geometry keeps one UV set (set zero), see `geometry`.
        texture_index: 0,
        ..Map::default()
    }))
}

/// `NiTexturingProperty`: the base and glow slots with already converted textures.
pub fn texturing(
    block: usize,
    src: &SrcTexturing,
    base: Option<NiLink<NiSourceTexture>>,
    glow: Option<NiLink<NiSourceTexture>>,
) -> Result<(NiTexturingProperty, TexturingLoss), ConvertError> {
    let mut slots: Vec<Option<TextureMap>> = vec![None; OUT_SLOTS];
    let (base_desc, glow_desc) = kept_slots(src);
    if let (Some(desc), Some(link)) = (base_desc, base) {
        slots[SLOT_BASE] = Some(map(block, desc, link)?);
    }
    if let (Some(desc), Some(link)) = (glow_desc, glow) {
        slots[SLOT_GLOW] = Some(map(block, desc, link)?);
    }

    let occupied = [&src.dark, &src.detail, &src.gloss]
        .into_iter()
        .filter(|s| s.is_some())
        .count()
        + usize::from(src.bump.is_some())
        + src.decals.iter().filter(|s| s.is_some()).count()
        + src
            .shader_textures
            .iter()
            .filter(|s| s.map.is_some())
            .count();
    let transforms = [base_desc, glow_desc]
        .into_iter()
        .flatten()
        .filter(|d| d.transform.is_some())
        .count();

    let out = NiTexturingProperty {
        base: NiProperty {
            base: net(&src.net.name),
            flags: 0,
        },
        apply_mode: apply_mode(block, src.apply_mode)?,
        texture_maps: slots,
    };
    Ok((
        out,
        TexturingLoss {
            dropped_slots: occupied,
            dropped_transforms: transforms,
        },
    ))
}

#[cfg(test)]
mod tests;
