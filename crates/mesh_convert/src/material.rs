//! Материал: свойства Oblivion → свойства Morrowind, T1.5.
//!
//! У Oblivion, в отличие от Skyrim, нет `BSShader*`: материал задают те же свойства, что
//! у Morrowind (`NiMaterialProperty`, `NiTexturingProperty`, `NiAlphaProperty`,
//! `NiVertexColorProperty`), поэтому здесь только отображение полей 20.0.0.x → 4.0.0.2,
//! таблица которого — в `docs/ru/formats/nif-oblivion.md`. Связывание блоков между собой
//! делает [`crate::geometry`], здесь функции на один блок.
//!
//! Чего нет в выходе намеренно:
//!
//! * `NiTexturingProperty`: остались слоты base и glow. Dark, detail, gloss, bump и decal
//!   ванильный Morrowind не использует.
//! * Преобразование UV (`Has Texture Transform`): у слота Morrowind поля для него нет.
//! * Контроллеры и дополнительные данные у свойств (`NiObjectNET`) — как у узлов.

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

/// Биты `AlphaFlags`, существующие в 4.0.0.2: смешивание (0), режимы источника и приёмника
/// (1–8), тест (9) и функция теста (10–12). Биты 13–15 (`No Sorter`, `Clone Unique`,
/// `Editor Alpha Threshold`) — Bethesda-only, у Morrowind их нет.
const ALPHA_FLAGS_MASK: u16 = 0x1FFF;

/// Число слотов в выходном `NiTexturingProperty`: base, dark, detail, gloss, glow, bump,
/// decal 0. Занят из них только base и (редко) glow. Что именно столько слотов у свойств
/// ванильного Morrowind — **не проверено** на мешах `Morrowind.bsa`.
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

/// `NiMaterialProperty`: цвета, блеск и альфа — как есть, кроме нулевого ambient. Флагов в
/// 20.0.0.4 у этого блока нет, в 4.0.0.2 они есть и остаются нулевыми.
///
/// **Нулевой ambient → diffuse** (решение владельца 2026-09-30). Morrowind освещает материал
/// рассеянным светом × ambient плюс прямым × diffuse, и всё, что отвёрнуто от ламп, с ambient 0
/// выходит чисто чёрным: рукава рубашек NPC `SEWorld` в тёмных домах (игра, 2026-09-30). Oblivion
/// ambient материала, судя по виду тех же вещей в нём, не использует. В `SEWorld` таких форм 956,
/// у 371 из них (184 меша: одежда, эффекты, деревья) ambient не заменяют и цвета вершин.
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

/// `NiVertexColorProperty`: флаги и оба режима переносятся; значение вне известных —
/// ошибка, а не молчаливая замена.
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

/// `NiSpecularProperty`: флаги как есть.
pub fn specular(src: &SrcSpecular) -> NiSpecularProperty {
    NiSpecularProperty {
        base: NiProperty {
            base: net(&src.net.name),
            flags: src.flags,
        },
    }
}

/// `NiStencilProperty`: поля и их значения одинаковы в обеих версиях; у 20.0.0.4 нет только
/// `flags`, в выходе они нулевые. Главное здесь — режим двусторонности (`draw_mode`).
pub fn stencil(src: &SrcStencil) -> NiStencilProperty {
    // Экспортёры (3ds Max и др.) иногда пишут в эти поля мусор, например `1.0f` как целое
    // (1065353216). У Morrowind из стенсила важен только режим двусторонности, поэтому
    // непонятное значение заменяется значением по умолчанию, а не валит весь меш.
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

/// `NiAlphaProperty`: флаги (без Bethesda-only битов) и порог теста.
pub fn alpha(src: &SrcAlpha) -> NiAlphaProperty {
    NiAlphaProperty {
        base: NiProperty {
            base: net(&src.net.name),
            flags: src.flags & ALPHA_FLAGS_MASK,
        },
        test_ref: src.threshold,
    }
}

/// `NiSourceTexture` с внешним файлом. Внутренние текстуры (пиксели в самом NIF) в мешах
/// `STAT` не встречаются, и переносить их нечем — ошибка. `file_name` — уже итоговое имя
/// (его выбирает вызывающий).
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

/// Слоты, которые переживают конвертацию: base и glow.
pub fn kept_slots(src: &SrcTexturing) -> (Option<&TexDesc>, Option<&TexDesc>) {
    (src.base.as_ref(), src.glow.as_ref())
}

/// Что потеряно в `NiTexturingProperty` при отображении; для отчёта.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TexturingLoss {
    /// Занятые слоты, которых нет в выходе (dark, detail, gloss, bump, decals, шейдерные).
    pub dropped_slots: usize,
    /// Слоты с преобразованием UV, которое в выходе не представимо.
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

/// Фильтр 6 (`FILTER_ANISOTROPIC`) у Morrowind не существует: берётся `Trilerp`, ближайший
/// по смыслу. Остальные значения одинаковы в обеих версиях.
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
        // У Morrowind в геометрии остаётся один набор UV (нулевой), см. `geometry`.
        texture_index: 0,
        ..Map::default()
    }))
}

/// `NiTexturingProperty`: слоты base и glow с уже сконвертированными текстурами.
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
