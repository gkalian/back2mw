use super::*;
use crate::read::common::{BlockRef, ObjectNet};

fn net(name: &str) -> ObjectNet {
    ObjectNet {
        name: name.to_owned(),
        extra_data: vec![BlockRef(Some(9))],
        controller: BlockRef(Some(8)),
    }
}

fn desc(source: u32, filter: u32, uv_set: u32, transform: bool) -> TexDesc {
    TexDesc {
        source: BlockRef(Some(source)),
        clamp_mode: 1,
        filter_mode: filter,
        uv_set,
        transform: transform.then_some(crate::read::properties::TexTransform {
            translation: [0.0, 0.0],
            scale: [1.0, 1.0],
            rotation: 0.0,
            method: 0,
            center: [0.5, 0.5],
        }),
    }
}

/// Two different texture references.
fn links() -> (NiLink<NiSourceTexture>, NiLink<NiSourceTexture>) {
    let mut stream = tes3::nif::NiStream::new();
    (
        stream.insert(NiSourceTexture::default()),
        stream.insert(NiSourceTexture::default()),
    )
}

fn texturing_source() -> SrcTexturing {
    SrcTexturing {
        net: net("tex"),
        apply_mode: 2,
        texture_count: 7,
        base: Some(desc(1, 2, 0, false)),
        dark: None,
        detail: None,
        gloss: None,
        glow: None,
        bump: None,
        decals: [None, None, None, None],
        shader_textures: Vec::new(),
    }
}

#[test]
fn material_keeps_colours_shine_and_alpha() {
    let out = material(&SrcMaterial {
        net: net("mat"),
        ambient: [0.1, 0.2, 0.3],
        diffuse: [0.4, 0.5, 0.6],
        specular: [0.7, 0.8, 0.9],
        emissive: [0.0, 0.0, 0.1],
        glossiness: 25.0,
        alpha: 0.5,
    });
    assert_eq!(out.base.base.name, "mat");
    assert!(
        out.base.base.extra_data.is_null() && out.base.base.controller.is_null(),
        "extra data and the controller are not carried over"
    );
    assert_eq!(out.ambient_color.to_array(), [0.1, 0.2, 0.3]);
    assert_eq!(out.diffuse_color.to_array(), [0.4, 0.5, 0.6]);
    assert_eq!(out.specular_color.to_array(), [0.7, 0.8, 0.9]);
    assert_eq!(out.emissive_color.to_array(), [0.0, 0.0, 0.1]);
    assert_eq!((out.shine, out.alpha), (25.0, 0.5));
}

/// Ambient 0 in Morrowind is a black shadow (the sleeves of `SEWorld` NPCs): it becomes diffuse.
#[test]
fn a_zero_ambient_takes_the_diffuse_colour() {
    let src = |ambient| SrcMaterial {
        net: net("mat"),
        ambient,
        diffuse: [0.4, 0.5, 0.6],
        specular: [0.0; 3],
        emissive: [0.0; 3],
        glossiness: 10.0,
        alpha: 1.0,
    };
    assert_eq!(
        material(&src([0.0; 3])).ambient_color.to_array(),
        [0.4, 0.5, 0.6]
    );
    assert_eq!(
        material(&src([0.0, 0.0, 0.01])).ambient_color.to_array(),
        [0.0, 0.0, 0.01],
        "a non-zero ambient stays as is"
    );
}

#[test]
fn vertex_color_maps_flags_and_modes() {
    let out = vertex_color(
        4,
        &SrcVertexColor {
            net: net("vc"),
            flags: 0x28,
            vertex_mode: 2,
            lighting_mode: 0,
        },
    )
    .unwrap();
    assert_eq!(out.base.flags, 0x28);
    assert_eq!(out.source_vertex_mode, SourceVertexMode::AmbientDiffuse);
    assert_eq!(out.lighting_mode, LightingMode::Emissive);
}

#[test]
fn vertex_color_with_an_unknown_mode_is_an_error() {
    let bad = SrcVertexColor {
        net: net("vc"),
        flags: 0,
        vertex_mode: 3,
        lighting_mode: 1,
    };
    assert_eq!(
        vertex_color(4, &bad).unwrap_err(),
        ConvertError::UnknownValue {
            block: 4,
            field: "Vertex Mode",
            value: 3
        }
    );
}

#[test]
fn alpha_keeps_test_bits_and_threshold_and_drops_bethesda_only_bits() {
    // 0x12ED — typical Oblivion flags; bits 13–15 do not exist in 4.0.0.2.
    let out = alpha(&SrcAlpha {
        net: net("alpha"),
        flags: 0xE000 | 0x12ED,
        threshold: 128,
    });
    assert_eq!(out.base.flags, 0x12ED);
    assert_eq!(out.test_ref, 128);
    assert!(out.alpha_blending());
    assert!(out.alpha_testing());
}

fn source_texture_source(external: bool, prefs: [u32; 3]) -> SrcSourceTexture {
    SrcSourceTexture {
        net: net("t"),
        use_external: external,
        file_name: "ignored.dds".to_owned(),
        pixel_data: BlockRef::NONE,
        format_prefs: prefs,
        is_static: 1,
        direct_render: true,
    }
}

