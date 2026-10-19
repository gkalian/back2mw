use super::common::{BlockRef, ObjectNet, read_object_net, read_ref};
use super::cursor::Cursor;
use super::error::Result;

/// `NiMaterialProperty` для 20.0.0.x, `BSVER` 11: флагов нет, ambient и diffuse есть,
/// `Emissive Mult` нет.
#[derive(Debug, Clone, PartialEq)]
pub struct NiMaterialProperty {
    pub net: ObjectNet,
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
    pub specular: [f32; 3],
    pub emissive: [f32; 3],
    pub glossiness: f32,
    pub alpha: f32,
}

pub fn read_material_property(cur: &mut Cursor<'_>) -> Result<NiMaterialProperty> {
    let net = read_object_net(cur)?;
    if cur.version() <= 0x0A00_0102 {
        cur.u16()?; // флаги (до 10.0.1.2)
    }
    Ok(NiMaterialProperty {
        net,
        ambient: cur.floats()?,
        diffuse: cur.floats()?,
        specular: cur.floats()?,
        emissive: cur.floats()?,
        glossiness: cur.f32()?,
        alpha: cur.f32()?,
    })
}

/// `NiVertexColorProperty`: флаги и — до 20.0.0.5 включительно — два режима отдельными
/// полями.
#[derive(Debug, Clone, PartialEq)]
pub struct NiVertexColorProperty {
    pub net: ObjectNet,
    pub flags: u16,
    /// `SourceVertexMode`: 0 — игнорировать, 1 — эмиссия, 2 — ambient+diffuse.
    pub vertex_mode: u32,
    /// `LightingMode`: 0 — только эмиссия, 1 — эмиссия+ambient+diffuse.
    pub lighting_mode: u32,
}

pub fn read_vertex_color_property(cur: &mut Cursor<'_>) -> Result<NiVertexColorProperty> {
    Ok(NiVertexColorProperty {
        net: read_object_net(cur)?,
        flags: cur.u16()?,
        vertex_mode: cur.u32()?,
        lighting_mode: cur.u32()?,
    })
}

/// Преобразование текстурных координат (`Has Texture Transform`).
#[derive(Debug, Clone, PartialEq)]
pub struct TexTransform {
    pub translation: [f32; 2],
    pub scale: [f32; 2],
    pub rotation: f32,
    pub method: u32,
    pub center: [f32; 2],
}

/// `TexDesc` — один слот текстуры.
#[derive(Debug, Clone, PartialEq)]
pub struct TexDesc {
    pub source: BlockRef,
    /// `TexClampMode`: 0..3.
    pub clamp_mode: u32,
    /// `TexFilterMode`.
    pub filter_mode: u32,
    pub uv_set: u32,
    pub transform: Option<TexTransform>,
}

/// Поля `PS2 L`/`PS2 K` в `nif.xml` помечены `until="10.4.0.1"`: `until` включает
/// границу, то есть в 20.0.0.4 их нет. Сначала они были прочитаны по ошибке, и весь
/// файл съехал на четыре байта — это увидел контроль «курсор в конце файла». У 10.2.0.0
/// они есть (T2.3).
fn read_tex_desc(cur: &mut Cursor<'_>) -> Result<TexDesc> {
    let source = read_ref(cur)?;
    let clamp_mode = cur.u32()?;
    let filter_mode = cur.u32()?;
    let uv_set = cur.u32()?;
    if cur.version() <= 0x0A04_0001 {
        cur.bytes(4)?; // PS2 L, PS2 K
    }
    // Преобразование — с 10.1.0.0: у 10.0.1.x поля нет.
    let transform = if cur.version() >= 0x0A01_0000 && cur.bool()? {
        Some(TexTransform {
            translation: cur.floats()?,
            scale: cur.floats()?,
            rotation: cur.f32()?,
            method: cur.u32()?,
            center: cur.floats()?,
        })
    } else {
        None
    };
    Ok(TexDesc {
        source,
        clamp_mode,
        filter_mode,
        uv_set,
        transform,
    })
}

pub(super) fn read_opt_tex_desc(cur: &mut Cursor<'_>) -> Result<Option<TexDesc>> {
    if cur.bool()? {
        Ok(Some(read_tex_desc(cur)?))
    } else {
        Ok(None)
    }
}

/// Карта рельефа: текстура и параметры (Morrowind её не использует).
#[derive(Debug, Clone, PartialEq)]
pub struct BumpMap {
    pub texture: TexDesc,
    pub luma_scale: f32,
    pub luma_offset: f32,
    /// `Matrix22`, четыре `f32` в порядке файла.
    pub matrix: [f32; 4],
}

/// Шейдерная текстура (`ShaderTexDesc`).
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderTexture {
    pub map: Option<(TexDesc, u32)>,
}

