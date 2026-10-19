use super::*;
use crate::read::common::ObjectNet;
use crate::read::header::{NifHeader, VERSION_20_0_0_4};
use crate::read::{NiNode as SrcNode, NiTriStrips as SrcShape};

fn header(blocks: usize) -> NifHeader {
    NifHeader {
        line: "Gamebryo File Format, Version 20.0.0.4".to_owned(),
        version: VERSION_20_0_0_4,
        user_version: 11,
        user_version_2: 11,
        export_info: Default::default(),
        block_types: vec!["X".to_owned()],
        block_type_index: vec![0; blocks],
        groups: Vec::new(),
        body_offset: 0,
    }
}

fn av(name: &str) -> AvObject {
    AvObject {
        net: ObjectNet {
            name: name.to_owned(),
            extra_data: Vec::new(),
            controller: BlockRef::NONE,
        },
        flags: 0x000E,
        translation: [1.0, 2.0, 3.0],
        rotation: [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        scale: 2.0,
        properties: Vec::new(),
        collision_object: BlockRef::NONE,
    }
}

fn node(name: &str, children: &[u32]) -> Block {
    Block::Node(SrcNode {
        av: av(name),
        children: children.iter().map(|&i| BlockRef(Some(i))).collect(),
        effects: Vec::new(),
    })
}

fn shape(name: &str, data: u32) -> Block {
    Block::TriStrips(SrcShape {
        av: av(name),
        data: BlockRef(Some(data)),
        skin_instance: BlockRef::NONE,
        shader_name: None,
        shader_extra_data: None,
    })
}

/// Данные формы: `vertices` вершин, заданные полосы и `uv_sets` наборов UV.
fn strips_data(strips: Vec<Vec<u16>>, vertices: usize, uv_sets: usize) -> Block {
    Block::TriStripsData(Box::new(NiTriStripsData {
        group_id: 0,
        num_vertices: u16::try_from(vertices).unwrap(),
        keep_flags: 0,
        compress_flags: 0,
        vertices: (0..vertices)
            .map(|i| [f32::from(u16::try_from(i).unwrap() % 100), 0.0, 1.0])
            .collect(),
        data_flags: 0,
        normals: vec![[0.0, 0.0, 1.0]; vertices],
        tangents: vec![[1.0, 0.0, 0.0]; vertices],
        bitangents: vec![[0.0, 1.0, 0.0]; vertices],
        bound_center: [0.5, 0.5, 0.0],
        bound_radius: 1.5,
        colors: vec![[1.0, 0.5, 0.25, 1.0]; vertices],
        uv_sets: (0..uv_sets)
            .map(|s| vec![[f32::from(u16::try_from(s).unwrap()), 0.5]; vertices])
            .collect(),
        consistency_flags: 0,
        additional_data: BlockRef::NONE,
        num_triangles: 0,
        strips,
        triangles: Vec::new(),
    }))
}

fn nif(blocks: Vec<Block>, roots: &[u32]) -> SourceNif {
    SourceNif {
        header: header(blocks.len()),
        blocks,
        roots: roots.iter().map(|&i| BlockRef(Some(i))).collect(),
    }
}

/// Узел с одной формой: типичный меш статики.
fn simple(strips: Vec<Vec<u16>>, vertices: usize, uv_sets: usize) -> SourceNif {
    nif(
        vec![
            node("Root", &[1]),
            shape("Rock:0", 2),
            strips_data(strips, vertices, uv_sets),
        ],
        &[0],
    )
}

#[test]
fn strip_becomes_triangles_with_alternating_winding() {
    let (tris, degenerate) = strips_to_triangles(&[vec![0, 1, 2, 3]]);
    assert_eq!(tris, [[0, 1, 2], [2, 1, 3]]);
    assert_eq!(degenerate, 0);
}

#[test]
fn degenerate_triangles_are_dropped_but_parity_follows_the_strip() {
    // (0,1,2) годен, (1,2,2) и (2,2,3) вырождены, (2,3,4) — i = 3, нечётный
    let strips = [vec![0, 1, 2, 2, 3, 4]];
    let (tris, degenerate) = strips_to_triangles(&strips);
    assert_eq!(tris, [[0, 1, 2], [3, 2, 4]]);
    assert_eq!(degenerate, 2);
    assert_eq!(strip_triangle_count(&strips), 4);
}

#[test]
fn strips_shorter_than_three_points_give_nothing() {
    let strips = [vec![], vec![7], vec![7, 8]];
    assert_eq!(strips_to_triangles(&strips), (vec![], 0));
    assert_eq!(strip_triangle_count(&strips), 0);
}

#[test]
fn several_strips_are_concatenated_each_with_its_own_parity() {
    let (tris, _) = strips_to_triangles(&[vec![0, 1, 2], vec![3, 4, 5, 6]]);
    assert_eq!(tris, [[0, 1, 2], [3, 4, 5], [5, 4, 6]]);
}

#[test]
fn converts_a_node_with_a_shape_and_reads_back() {
    let source = simple(vec![vec![0, 1, 2, 3], vec![2, 3, 0]], 4, 2);
    let mut converted = convert(&source).unwrap();
    assert_eq!(
        converted.stats,
        Stats {
            shapes: 1,
            strip_triangles: 3,
            degenerate: 0,
            triangles: 3,
            ..Stats::default()
        }
    );

    let bytes = converted.save_bytes().unwrap();
    let back = NiStream::from_bytes(&bytes).unwrap();

    let roots: Vec<_> = back.roots_of_type::<NiNode>().collect();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].base.base.name, "Root");
    assert_eq!(roots[0].base.flags, 0x000E);
    assert_eq!(roots[0].base.translation.to_array(), [1.0, 2.0, 3.0]);
    assert_eq!(roots[0].base.scale, 2.0);
    assert_eq!(roots[0].children.len(), 1);
    assert!(roots[0].effects.is_empty());

    let shapes: Vec<_> = back.objects_of_type::<NiTriShape>().collect();
    assert_eq!(shapes.len(), 1);
    assert_eq!(shapes[0].base.base.base.base.name, "Rock:0");

    let data: Vec<_> = back.objects_of_type::<NiTriShapeData>().collect();
    assert_eq!(data.len(), 1);
    let geometry = &data[0].base.base;
    assert_eq!(geometry.vertices.len(), 4);
    assert_eq!(geometry.normals.len(), 4);
    assert_eq!(geometry.vertex_colors.len(), 4);
    assert_eq!(geometry.bound.radius, 1.5);
    assert_eq!(data[0].triangles, [[0, 1, 2], [2, 1, 3], [2, 3, 0]]);
    // из двух наборов UV остался только первый
    assert_eq!(geometry.num_uv_sets(), 1);
    assert_eq!(geometry.uv_set(0).unwrap()[0].to_array(), [0.0, 0.5]);
}