#[test]
fn source_texture_takes_the_name_chosen_by_the_caller() {
    let out = source_texture(
        3,
        &source_texture_source(true, [6, 1, 3]),
        "ob_x.dds".to_owned(),
    )
    .unwrap();
    assert_eq!(out.source, TextureSource::External("ob_x.dds".to_owned()));
    assert_eq!(out.pixel_layout, PixelLayout::Default);
    assert_eq!(out.use_mipmaps, UseMipMaps::Yes);
    assert_eq!(out.alpha_format, AlphaFormat::Default);
    assert!(out.is_static);
}

#[test]
fn internal_texture_is_an_error() {
    assert_eq!(
        source_texture(3, &source_texture_source(false, [6, 1, 3]), String::new()).unwrap_err(),
        ConvertError::InternalTexture { index: 3 }
    );
}

#[test]
fn source_texture_with_an_unknown_format_value_is_an_error() {
    assert_eq!(
        source_texture(3, &source_texture_source(true, [6, 1, 9]), String::new()).unwrap_err(),
        ConvertError::UnknownValue {
            block: 3,
            field: "Alpha Format",
            value: 9
        }
    );
}

#[test]
fn texturing_keeps_base_and_glow_in_their_slots() {
    let (a, b) = links();
    let mut src = texturing_source();
    src.glow = Some(desc(2, 4, 0, false));
    let (out, loss) = texturing(5, &src, Some(a), Some(b)).unwrap();

    assert_eq!(out.apply_mode, ApplyMode::Modulate);
    assert_eq!(out.texture_maps.len(), OUT_SLOTS);
    let Some(TextureMap::Map(base)) = &out.texture_maps[SLOT_BASE] else {
        panic!("the base slot is empty");
    };
    assert_eq!(base.texture, a);
    assert_eq!(base.clamp_mode, ClampMode::ClampSWrapT);
    assert_eq!(base.filter_mode, FilterMode::Trilerp);
    assert_eq!(base.texture_index, 0);
    let Some(TextureMap::Map(glow)) = &out.texture_maps[SLOT_GLOW] else {
        panic!("the glow slot is empty");
    };
    assert_eq!(glow.texture, b);
    assert_eq!(glow.filter_mode, FilterMode::NearestMipLerp);
    assert_eq!(loss, TexturingLoss::default());
    let filled = out.texture_maps.iter().flatten().count();
    assert_eq!(filled, 2);
}

#[test]
fn texturing_drops_other_slots_and_counts_them() {
    let (a, _) = links();
    let mut src = texturing_source();
    src.dark = Some(desc(2, 2, 0, false));
    src.gloss = Some(desc(3, 2, 0, false));
    src.decals[0] = Some(desc(4, 2, 0, false));
    src.base = Some(desc(1, 2, 0, true));
    let (out, loss) = texturing(5, &src, Some(a), None).unwrap();
    assert_eq!(out.texture_maps.iter().flatten().count(), 1);
    assert_eq!(
        loss,
        TexturingLoss {
            dropped_slots: 3,
            dropped_transforms: 1
        }
    );
}

#[test]
fn anisotropic_filter_becomes_trilerp_and_other_unknowns_are_errors() {
    let (a, _) = links();
    let mut src = texturing_source();
    src.base = Some(desc(1, 6, 0, false));
    let (out, _) = texturing(5, &src, Some(a), None).unwrap();
    let Some(TextureMap::Map(base)) = &out.texture_maps[SLOT_BASE] else {
        panic!("the base slot is empty");
    };
    assert_eq!(base.filter_mode, FilterMode::Trilerp);

    src.base = Some(desc(1, 7, 0, false));
    assert_eq!(
        texturing(5, &src, Some(a), None).unwrap_err(),
        ConvertError::UnknownValue {
            block: 5,
            field: "Filter Mode",
            value: 7
        }
    );
}

#[test]
fn texturing_without_any_kept_slot_is_empty() {
    let mut src = texturing_source();
    src.base = None;
    let (out, _) = texturing(5, &src, None, None).unwrap();
    assert!(out.texture_maps.iter().all(Option::is_none));
}

#[test]
fn stencil_keeps_double_sided_mode_and_every_field() {
    use crate::read::shapes::NiStencilProperty as Src;
    let src = Src {
        net: net("st"),
        enabled: true,
        function: 4,
        reference: 3,
        mask: 0xFFFF_FFFF,
        fail_action: 0,
        z_fail_action: 1,
        pass_action: 3,
        draw_mode: 3,
    };
    let out = stencil(&src);
    assert_eq!(out.draw_mode, DrawMode::Both);
    assert_eq!(out.stencil_function, StencilTestFunction::Greater);
    assert_eq!(out.pass_action, Action::Increment);
    assert_eq!(out.pass_z_fail_action, Action::Zero);
    assert_eq!((out.stencil_ref, out.stencil_mask), (3, 0xFFFF_FFFF));
    assert!(out.stencil_enabled);

    // Junk from an exporter (float 1.0 in an integer field) does not fail the mesh but gives
    // default values.
    let junk = Src {
        function: 1_065_353_216,
        fail_action: 1_065_353_216,
        draw_mode: 1_065_353_216,
        ..src
    };
    let out = stencil(&junk);
    assert_eq!(out.draw_mode, DrawMode::Default);
    assert_eq!(out.fail_action, Action::Keep);
    assert_eq!(out.stencil_function, StencilTestFunction::Never);
}

#[test]
fn specular_keeps_flags() {
    use crate::read::shapes::NiSpecularProperty as Src;
    let out = specular(&Src {
        net: net("sp"),
        flags: 1,
    });
    assert_eq!(out.base.flags, 1);
}