/// `NiTexturingProperty`.
#[derive(Debug, Clone, PartialEq)]
pub struct NiTexturingProperty {
    pub net: ObjectNet,
    /// `ApplyMode`: 2 — modulate.
    pub apply_mode: u32,
    pub texture_count: u32,
    pub base: Option<TexDesc>,
    pub dark: Option<TexDesc>,
    pub detail: Option<TexDesc>,
    pub gloss: Option<TexDesc>,
    pub glow: Option<TexDesc>,
    pub bump: Option<BumpMap>,
    /// Декали 0–3; слот есть, только если `texture_count` его покрывает.
    pub decals: [Option<TexDesc>; 4],
    pub shader_textures: Vec<ShaderTexture>,
}

pub fn read_texturing_property(cur: &mut Cursor<'_>) -> Result<NiTexturingProperty> {
    let net = read_object_net(cur)?;
    if cur.version() <= 0x0A00_0102 {
        cur.u16()?; // флаги (до 10.0.1.2)
    }
    let apply_mode = cur.u32()?;
    let texture_count = cur.u32()?;

    let base = read_opt_tex_desc(cur)?;
    let dark = read_opt_tex_desc(cur)?;
    let detail = read_opt_tex_desc(cur)?;
    let gloss = read_opt_tex_desc(cur)?;
    let glow = read_opt_tex_desc(cur)?;

    let bump = if texture_count > 5 && cur.bool()? {
        Some(BumpMap {
            texture: read_tex_desc(cur)?,
            luma_scale: cur.f32()?,
            luma_offset: cur.f32()?,
            matrix: cur.floats()?,
        })
    } else {
        None
    };

    // Для 20.0.0.4 декаль N присутствует при `texture_count > 6 + N`.
    let mut decals: [Option<TexDesc>; 4] = [None, None, None, None];
    for (n, slot) in decals.iter_mut().enumerate() {
        let needed = 6 + u32::try_from(n).expect("индекс декали 0..4");
        if texture_count > needed {
            *slot = read_opt_tex_desc(cur)?;
        }
    }

    let num_shader = cur.len_u32()?;
    cur.check_array(num_shader, 1)?;
    let mut shader_textures = Vec::with_capacity(num_shader);
    for _ in 0..num_shader {
        let map = if cur.bool()? {
            Some((read_tex_desc(cur)?, cur.u32()?))
        } else {
            None
        };
        shader_textures.push(ShaderTexture { map });
    }

    Ok(NiTexturingProperty {
        net,
        apply_mode,
        texture_count,
        base,
        dark,
        detail,
        gloss,
        glow,
        bump,
        decals,
        shader_textures,
    })
}

/// `NiSourceTexture`. С 10.1.0.0 и у внешней, и у внутренней текстуры имя файла
/// читается, ссылка на пиксельные данные лежит всегда (у внешней — `−1`). У 10.0.1.x имя
/// есть только у внешней, у внутренней — байт `Use Internal` и ссылка, если он 1; у обеих
/// нет `Direct Render` (он с 10.1.0.103).
#[derive(Debug, Clone, PartialEq)]
pub struct NiSourceTexture {
    pub net: ObjectNet,
    pub use_external: bool,
    pub file_name: String,
    pub pixel_data: BlockRef,
    /// `FormatPrefs`: раскладка пикселей, мипмапы, альфа-формат.
    pub format_prefs: [u32; 3],
    pub is_static: u8,
    pub direct_render: bool,
}

pub fn read_source_texture(cur: &mut Cursor<'_>) -> Result<NiSourceTexture> {
    let net = read_object_net(cur)?;
    let use_external = cur.u8()? != 0;
    let (file_name, pixel_data) = if cur.version() >= 0x0A01_0000 {
        (cur.sized_string()?, read_ref(cur)?)
    } else if use_external {
        (cur.sized_string()?, BlockRef::NONE)
    } else if cur.u8()? != 0 {
        (String::new(), read_ref(cur)?) // Use Internal (до 10.0.1.3)
    } else {
        (String::new(), BlockRef::NONE)
    };
    Ok(NiSourceTexture {
        net,
        use_external,
        file_name,
        pixel_data,
        format_prefs: [cur.u32()?, cur.u32()?, cur.u32()?],
        is_static: cur.u8()?,
        direct_render: if cur.version() >= 0x0A01_0067 {
            cur.bool()?
        } else {
            true
        },
    })
}

/// `NiAlphaProperty`: флаги (`AlphaFlags` в `nif.xml`) и порог альфа-теста.
#[derive(Debug, Clone, PartialEq)]
pub struct NiAlphaProperty {
    pub net: ObjectNet,
    pub flags: u16,
    pub threshold: u8,
}

pub fn read_alpha_property(cur: &mut Cursor<'_>) -> Result<NiAlphaProperty> {
    Ok(NiAlphaProperty {
        net: read_object_net(cur)?,
        flags: cur.u16()?,
        threshold: cur.u8()?,
    })
}