#[test]
fn triangle_count_equals_strip_arithmetic_minus_degenerate() {
    let source = simple(vec![vec![0, 1, 2, 2, 3, 4], vec![4, 3, 2]], 5, 1);
    let s = convert(&source).unwrap().stats;
    assert_eq!(s.strip_triangles, 4 + 1);
    assert_eq!(s.degenerate, 2);
    assert_eq!(s.triangles, s.strip_triangles - s.degenerate);
}

#[test]
fn a_shape_without_uv_sets_colours_or_normals_converts_without_them() {
    let mut source = simple(vec![vec![0, 1, 2]], 3, 0);
    if let Block::TriStripsData(d) = &mut source.blocks[2] {
        d.colors.clear();
        d.normals.clear();
    }
    let mut converted = convert(&source).unwrap();
    let back = NiStream::from_bytes(&converted.save_bytes().unwrap()).unwrap();
    let data = back.objects_of_type::<NiTriShapeData>().next().unwrap();
    assert_eq!(data.base.base.num_uv_sets(), 0);
    assert!(data.base.base.vertex_colors.is_empty());
    assert!(data.base.base.normals.is_empty());
}

#[test]
fn rotation_bytes_are_copied_verbatim() {
    let source = nif(vec![node("R", &[])], &[0]);
    let bytes = convert(&source).unwrap().save_bytes().unwrap();
    let expected: Vec<u8> = [0.0f32, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    assert!(
        bytes.windows(expected.len()).any(|w| w == expected),
        "девять чисел поворота не легли в файл теми же байтами в том же порядке"
    );
}

#[test]
fn node_names_starting_with_collision_are_guarded() {
    // `CollisionBox01` Oblivion: корень с таким именем роняет Construction Set (`crate::node_name`).
    let source = nif(
        vec![
            node("CollisionBox01", &[1, 2]),
            node("collisionInner", &[]),
            shape("Collision:0", 3),
            strips_data(vec![vec![0, 1, 2]], 3, 1),
        ],
        &[0],
    );
    let back = NiStream::from_bytes(&convert(&source).unwrap().save_bytes().unwrap()).unwrap();
    let mut names: Vec<&str> = back
        .objects_of_type::<NiAVObject>()
        .map(|o| o.base.name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["ob_Collision:0", "ob_CollisionBox01", "ob_collisionInner"]
    );
    // Контроль: обычные имена остаются как были.
    let plain = nif(vec![node("Root", &[])], &[0]);
    let back = NiStream::from_bytes(&convert(&plain).unwrap().save_bytes().unwrap()).unwrap();
    assert_eq!(
        back.roots_of_type::<NiNode>()
            .next()
            .unwrap()
            .base
            .base
            .name,
        "Root"
    );
}

#[test]
fn a_shared_child_stays_shared() {
    let source = nif(
        vec![
            node("R", &[1, 1]),
            shape("S", 2),
            strips_data(vec![vec![0, 1, 2]], 3, 1),
        ],
        &[0],
    );
    let mut converted = convert(&source).unwrap();
    assert_eq!(converted.stats.shapes, 1);
    let back = NiStream::from_bytes(&converted.save_bytes().unwrap()).unwrap();
    assert_eq!(back.objects_of_type::<NiTriShape>().count(), 1);
    let root = back.roots_of_type::<NiNode>().next().unwrap();
    assert_eq!(root.children.len(), 2);
    assert_eq!(root.children[0], root.children[1]);
}

#[test]
fn editor_markers_are_dropped_with_their_subtree() {
    let source = nif(
        vec![
            node("Root", &[1, 2, 5]),
            shape("Wall", 3),
            node("EditorMarker", &[4]),
            strips_data(vec![vec![0, 1, 2]], 3, 1),
            shape("Box", 3),
            shape("EditorMarkerC:0", 3),
        ],
        &[0],
    );
    let mut converted = convert(&source).unwrap();
    assert_eq!(converted.stats.dropped_editor_markers, 2);
    let back = NiStream::from_bytes(&converted.save_bytes().unwrap()).unwrap();
    let root = back.roots_of_type::<NiNode>().next().unwrap();
    assert_eq!(root.children.len(), 1, "осталась только стена");
    assert_eq!(back.objects_of_type::<NiTriShape>().count(), 1);
}

#[test]
fn output_is_deterministic() {
    let make = || {
        nif(
            vec![
                node("R", &[1, 3]),
                shape("A", 2),
                strips_data(vec![vec![0, 1, 2, 3]], 4, 1),
                shape("B", 4),
                strips_data(vec![vec![0, 1, 2]], 3, 1),
            ],
            &[0],
        )
    };
    let a = convert(&make()).unwrap().save_bytes().unwrap();
    let b = convert(&make()).unwrap().save_bytes().unwrap();
    assert_eq!(a, b);
}

#[test]
fn no_roots_is_an_error() {
    let source = nif(vec![node("R", &[])], &[]);
    assert_eq!(convert(&source).unwrap_err(), ConvertError::NoRoots);
}

#[test]
fn an_index_outside_the_vertex_array_is_an_error() {
    let source = simple(vec![vec![0, 1, 9]], 3, 1);
    assert_eq!(
        convert(&source).unwrap_err(),
        ConvertError::IndexOutOfRange {
            shape: 2,
            index: 9,
            vertices: 3
        }
    );
}

#[test]
fn a_cycle_is_an_error_not_a_stack_overflow() {
    let source = nif(vec![node("R", &[0])], &[0]);
    assert_eq!(
        convert(&source).unwrap_err(),
        ConvertError::Cycle { index: 0 }
    );
}

#[test]
fn a_dangling_child_is_an_error() {
    let source = nif(vec![node("R", &[5])], &[0]);
    assert_eq!(
        convert(&source).unwrap_err(),
        ConvertError::DanglingRef {
            from: 0,
            to: Some(5)
        }
    );
}

#[test]
fn a_root_that_is_not_a_node_or_shape_is_an_error() {
    let source = nif(
        vec![Block::BsxFlags(crate::read::BsxFlags {
            name: "BSX".to_owned(),
            flags: 0,
        })],
        &[0],
    );
    assert!(matches!(
        convert(&source).unwrap_err(),
        ConvertError::UnexpectedBlock { index: 0, .. }
    ));
}

#[test]
fn shape_data_must_be_strips_data() {
    let source = nif(vec![node("R", &[1]), shape("S", 0)], &[0]);
    assert!(matches!(
        convert(&source).unwrap_err(),
        ConvertError::UnexpectedBlock {
            index: 0,
            expected: "NiTriStripsData or NiTriShapeData"
        }
    ));
}

/// Полоса `0,1,2,0,1,2…`: все тройки различны, вырожденных нет.
fn cycling_strip(points: u32) -> Vec<u16> {
    (0..points).map(|i| u16::try_from(i % 3).unwrap()).collect()
}

#[test]
fn more_than_65535_triangles_is_an_error() {
    let source = simple(vec![cycling_strip(65_538)], 3, 1);
    assert_eq!(
        convert(&source).unwrap_err(),
        ConvertError::TooManyTriangles {
            shape: 2,
            triangles: 65_536
        }
    );
}

#[test]
fn exactly_65535_triangles_fit() {
    let source = simple(vec![cycling_strip(65_537)], 3, 1);
    let mut converted = convert(&source).unwrap();
    assert_eq!(converted.stats.triangles, 65_535);
    let back = NiStream::from_bytes(&converted.save_bytes().unwrap()).unwrap();
    let data = back.objects_of_type::<NiTriShapeData>().next().unwrap();
    assert_eq!(data.triangles.len(), 65_535);
}

#[test]
fn a_name_outside_cp1252_is_an_error_on_save_not_a_silent_replacement() {
    let mut source = nif(vec![node("R", &[])], &[0]);
    if let Block::Node(n) = &mut source.blocks[0] {
        n.av.net.name = "Крыша".to_owned();
    }
    let mut converted = convert(&source).unwrap();
    assert!(converted.save_bytes().is_err());
}

// --- Свойства (T1.5) ---------------------------------------------------------------

use crate::read::properties::TexDesc;
use crate::read::{
    NiAlphaProperty as SrcAlpha, NiMaterialProperty as SrcMaterial,
    NiSourceTexture as SrcSourceTexture, NiTexturingProperty as SrcTexturing,
};
use tes3::nif::{
    NiAlphaProperty, NiMaterialProperty, NiSourceTexture, NiTexturingProperty, NiTriShapeData,
    TextureSource,
};

fn net(name: &str) -> ObjectNet {
    ObjectNet {
        name: name.to_owned(),
        extra_data: Vec::new(),
        controller: BlockRef::NONE,
    }
}

fn shape_with(name: &str, data: u32, props: &[u32]) -> Block {
    let Block::TriStrips(mut s) = shape(name, data) else {
        unreachable!()
    };
    s.av.properties = props.iter().map(|&i| BlockRef(Some(i))).collect();
    Block::TriStrips(s)
}

fn texturing(base_source: u32, uv_set: u32, glow_uv: Option<u32>) -> Block {
    let desc = |source, uv_set| TexDesc {
        source: BlockRef(Some(source)),
        clamp_mode: 3,
        filter_mode: 2,
        uv_set,
        transform: None,
    };
    Block::TexturingProperty(Box::new(SrcTexturing {
        net: net("tex"),
        apply_mode: 2,
        texture_count: 7,
        base: Some(desc(base_source, uv_set)),
        dark: None,
        detail: None,
        gloss: None,
        glow: glow_uv.map(|uv| desc(base_source, uv)),
        bump: None,
        decals: [None, None, None, None],
        shader_textures: Vec::new(),
    }))
}

fn material_block() -> Block {
    Block::MaterialProperty(SrcMaterial {
        net: net("mat"),
        ambient: [1.0; 3],
        diffuse: [0.5; 3],
        specular: [0.0; 3],
        emissive: [0.0; 3],
        glossiness: 10.0,
        alpha: 1.0,
    })
}

fn alpha_block() -> Block {
    Block::AlphaProperty(SrcAlpha {
        net: net("alpha"),
        flags: 0x12ED,
        threshold: 128,
    })
}

fn texture_block(file: &str) -> Block {
    Block::SourceTexture(SrcSourceTexture {
        net: net("t"),
        use_external: true,
        file_name: file.to_owned(),
        pixel_data: BlockRef::NONE,
        format_prefs: [6, 1, 3],
        is_static: 1,
        direct_render: true,
    })
}

/// Два узла-формы, делящие материал; у первой ещё текстура и альфа.
///
/// 0 корень, 1 форма A, 2 данные A, 3 текстурирование, 4 материал, 5 альфа, 6 текстура,
/// 7 форма B, 8 данные B.
fn with_properties(uv_set: u32, glow_uv: Option<u32>) -> SourceNif {
    nif(
        vec![
            node("R", &[1, 7]),
            shape_with("A", 2, &[3, 4, 5]),
            strips_data(vec![vec![0, 1, 2]], 3, 2),
            texturing(6, uv_set, glow_uv),
            material_block(),
            alpha_block(),
            texture_block("Textures\\Rocks\\Rock01.DDS"),
            shape_with("B", 8, &[4]),
            strips_data(vec![vec![0, 1, 2]], 3, 2),
        ],
        &[0],
    )
}

#[test]
fn properties_are_converted_linked_and_shared() {
    let mut converted = convert(&with_properties(0, None)).unwrap();
    let s = converted.stats;
    assert_eq!(
        (s.shapes, s.properties, s.textures),
        (2, 3, 1),
        "материал общий и конвертируется один раз"
    );
    let back = NiStream::from_bytes(&converted.save_bytes().unwrap()).unwrap();
    assert_eq!(back.objects_of_type::<NiMaterialProperty>().count(), 1);
    assert_eq!(back.objects_of_type::<NiTexturingProperty>().count(), 1);
    let alpha: Vec<_> = back.objects_of_type::<NiAlphaProperty>().collect();
    assert_eq!(alpha.len(), 1);
    assert_eq!((alpha[0].base.flags, alpha[0].test_ref), (0x12ED, 128));
    let textures: Vec<_> = back.objects_of_type::<NiSourceTexture>().collect();
    assert_eq!(textures.len(), 1);
    assert_eq!(
        textures[0].source,
        TextureSource::External("ob_rocks_rock01.dds".to_owned())
    );
    assert_eq!(
        converted.textures,
        [TextureCopy {
            source_path: "textures\\rocks\\rock01.dds".to_owned(),
            target_name: "ob_rocks_rock01.dds".to_owned(),
        }]
    );

    let shapes: Vec<_> = back.objects_of_type::<NiTriShape>().collect();
    assert_eq!(shapes[0].base.base.base.properties.len(), 3);
    assert_eq!(shapes[1].base.base.base.properties.len(), 1);
    assert_eq!(
        shapes[0].base.base.base.properties[1], shapes[1].base.base.base.properties[0],
        "общий материал остался общим"
    );
}

#[test]
fn the_uv_set_of_the_base_texture_is_the_one_kept() {
    let mut converted = convert(&with_properties(1, None)).unwrap();
    assert_eq!(converted.stats.uv_set_switched, 1);
    let back = NiStream::from_bytes(&converted.save_bytes().unwrap()).unwrap();
    let data: Vec<_> = back.objects_of_type::<NiTriShapeData>().collect();
    // В данных набор `s` состоит из точек с u = s; форма A выбрала набор 1, форма B — 0.
    assert_eq!(data[0].base.base.uv_sets[0].x, 1.0);
    assert_eq!(data[1].base.base.uv_sets[0].x, 0.0);
}

#[test]
fn a_glow_map_on_another_uv_set_is_counted() {
    let converted = convert(&with_properties(0, Some(1))).unwrap();
    assert_eq!(converted.stats.glow_uv_mismatch, 1);
}

#[test]
fn a_uv_set_missing_from_the_data_is_an_error() {
    let err = convert(&with_properties(5, None)).unwrap_err();
    assert!(
        matches!(
            err,
            ConvertError::MissingUvSet {
                set: 5,
                sets: 2,
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn a_property_reference_to_a_non_property_is_an_error() {
    let source = nif(
        vec![
            node("R", &[1]),
            shape_with("A", 2, &[2]),
            strips_data(vec![vec![0, 1, 2]], 3, 1),
        ],
        &[0],
    );
    assert!(matches!(
        convert(&source).unwrap_err(),
        ConvertError::UnexpectedBlock { index: 2, .. }
    ));
}

#[test]
fn properties_on_nodes_are_converted_too() {
    let mut root = node("R", &[1]);
    if let Block::Node(n) = &mut root {
        n.av.properties = vec![BlockRef(Some(3))];
    }
    let mut converted = convert(&nif(
        vec![
            root,
            shape("A", 2),
            strips_data(vec![vec![0, 1, 2]], 3, 1),
            material_block(),
        ],
        &[0],
    ))
    .unwrap();
    let back = NiStream::from_bytes(&converted.save_bytes().unwrap()).unwrap();
    let root = back.roots_of_type::<NiNode>().next().unwrap();
    assert_eq!(root.base.properties.len(), 1);
}

// --- Текстуры (T1.6) ---------------------------------------------------------------

/// Меш с одной формой, у которой одна base-текстура с данным путём.
fn with_texture(path: &str) -> SourceNif {
    nif(
        vec![
            node("R", &[1]),
            shape_with("A", 2, &[3]),
            strips_data(vec![vec![0, 1, 2]], 3, 1),
            texturing(4, 0, None),
            texture_block(path),
        ],
        &[0],
    )
}

#[test]
fn a_texture_shared_by_two_slots_is_copied_once() {
    let source = nif(
        vec![
            node("R", &[1]),
            shape_with("A", 2, &[3]),
            strips_data(vec![vec![0, 1, 2]], 3, 1),
            // base и glow ссылаются на один и тот же блок текстуры
            texturing(4, 0, Some(0)),
            texture_block("textures/a/b.dds"),
        ],
        &[0],
    );
    let converted = convert(&source).unwrap();
    assert_eq!(converted.stats.textures, 1);
    assert_eq!(converted.textures.len(), 1);
}

#[test]
fn a_collision_between_two_meshes_is_found_by_the_shared_registry() {
    let mut registry = TextureRegistry::new();
    convert_with(&with_texture("textures/a_b/c.dds"), &mut registry).unwrap();
    let err = convert_with(&with_texture("textures/a/b_c.dds"), &mut registry).unwrap_err();
    assert!(
        matches!(
            err,
            ConvertError::Texture {
                error: TextureError::Collision { .. },
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn a_separate_registry_per_call_does_not_see_other_meshes() {
    convert(&with_texture("textures/a_b/c.dds")).unwrap();
    convert(&with_texture("textures/a/b_c.dds")).unwrap();
}

#[test]
fn a_normal_map_in_a_kept_slot_is_an_error() {
    // Ребёнок узла с такой текстурой выбрасывается как искажение (T2.3b), поэтому форма здесь —
    // сам корень: его не выбросить, и ошибка остаётся.
    let source = nif(
        vec![
            shape_with("A", 1, &[2]),
            strips_data(vec![vec![0, 1, 2]], 3, 1),
            texturing(3, 0, None),
            texture_block("textures/a/wall_n.dds"),
        ],
        &[0],
    );
    let err = convert(&source).unwrap_err();
    assert!(
        matches!(
            err,
            ConvertError::Texture {
                error: TextureError::NormalMap { .. },
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn an_internal_texture_is_an_error_before_its_name_is_looked_at() {
    let mut source = with_texture("");
    if let Block::SourceTexture(t) = &mut source.blocks[4] {
        t.use_external = false;
    }
    assert_eq!(
        convert(&source).unwrap_err(),
        ConvertError::InternalTexture { index: 4 }
    );
}

// --- Коллизия (T1.7) ---------------------------------------------------------------

use crate::read::havok::{
    BhkCollisionObject, BhkMoppBvTreeShape, BhkNiTriStripsShape, BhkRigidBody, HavokFilter,
};
use tes3::nif::RootCollisionNode;

fn rigid_body(shape: u32, translated: bool, translation: [f32; 3], rotation: [f32; 4]) -> Block {
    Block::RigidBody(BhkRigidBody {
        translated,
        shape: BlockRef(Some(shape)),
        filter: HavokFilter {
            layer: 1,
            flags: 0,
            group: 0,
        },
        translation: [translation[0], translation[1], translation[2], 0.0],
        rotation,
        mass: 0.0,
        friction: 0.5,
        restitution: 0.4,
        motion_system: 7,
        quality_type: 1,
        constraints: Vec::new(),
        body_flags: 0,
    })
}

fn tri_strips_shape(data: &[u32]) -> Block {
    Block::NiTriStripsShape(BhkNiTriStripsShape {
        material: 0,
        radius: 0.1,
        grow_by: 1,
        scale: [1.0, 1.0, 1.0, 0.0],
        strips_data: data.iter().map(|&i| BlockRef(Some(i))).collect(),
        filters: Vec::new(),
    })
}

fn mopp(shape: u32) -> Block {
    Block::MoppBvTreeShape(BhkMoppBvTreeShape {
        shape: BlockRef(Some(shape)),
        scale: 1.0,
        mopp_size: 0,
        mopp_offset: [0.0; 4],
    })
}

const IDENTITY_Q: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Меш с коллизией у корня. 0 корень, 1 форма, 2 её данные, 3 объект коллизии, 4 тело;
/// без `mopp`: 5 форма коллизии, 6 её данные; с `mopp`: 5 `mopp`, 6 форма, 7 данные.
/// Данные коллизии — одна полоса из четырёх вершин (квадрат со стороной 1 в плоскости XY).
fn with_collision(
    through_mopp: bool,
    translated: bool,
    translation: [f32; 3],
    rotation: [f32; 4],
) -> SourceNif {
    let mut root = node("R", &[1]);
    if let Block::Node(n) = &mut root {
        n.av.collision_object = BlockRef(Some(3));
    }
    let mut blocks = vec![
        root,
        shape("S", 2),
        strips_data(vec![vec![0, 1, 2]], 3, 1),
        Block::CollisionObject(BhkCollisionObject {
            target: BlockRef(Some(0)),
            flags: 41,
            body: BlockRef(Some(4)),
        }),
        rigid_body(5, translated, translation, rotation),
    ];
    if through_mopp {
        blocks.push(mopp(6));
        blocks.push(tri_strips_shape(&[7]));
    } else {
        blocks.push(tri_strips_shape(&[6]));
    }
    blocks.push(collision_data());
    nif(blocks, &[0])
}

fn collision_data() -> Block {
    let Block::TriStripsData(mut d) = strips_data(vec![vec![0, 1, 2, 3]], 4, 0) else {
        unreachable!()
    };
    d.vertices = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    Block::TriStripsData(d)
}

/// Вершины форм под `RootCollisionNode` и число таких узлов в файле.
fn collision_vertices(converted: &mut Converted) -> (Vec<Vec<[f32; 3]>>, usize) {
    let back = NiStream::from_bytes(&converted.save_bytes().unwrap()).unwrap();
    let nodes = back.objects_of_type::<RootCollisionNode>().count();
    let mut out = Vec::new();
    if let Some(node) = back.objects_of_type::<RootCollisionNode>().next() {
        for child in &node.base.children {
            let shape: &NiTriShape = back.get_as(*child).unwrap();
            let data: &NiTriShapeData = back.get_as(shape.base.base.geometry_data).unwrap();
            out.push(
                data.base
                    .base
                    .vertices
                    .iter()
                    .map(|v| v.to_array())
                    .collect(),
            );
        }
    }
    (out, nodes)
}

fn assert_close(actual: &[[f32; 3]], expected: &[[f32; 3]]) {
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected) {
        for k in 0..3 {
            assert!((a[k] - e[k]).abs() < 1e-4, "{actual:?} != {expected:?}");
        }
    }
}

#[test]
fn root_collision_holds_the_havok_shape_scaled_to_game_units() {
    let mut converted = convert(&with_collision(false, false, [0.0; 3], IDENTITY_Q)).unwrap();
    assert_eq!(
        (
            converted.stats.collision_shapes,
            converted.stats.collision_triangles
        ),
        (1, 2)
    );
    let (shapes, nodes) = collision_vertices(&mut converted);
    assert_eq!(nodes, 1);
    assert_close(
        &shapes[0],
        &[
            [0.0, 0.0, 0.0],
            [7.0, 0.0, 0.0],
            [0.0, 7.0, 0.0],
            [7.0, 7.0, 0.0],
        ],
    );
}

#[test]
fn the_mopp_wrapper_changes_nothing() {
    let mut plain = convert(&with_collision(false, false, [0.0; 3], IDENTITY_Q)).unwrap();
    let mut wrapped = convert(&with_collision(true, false, [0.0; 3], IDENTITY_Q)).unwrap();
    assert_eq!(
        collision_vertices(&mut plain),
        collision_vertices(&mut wrapped)
    );
}

#[test]
fn a_translated_body_shifts_by_seven_times_its_translation() {
    let mut converted = convert(&with_collision(false, true, [1.0, 2.0, 3.0], IDENTITY_Q)).unwrap();
    let (shapes, _) = collision_vertices(&mut converted);
    assert_close(&shapes[0][..1], &[[7.0, 14.0, 21.0]]);
    assert_close(&shapes[0][3..], &[[14.0, 21.0, 21.0]]);
}

#[test]
fn a_translated_body_rotates_before_it_shifts() {
    // Поворот на 90° вокруг Z: (x, y) → (−y, x); масштаб — до поворота, сдвиг — после.
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let mut converted = convert(&with_collision(
        false,
        true,
        [1.0, 0.0, 0.0],
        [0.0, 0.0, half, half],
    ))
    .unwrap();
    let (shapes, _) = collision_vertices(&mut converted);
    // вершина (1, 0, 0) → ×7 → (7, 0, 0) → поворот → (0, 7, 0) → +(7, 0, 0) = (7, 7, 0)
    assert_close(&shapes[0][1..2], &[[7.0, 7.0, 0.0]]);
}

#[test]
fn an_untranslated_body_ignores_its_translation() {
    let mut converted = convert(&with_collision(false, false, [5.0; 3], IDENTITY_Q)).unwrap();
    let (shapes, _) = collision_vertices(&mut converted);
    assert_close(&shapes[0][..1], &[[0.0, 0.0, 0.0]]);
}

#[test]
fn a_mesh_without_collision_gets_no_collision_node() {
    let mut converted = convert(&simple(vec![vec![0, 1, 2]], 3, 1)).unwrap();
    let (shapes, nodes) = collision_vertices(&mut converted);
    assert_eq!((shapes.len(), nodes), (0, 0));
    assert_eq!(converted.stats.collision_skipped, 0);
}

#[test]
fn collision_on_a_non_root_node_is_counted_not_converted() {
    let mut source = with_collision(false, false, [0.0; 3], IDENTITY_Q);
    // Снимаем коллизию с корня и вешаем на форму.
    if let Block::Node(n) = &mut source.blocks[0] {
        n.av.collision_object = BlockRef::NONE;
    }
    if let Block::TriStrips(s) = &mut source.blocks[1] {
        s.av.collision_object = BlockRef(Some(3));
    }
    let mut converted = convert(&source).unwrap();
    assert_eq!(converted.stats.collision_skipped, 1);
    assert_eq!(collision_vertices(&mut converted).1, 0);
}

#[test]
fn a_collision_chain_with_a_wrong_block_is_an_error() {
    let mut source = with_collision(false, false, [0.0; 3], IDENTITY_Q);
    // Тело ссылается на данные вместо формы.
    source.blocks[4] = rigid_body(2, false, [0.0; 3], IDENTITY_Q);
    assert!(matches!(
        convert(&source).unwrap_err(),
        ConvertError::UnexpectedBlock { index: 2, .. }
    ));
}

#[test]
fn a_collision_index_outside_the_vertices_is_an_error() {
    let mut source = with_collision(false, false, [0.0; 3], IDENTITY_Q);
    if let Block::TriStripsData(d) = &mut source.blocks[6] {
        d.strips = vec![vec![0, 1, 9]];
    }
    assert!(matches!(
        convert(&source).unwrap_err(),
        ConvertError::IndexOutOfRange { index: 9, .. }
    ));
}

// --- Другие формы коллизии (T1.3w) --------------------------------------------------

use crate::read::shapes::{BhkBoxShape, BhkListShape, BhkTransformShape, ConvexBase};

fn box_shape(half: [f32; 3]) -> Block {
    Block::BoxShape(BhkBoxShape {
        base: ConvexBase {
            material: 0,
            radius: 0.05,
        },
        half_extents: half,
    })
}

/// Коллизия у корня из произвольных блоков формы: 0 корень, 1 форма, 2 данные, 3 объект,
/// 4 тело (с формой 5), дальше — `shapes` начиная с индекса 5.
fn with_shapes(shapes: Vec<Block>) -> SourceNif {
    let mut source = with_collision(false, false, [0.0; 3], IDENTITY_Q);
    source.blocks.truncate(5);
    source.blocks.extend(shapes);
    source
}

#[test]
fn a_box_becomes_a_scaled_box_mesh() {
    let mut converted = convert(&with_shapes(vec![box_shape([1.0, 2.0, 3.0])])).unwrap();
    assert_eq!(converted.stats.collision_triangles, 12);
    let (shapes, _) = collision_vertices(&mut converted);
    let xs: Vec<f32> = shapes[0].iter().map(|v| v[0]).collect();
    assert!(xs.iter().all(|x| x.abs() == 7.0), "{xs:?}");
}

#[test]
fn a_list_gives_one_output_shape_per_child_and_a_transform_moves_its_subtree() {
    let mut matrix = [0.0f32; 16];
    for i in 0..4 {
        matrix[i * 5] = 1.0; // единичная
    }
    matrix[12] = 10.0; // сдвиг по x в четвёртом столбце
    let list = Block::ListShape(BhkListShape {
        shapes: vec![BlockRef(Some(6)), BlockRef(Some(7))],
    });
    let moved = Block::TransformShape(BhkTransformShape {
        shape: BlockRef(Some(8)),
        matrix,
    });
    let mut converted = convert(&with_shapes(vec![
        list,
        box_shape([1.0, 1.0, 1.0]),
        moved,
        box_shape([1.0, 1.0, 1.0]),
    ]))
    .unwrap();
    assert_eq!(converted.stats.collision_shapes, 2);
    let (shapes, _) = collision_vertices(&mut converted);
    let min_x = |s: &Vec<[f32; 3]>| s.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
    assert_eq!(min_x(&shapes[0]), -7.0);
    assert_eq!(
        min_x(&shapes[1]),
        63.0,
        "сдвиг 10 havok-юнитов = 70 игровых, минус полуразмер 7"
    );
}

#[test]
fn a_shape_tree_that_loops_is_an_error_not_a_stack_overflow() {
    let list = Block::ListShape(BhkListShape {
        shapes: vec![BlockRef(Some(5))],
    });
    assert!(matches!(
        convert(&with_shapes(vec![list])).unwrap_err(),
        ConvertError::Cycle { .. }
    ));
}

#[test]
fn stencil_and_specular_properties_are_converted() {
    use crate::read::shapes::{NiSpecularProperty, NiStencilProperty};
    let mut source = simple(vec![vec![0, 1, 2]], 3, 1);
    source
        .blocks
        .push(Block::StencilProperty(NiStencilProperty {
            net: net("st"),
            enabled: false,
            function: 0,
            reference: 0,
            mask: 0xFFFF_FFFF,
            fail_action: 0,
            z_fail_action: 0,
            pass_action: 0,
            draw_mode: 3,
        }));
    source
        .blocks
        .push(Block::SpecularProperty(NiSpecularProperty {
            net: net("sp"),
            flags: 0,
        }));
    if let Block::TriStrips(s) = &mut source.blocks[1] {
        s.av.properties = vec![BlockRef(Some(3)), BlockRef(Some(4))];
    }
    let mut converted = convert(&source).unwrap();
    let back = NiStream::from_bytes(&converted.save_bytes().unwrap()).unwrap();
    assert_eq!(
        back.objects_of_type::<tes3::nif::NiStencilProperty>()
            .count(),
        1
    );
    assert_eq!(
        back.objects_of_type::<tes3::nif::NiSpecularProperty>()
            .count(),
        1
    );
}

#[test]
fn a_tri_shape_with_ready_triangles_converts_like_a_strip_shape() {
    let mut source = simple(vec![], 4, 1);
    let Block::TriStrips(s) = source.blocks[1].clone() else {
        unreachable!()
    };
    source.blocks[1] = Block::TriShape(s);
    if let Block::TriStripsData(d) = source.blocks[2].clone() {
        let mut d = d;
        d.triangles = vec![[0, 1, 2], [2, 1, 3]];
        source.blocks[2] = Block::TriShapeData(d);
    }
    let converted = convert(&source).unwrap();
    assert_eq!(converted.stats.triangles, 2);
    assert_eq!(converted.stats.degenerate, 0);

    // индекс за пределами вершин — ошибка и здесь
    if let Block::TriShapeData(d) = &mut source.blocks[2] {
        d.triangles = vec![[0, 1, 9]];
    }
    assert!(matches!(
        convert(&source).unwrap_err(),
        ConvertError::IndexOutOfRange { index: 9, .. }
    ));
}

// --- Частицы и искажение (T2.3b) ----------------------------------------------------

#[test]
fn particle_systems_and_distortion_shapes_are_dropped() {
    let source = nif(
        vec![
            node("Root", &[1, 3, 4]),
            shape_with("Frame", 2, &[5]),
            strips_data(vec![vec![0, 1, 2]], 3, 1),
            Block::Ignored(crate::read::Ignored {
                type_name: "NiParticleSystem",
            }),
            shape_with("Warp", 2, &[6]),
            texturing(7, 0, None),
            texturing(8, 0, None),
            texture_block("Textures\\Gate\\Frame.dds"),
            texture_block("Textures\\Effects\\ObGateMiniWarpy_n.dds"),
        ],
        &[0],
    );
    let mut converted = convert(&source).unwrap();
    assert_eq!(converted.stats.dropped_particles, 1);
    assert_eq!(converted.stats.dropped_distortion, 1);
    assert_eq!(converted.stats.dropped_lights, 0);
    let back = NiStream::from_bytes(&converted.save_bytes().unwrap()).unwrap();
    assert_eq!(
        back.objects_of_type::<NiTriShape>().count(),
        1,
        "осталась рама"
    );
    assert_eq!(back.objects_of_type::<NiSourceTexture>().count(), 1);
}

// --- Частицы → эмиттер Morrowind (T2.7) ---------------------------------------------

/// С системами частиц `convert_with_particles` ставит на место системы узел-эмиттер и узел
/// частиц; без них (`convert_with`) система выбрасывается, как раньше.
#[test]
fn a_particle_system_becomes_a_morrowind_emitter_when_particles_are_given() {
    use crate::particles::Systems;
    use crate::read::psys::{Emission, Emitter, EmitterShape, ParticleSystem};
    use tes3::nif::{NiAutoNormalParticles, NiBSParticleNode, NiParticleSystemController};

    let source = nif(
        vec![
            node("Root", &[1, 3]),
            shape_with("Torch", 2, &[]),
            strips_data(vec![vec![0, 1, 2]], 3, 1),
            Block::Ignored(crate::read::Ignored {
                type_name: "NiParticleSystem",
            }),
            texturing(5, 0, None),
            texture_block("Textures\\Effects\\FireTorch.dds"),
        ],
        &[0],
    );
    let mut system_av = av("Fire");
    // Свойства системы: текстура (переносится) и блок, который частицам не нужен.
    system_av.properties = vec![BlockRef(Some(4))];
    let system = ParticleSystem {
        index: 3,
        av: system_av,
        world_space: true,
        pool: 12,
        mesh_particles: false,
        emitter: Some(Emitter {
            name: "Emitter".to_owned(),
            active: true,
            speed: 20.0,
            speed_variation: 0.0,
            declination: 0.0,
            declination_variation: 0.3,
            planar_angle: 0.0,
            planar_angle_variation: 3.0,
            color: [1.0; 4],
            radius: 8.0,
            life_span: 1.5,
            life_span_variation: 0.0,
            object: BlockRef::NONE,
            shape: EmitterShape::Sphere { radius: 0.0 },
            meshes: Vec::new(),
            velocity_type: 0,
            emission_axis: [0.0; 3],
        }),
        emission: Some(Emission {
            flags: 8,
            frequency: 1.0,
            phase: 0.0,
            start: 0.0,
            stop: 1.0,
            rate: Some(6.0),
            rate_keys: Vec::new(),
            sequence: None,
            once: false,
            triggered: false,
        }),
        modifiers: Vec::new(),
    };
    let particles = Systems::from_systems(vec![system.clone()]);

    let mut converted =
        convert_with_particles(&source, &mut TextureRegistry::new(), &particles).unwrap();
    assert_eq!(
        (
            converted.stats.converted_particles,
            converted.stats.lossy_particles,
            converted.stats.dropped_particles
        ),
        (1, 0, 0)
    );
    let back = NiStream::from_bytes(&converted.save_bytes().unwrap()).unwrap();
    assert_eq!(back.objects_of_type::<NiBSParticleNode>().count(), 1);
    assert_eq!(
        back.objects_of_type::<NiParticleSystemController>().count(),
        1
    );
    let geometry: Vec<&NiAutoNormalParticles> = back.objects_of_type().collect();
    assert_eq!(geometry.len(), 1);
    assert_eq!(
        geometry[0].base.base.base.properties.len(),
        1,
        "текстура системы перенесена"
    );
    assert_eq!(converted.textures.len(), 1, "и попала в список копирования");
    // Узел-эмиттер и узел частиц — дети того же узла, что и система.
    let root = back
        .objects_of_type::<NiNode>()
        .find(|n| n.base.base.name == "Root")
        .unwrap();
    assert_eq!(root.children.len(), 3, "форма, узел-эмиттер, узел частиц");

    // Без частиц — как раньше: система выброшена.
    let plain = convert(&source).unwrap();
    assert_eq!(
        (
            plain.stats.converted_particles,
            plain.stats.dropped_particles
        ),
        (0, 1)
    );

    // Система, которую эмиттер Morrowind не выражает (частицы по событию), выбрасывается.
    let mut triggered = system;
    triggered.emission.as_mut().unwrap().triggered = true;
    let refused = Systems::from_systems(vec![triggered]);
    let converted = convert_with_particles(&source, &mut TextureRegistry::new(), &refused).unwrap();
    assert_eq!(
        (
            converted.stats.converted_particles,
            converted.stats.dropped_particles
        ),
        (0, 1)
    );
}
