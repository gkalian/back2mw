//! Синтетические тесты блоков: байты собраны руками по раскладке из `nif.xml`, так что
//! тест ловит расхождение кода с раскладкой, а не с самим собой. Проверка на реальных
//! файлах — `tests/real_blocks.rs`.

use super::cursor::Cursor;
use super::havok::{read_collision_object, read_rigid_body};
use super::*;

/// Писатель байт.
#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f(&mut self, vs: &[f32]) -> &mut Self {
        for v in vs {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        self
    }
    fn zeros(&mut self, n: usize) -> &mut Self {
        self.0.extend(std::iter::repeat_n(0u8, n));
        self
    }
    fn s(&mut self, text: &str) -> &mut Self {
        self.u32(u32::try_from(text.len()).unwrap());
        self.0.extend_from_slice(text.as_bytes());
        self
    }
    fn net(&mut self, name: &str) -> &mut Self {
        self.s(name).u32(0).i32(-1)
    }
    /// `NiAVObject` с заметными значениями, чтобы перепутанный порядок полей был виден.
    fn av(&mut self, name: &str) -> &mut Self {
        self.net(name)
            .u16(0x000E)
            .f(&[1.0, 2.0, 3.0])
            .f(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
            .f(&[2.0])
            .u32(0)
            .i32(-1)
    }
}

/// Разбирает блок и требует, чтобы курсор встал ровно в конец.
fn read_exact<T>(bytes: &[u8], read: impl FnOnce(&mut Cursor<'_>) -> Result<T>) -> T {
    let mut cur = Cursor::new(bytes);
    let value = read(&mut cur).unwrap();
    assert_eq!(cur.remaining(), 0, "блок разобран не до конца");
    value
}

/// Любое усечение блока — ошибка, а не паника.
fn assert_truncation_is_error<T>(bytes: &[u8], read: impl Fn(&mut Cursor<'_>) -> Result<T>) {
    for cut in 0..bytes.len() {
        let mut cur = Cursor::new(&bytes[..cut]);
        assert!(read(&mut cur).is_err(), "усечение до {cut} байт прошло");
    }
}

#[test]
fn node_reads_transform_children_and_effects() {
    let mut w = W::default();
    w.av("Root").u32(2).i32(1).i32(2).u32(1).i32(7);
    let node = read_exact(&w.0, nodes::read_node);
    assert_eq!(node.av.net.name, "Root");
    assert_eq!(node.av.flags, 0x000E);
    assert_eq!(node.av.translation, [1.0, 2.0, 3.0]);
    assert_eq!(node.av.scale, 2.0);
    assert_eq!(node.av.collision_object, BlockRef::NONE);
    assert_eq!(node.children, [BlockRef(Some(1)), BlockRef(Some(2))]);
    assert_eq!(node.effects, [BlockRef(Some(7))]);
    assert_truncation_is_error(&w.0, nodes::read_node);
}

#[test]
fn av_object_reads_properties_and_collision_object() {
    let mut w = W::default();
    w.net("n").u16(0).f(&[0.0; 3]).f(&[0.0; 9]).f(&[1.0]);
    w.u32(2).i32(4).i32(5).i32(9);
    let av = read_exact(&w.0, common::read_av_object);
    assert_eq!(av.properties, [BlockRef(Some(4)), BlockRef(Some(5))]);
    assert_eq!(av.collision_object, BlockRef(Some(9)));
}

#[test]
fn object_net_reads_extra_data_list_and_controller() {
    let mut w = W::default();
    w.s("named").u32(2).i32(3).i32(4).i32(8);
    let net = read_exact(&w.0, common::read_object_net);
    assert_eq!(net.name, "named");
    assert_eq!(net.extra_data, [BlockRef(Some(3)), BlockRef(Some(4))]);
    assert_eq!(net.controller, BlockRef(Some(8)));
}

#[test]
fn refs_below_minus_one_are_rejected() {
    let mut w = W::default();
    w.i32(-2);
    let mut cur = Cursor::new(&w.0);
    assert_eq!(
        common::read_ref(&mut cur),
        Err(Error::InvalidRef { at: 0, value: -2 })
    );
}

#[test]
fn tri_strips_without_and_with_shader() {
    let mut w = W::default();
    w.av("Shape").i32(3).i32(-1).u8(0);
    let shape = read_exact(&w.0, geometry::read_tri_strips);
    assert_eq!(shape.data, BlockRef(Some(3)));
    assert_eq!(shape.skin_instance, BlockRef::NONE);
    assert_eq!(shape.shader_name, None);

    let mut w = W::default();
    w.av("Shape").i32(3).i32(6).u8(1).s("fancy").i32(-1);
    let shape = read_exact(&w.0, geometry::read_tri_strips);
    assert_eq!(shape.skin_instance, BlockRef(Some(6)));
    assert_eq!(shape.shader_name.as_deref(), Some("fancy"));
    assert_eq!(shape.shader_extra_data, Some(-1));
    assert_truncation_is_error(&w.0, geometry::read_tri_strips);
}

/// Полосы: `[0,1,2,3]` и `[2,3,0]` — 2 + 1 = 3 треугольника.
fn strips_data(with_tangents: bool, with_colors: bool, uv_sets: u16) -> Vec<u8> {
    let mut w = W::default();
    w.i32(0).u16(4).u8(0).u8(0);
    w.u8(1); // Has Vertices
    w.f(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0]);
    let flags = uv_sets | if with_tangents { 4096 } else { 0 };
    w.u16(flags);
    w.u8(1); // Has Normals
    w.f(&[0.0, 0.0, 1.0].repeat(4));
    if with_tangents {
        w.f(&[1.0, 0.0, 0.0].repeat(4));
        w.f(&[0.0, 1.0, 0.0].repeat(4));
    }
    w.f(&[0.5, 0.5, 0.0]).f(&[1.5]); // ограничивающая сфера
    if with_colors {
        w.u8(1).f(&[1.0, 0.5, 0.25, 1.0].repeat(4));
    } else {
        w.u8(0);
    }
    for set in 0..uv_sets {
        let base = f32::from(set);
        w.f(&[base, 0.0, base, 1.0, base + 1.0, 0.0, base + 1.0, 1.0]);
    }
    w.u16(1); // Consistency Flags
    w.i32(-1); // Additional Data
    w.u16(3); // Num Triangles
    w.u16(2); // Num Strips
    w.u16(4).u16(3); // Strip Lengths
    w.u8(1); // Has Points
    w.u16(0).u16(1).u16(2).u16(3);
    w.u16(2).u16(3).u16(0);
    w.0
}

#[test]
fn strips_data_reads_every_optional_array() {
    let bytes = strips_data(true, true, 2);
    let data = read_exact(&bytes, geometry::read_tri_strips_data);
    assert_eq!(data.num_vertices, 4);
    assert_eq!(data.vertices.len(), 4);
    assert_eq!(data.vertices[1], [1.0, 0.0, 0.0]);
    assert_eq!(data.normals.len(), 4);
    assert_eq!(data.tangents.len(), 4);
    assert_eq!(data.bitangents.len(), 4);
    assert_eq!(data.tangents[0], [1.0, 0.0, 0.0]);
    assert_eq!(data.bitangents[0], [0.0, 1.0, 0.0]);
    assert_eq!(data.bound_center, [0.5, 0.5, 0.0]);
    assert_eq!(data.bound_radius, 1.5);
    assert_eq!(data.colors, vec![[1.0, 0.5, 0.25, 1.0]; 4]);
    assert_eq!(data.uv_sets.len(), 2);
    assert_eq!(data.uv_sets[1][2], [2.0, 0.0]);
    assert_eq!(data.num_triangles, 3);
    assert_eq!(data.strips, vec![vec![0, 1, 2, 3], vec![2, 3, 0]]);
    assert_truncation_is_error(&bytes, geometry::read_tri_strips_data);
}

#[test]
fn strips_data_without_optional_arrays_reads_only_what_is_declared() {
    let bytes = strips_data(false, false, 1);
    let data = read_exact(&bytes, geometry::read_tri_strips_data);
    assert!(data.tangents.is_empty());
    assert!(data.colors.is_empty());
    assert_eq!(data.uv_sets.len(), 1);
    assert_eq!(data.strips.len(), 2);
}

#[test]
fn strips_data_with_zero_uv_sets_and_no_points() {
    let mut w = W::default();
    w.i32(0).u16(0).u8(0).u8(0);
    w.u8(0); // Has Vertices
    w.u16(0);
    w.u8(0); // Has Normals
    w.f(&[0.0; 3]).f(&[0.0]);
    w.u8(0); // Has Vertex Colors
    w.u16(0).i32(-1);
    w.u16(0).u16(1).u16(5).u8(0); // 1 полоса длиной 5, Has Points = 0
    let data = read_exact(&w.0, geometry::read_tri_strips_data);
    assert!(data.vertices.is_empty() && data.uv_sets.is_empty());
    assert!(data.strips.is_empty());
}

#[test]
fn a_huge_declared_strip_count_is_an_error_not_an_allocation() {
    let mut bytes = strips_data(false, false, 0);
    // после заголовка блока и массивов число полос — последнее u16 перед длинами;
    // проще поставить заведомо большое число вершин
    bytes[4..6].copy_from_slice(&u16::MAX.to_le_bytes());
    let mut cur = Cursor::new(&bytes);
    assert!(geometry::read_tri_strips_data(&mut cur).is_err());
}

#[test]
fn material_property_reads_all_colours() {
    let mut w = W::default();
    w.net("mat");
    w.f(&[0.1, 0.2, 0.3]).f(&[0.4, 0.5, 0.6]);
    w.f(&[0.7, 0.8, 0.9]).f(&[0.0, 0.0, 0.1]);
    w.f(&[25.0, 0.5]);
    let m = read_exact(&w.0, properties::read_material_property);
    assert_eq!(m.ambient, [0.1, 0.2, 0.3]);
    assert_eq!(m.diffuse, [0.4, 0.5, 0.6]);
    assert_eq!(m.specular, [0.7, 0.8, 0.9]);
    assert_eq!(m.emissive, [0.0, 0.0, 0.1]);
    assert_eq!((m.glossiness, m.alpha), (25.0, 0.5));
    assert_truncation_is_error(&w.0, properties::read_material_property);
}

#[test]
fn vertex_color_property_reads_flags_and_both_modes() {
    let mut w = W::default();
    w.net("vc").u16(0x0028).u32(2).u32(1);
    let p = read_exact(&w.0, properties::read_vertex_color_property);
    assert_eq!((p.flags, p.vertex_mode, p.lighting_mode), (0x28, 2, 1));
}

fn tex_desc(w: &mut W, source: i32, with_transform: bool) {
    w.i32(source).u32(3).u32(2).u32(0);
    if with_transform {
        w.u8(1)
            .f(&[0.1, 0.2])
            .f(&[2.0, 3.0])
            .f(&[0.5])
            .u32(0)
            .f(&[0.5, 0.5]);
    } else {
        w.u8(0);
    }
}

#[test]
fn texturing_property_with_base_only_and_seven_slots() {
    let mut w = W::default();
    w.net("tex").u32(2).u32(7);
    w.u8(1);
    tex_desc(&mut w, 5, false);
    w.u8(0).u8(0).u8(0).u8(0); // dark, detail, gloss, glow
    w.u8(0); // bump
    w.u8(0); // decal 0: texture_count 7 покрывает только его (нужно > 6)
    w.u32(0); // Num Shader Textures
    let p = read_exact(&w.0, properties::read_texturing_property);
    assert_eq!(p.texture_count, 7);
    let base = p.base.as_ref().unwrap();
    assert_eq!(base.source, BlockRef(Some(5)));
    assert_eq!((base.clamp_mode, base.filter_mode), (3, 2));
    assert!(p.dark.is_none() && p.glow.is_none() && p.bump.is_none());
    assert!(p.decals.iter().all(Option::is_none));
    assert_eq!(p.shader_textures.len(), 0);
}

#[test]
fn texturing_property_reads_transform_bump_decal_and_shader_slots() {
    let mut w = W::default();
    w.net("tex").u32(2).u32(9);
    w.u8(1);
    tex_desc(&mut w, 1, true);
    w.u8(0).u8(0).u8(0);
    w.u8(1);
    tex_desc(&mut w, 2, false); // glow
    w.u8(1); // bump
    tex_desc(&mut w, 3, false);
    w.f(&[0.5, 0.25]).f(&[1.0, 0.0, 0.0, 1.0]);
    w.u8(1);
    tex_desc(&mut w, 4, false); // decal 0
    w.u8(0).u8(0); // decal 1, decal 2
    // texture_count 9 покрывает decal 0..2 (нужно > 6, > 7, > 8), decal 3 — нет
    w.u32(1); // Num Shader Textures
    w.u8(1);
    tex_desc(&mut w, 6, false);
    w.u32(42);

    let p = read_exact(&w.0, properties::read_texturing_property);
    let t = p.base.unwrap().transform.unwrap();
    assert_eq!(t.translation, [0.1, 0.2]);
    assert_eq!(t.scale, [2.0, 3.0]);
    assert_eq!(t.rotation, 0.5);
    assert_eq!(t.center, [0.5, 0.5]);
    assert_eq!(p.glow.unwrap().source, BlockRef(Some(2)));
    let bump = p.bump.unwrap();
    assert_eq!(bump.texture.source, BlockRef(Some(3)));
    assert_eq!((bump.luma_scale, bump.luma_offset), (0.5, 0.25));
    assert_eq!(bump.matrix, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(p.decals[0].as_ref().unwrap().source, BlockRef(Some(4)));
    assert!(p.decals[1].is_none() && p.decals[2].is_none() && p.decals[3].is_none());
    assert_eq!(p.shader_textures.len(), 1);
    let (desc, id) = p.shader_textures[0].map.as_ref().unwrap();
    assert_eq!((desc.source, *id), (BlockRef(Some(6)), 42));
    assert_truncation_is_error(&w.0, properties::read_texturing_property);
}

#[test]
fn source_texture_reads_external_file_name() {
    let mut w = W::default();
    w.net("t").u8(1).s("stone.tga").i32(-1);
    w.u32(6).u32(1).u32(0).u8(1).u8(1);
    let t = read_exact(&w.0, properties::read_source_texture);
    assert!(t.use_external);
    assert_eq!(t.file_name, "stone.tga");
    assert_eq!(t.pixel_data, BlockRef::NONE);
    assert_eq!(t.format_prefs, [6, 1, 0]);
    assert!(t.direct_render);
    assert_truncation_is_error(&w.0, properties::read_source_texture);
}

#[test]
fn extra_data_blocks() {
    let mut w = W::default();
    w.s("Prn").s("Bip01");
    let s = read_exact(&w.0, extra::read_string_extra);
    assert_eq!((s.name.as_str(), s.string.as_str()), ("Prn", "Bip01"));

    let mut w = W::default();
    w.s("Tangent space (binormal & tangent vectors)")
        .u32(3)
        .u8(1)
        .u8(2)
        .u8(3);
    let b = read_exact(&w.0, extra::read_binary_extra);
    assert_eq!(b.data, [1, 2, 3]);
    assert_truncation_is_error(&w.0, extra::read_binary_extra);

    let mut w = W::default();
    w.s("BSX").u32(0x83);
    let f = read_exact(&w.0, extra::read_bsx_flags);
    assert_eq!((f.name.as_str(), f.flags), ("BSX", 0x83));
}

#[test]
fn a_binary_extra_data_claiming_more_bytes_than_exist_is_an_error() {
    let mut w = W::default();
    w.s("x").u32(u32::MAX);
    let mut cur = Cursor::new(&w.0);
    assert!(extra::read_binary_extra(&mut cur).is_err());
}

#[test]
fn collision_object_reads_target_flags_and_body() {
    let mut w = W::default();
    w.i32(0).u16(0x81).i32(9);
    let c = read_exact(&w.0, read_collision_object);
    assert_eq!(c.target, BlockRef(Some(0)));
    assert_eq!(c.flags, 0x81);
    assert_eq!(c.body, BlockRef(Some(9)));
}

fn rigid_body_bytes() -> Vec<u8> {
    let mut w = W::default();
    w.i32(8); // shape
    w.u8(1).u8(0).u16(0); // HavokFilter: слой, флаги, группа
    w.zeros(4).u8(1).zeros(3).zeros(12); // информация о мире
    w.u8(1).u8(0).u16(0xFFFF); // bhkEntityCInfo
    w.zeros(4); // unused 01
    w.u8(1).u8(0).u16(0); // фильтр в CInfo
    w.zeros(4); // unused 02
    w.u8(1).u8(0).u16(0xFFFF); // отклик, unused 03, задержка
    w.zeros(4); // unused 04
    w.f(&[10.0, 20.0, 30.0, 0.0]); // сдвиг
    w.f(&[0.0, 0.0, 0.707, 0.707]); // поворот
    w.zeros(32).zeros(48).zeros(16); // скорости, инерция, центр
    w.f(&[0.0, 0.1, 0.05, 0.5, 0.4, 104.4, 31.57, 0.15]);
    w.u8(7).u8(0).u8(0).u8(1); // motion, деактиватор, решатель, качество
    w.zeros(12);
    w.u32(1).i32(11); // ограничения
    w.u32(1); // Body Flags
    w.0
}

#[test]
fn rigid_body_reads_through_the_whole_cinfo() {
    let bytes = rigid_body_bytes();
    let body = read_exact(&bytes, |c| read_rigid_body(c, true));
    assert!(body.translated);
    assert_eq!(body.shape, BlockRef(Some(8)));
    assert_eq!(body.filter.layer, 1);
    assert_eq!(body.translation, [10.0, 20.0, 30.0, 0.0]);
    assert_eq!(body.rotation, [0.0, 0.0, 0.707, 0.707]);
    assert_eq!(body.mass, 0.0);
    assert_eq!((body.friction, body.restitution), (0.5, 0.4));
    assert_eq!((body.motion_system, body.quality_type), (7, 1));
    assert_eq!(body.constraints, [BlockRef(Some(11))]);
    assert_eq!(body.body_flags, 1);
    assert_truncation_is_error(&bytes, |c| read_rigid_body(c, false));
}

#[test]
fn mopp_shape_skips_the_code_but_keeps_its_size() {
    let mut w = W::default();
    w.i32(4)
        .zeros(12)
        .f(&[1.0])
        .u32(5)
        .f(&[-1.0, -2.0, -3.0, 4.0]);
    w.0.extend_from_slice(&[9; 5]);
    let m = read_exact(&w.0, havok::read_mopp_shape);
    assert_eq!(m.shape, BlockRef(Some(4)));
    assert_eq!(m.scale, 1.0);
    assert_eq!(m.mopp_size, 5);
    assert_eq!(m.mopp_offset, [-1.0, -2.0, -3.0, 4.0]);
    assert_truncation_is_error(&w.0, havok::read_mopp_shape);
}

#[test]
fn tri_strips_shape_reads_refs_and_filters() {
    let mut w = W::default();
    w.u32(1).f(&[0.1]).zeros(20).u32(1).f(&[1.0, 1.0, 1.0, 0.0]);
    w.u32(2).i32(5).i32(6);
    w.u32(2).u8(1).u8(0).u16(0).u8(2).u8(0).u16(1);
    let s = read_exact(&w.0, havok::read_tri_strips_shape);
    assert_eq!(s.material, 1);
    assert_eq!(s.radius, 0.1);
    assert_eq!(s.grow_by, 1);
    assert_eq!(s.scale, [1.0, 1.0, 1.0, 0.0]);
    assert_eq!(s.strips_data, [BlockRef(Some(5)), BlockRef(Some(6))]);
    assert_eq!(s.filters.len(), 2);
    assert_eq!(
        s.filters[1],
        HavokFilter {
            layer: 2,
            flags: 0,
            group: 1
        }
    );
    assert_truncation_is_error(&w.0, havok::read_tri_strips_shape);
}

// ---------------------------------------------------------------------------
// Файл целиком
// ---------------------------------------------------------------------------

/// Собирает NIF 20.0.0.4: заголовок, тела блоков, футер с корнем `0`.
fn file(types: &[&str], index: &[u16], bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut w = W::default();
    w.0.extend_from_slice(b"Gamebryo File Format, Version 20.0.0.4\n");
    w.u32(header::VERSION_20_0_0_4).u8(1).u32(11);
    w.u32(u32::try_from(index.len()).unwrap()).u32(11);
    for s in ["a", "b", "c"] {
        w.u8(1);
        w.0.extend_from_slice(s.as_bytes());
    }
    w.u16(u16::try_from(types.len()).unwrap());
    for t in types {
        w.s(t);
    }
    for i in index {
        w.u16(*i);
    }
    w.u32(0);
    for body in bodies {
        w.0.extend_from_slice(body);
    }
    w.u32(1).i32(0);
    w.0
}

fn node_body() -> Vec<u8> {
    let mut w = W::default();
    w.av("Root").u32(0).u32(0);
    w.0
}

fn bsx_body() -> Vec<u8> {
    let mut w = W::default();
    w.s("BSX").u32(2);
    w.0
}

#[test]
fn parse_reads_a_whole_file_and_its_roots() {
    let bytes = file(&["NiNode", "BSXFlags"], &[0, 1], &[node_body(), bsx_body()]);
    let nif = parse(&bytes).unwrap();
    assert_eq!(nif.blocks.len(), 2);
    assert!(matches!(nif.blocks[0], Block::Node(_)));
    assert!(matches!(nif.blocks[1], Block::BsxFlags(_)));
    assert_eq!(nif.roots, [BlockRef(Some(0))]);
    assert!(matches!(
        nif.block(BlockRef(Some(1))),
        Some(Block::BsxFlags(_))
    ));
    assert!(nif.block(BlockRef::NONE).is_none());
    assert!(nif.block(BlockRef(Some(2))).is_none());
}

#[test]
fn parse_rejects_bytes_after_the_footer() {
    let mut bytes = file(&["NiNode"], &[0], &[node_body()]);
    bytes.push(0);
    match parse(&bytes) {
        Err(Error::TrailingData {
            last_block,
            last_type,
            pos,
            len,
        }) => {
            assert_eq!((last_block, last_type.as_str()), (0, "NiNode"));
            assert_eq!(pos + 1, len);
        }
        other => panic!("ожидалась TrailingData, получено {other:?}"),
    }
}

#[test]
fn parse_reports_an_unsupported_block_with_its_index_and_type() {
    let bytes = file(
        &["NiNode", "NiPhysXProp"],
        &[0, 1],
        &[node_body(), vec![0; 8]],
    );
    assert_eq!(
        parse(&bytes),
        Err(Error::UnsupportedBlock {
            index: 1,
            type_name: "NiPhysXProp".to_owned()
        })
    );
}

#[test]
fn parse_names_the_block_where_a_read_failed() {
    // NiTriStrips с булевым `Has Shader` = 2: раскладка съехала внутри блока 1.
    let mut broken = W::default();
    broken.av("Shape").i32(0).i32(-1).u8(2);
    let bytes = file(
        &["NiNode", "NiTriStrips"],
        &[0, 1],
        &[node_body(), broken.0],
    );
    match parse(&bytes) {
        Err(Error::InBlock {
            index,
            type_name,
            source,
        }) => {
            assert_eq!((index, type_name.as_str()), (1, "NiTriStrips"));
            assert!(matches!(*source, Error::InvalidBool { value: 2, .. }));
        }
        other => panic!("ожидалась InBlock, получено {other:?}"),
    }
}

#[test]
fn every_truncation_of_a_whole_file_is_an_error_not_a_panic() {
    let bytes = file(&["NiNode", "BSXFlags"], &[0, 1], &[node_body(), bsx_body()]);
    for cut in 0..bytes.len() {
        assert!(
            parse(&bytes[..cut]).is_err(),
            "усечение до {cut} байт прошло"
        );
    }
}

#[test]
fn alpha_property_reads_flags_and_threshold() {
    let mut w = W::default();
    w.net("alpha").u16(0x12ED).u8(128);
    let p = read_exact(&w.0, properties::read_alpha_property);
    assert_eq!((p.flags, p.threshold), (0x12ED, 128));
    assert_truncation_is_error(&w.0, properties::read_alpha_property);
}

// --- T1.3w: остальные свойства и формы -----------------------------------------------

#[test]
fn stencil_property_reads_every_field_and_no_flags() {
    let mut w = W::default();
    w.net("st")
        .u8(1)
        .u32(4)
        .u32(7)
        .u32(0xFFFF_FFFF)
        .u32(0)
        .u32(1)
        .u32(3)
        .u32(3);
    let p = read_exact(&w.0, shapes::read_stencil_property);
    assert!(p.enabled);
    assert_eq!((p.function, p.reference, p.mask), (4, 7, 0xFFFF_FFFF));
    assert_eq!((p.fail_action, p.z_fail_action, p.pass_action), (0, 1, 3));
    assert_eq!(p.draw_mode, 3);
    assert_truncation_is_error(&w.0, shapes::read_stencil_property);
}

#[test]
fn specular_property_reads_flags() {
    let mut w = W::default();
    w.net("sp").u16(1);
    assert_eq!(read_exact(&w.0, shapes::read_specular_property).flags, 1);
}

#[test]
fn furniture_marker_skips_sixteen_bytes_per_position() {
    let mut w = W::default();
    w.s("FRN").u32(2);
    w.f(&[1.0, 2.0, 3.0]).u16(0).u8(1).u8(1);
    w.f(&[4.0, 5.0, 6.0]).u16(0).u8(2).u8(2);
    let m = read_exact(&w.0, shapes::read_furniture_marker);
    assert_eq!((m.name.as_str(), m.positions), ("FRN", 2));
    assert_truncation_is_error(&w.0, shapes::read_furniture_marker);
}

#[test]
fn box_shape_reads_half_extents_after_padding() {
    let mut w = W::default();
    w.u32(7).f(&[0.1]).zeros(8).f(&[1.0, 2.0, 3.0]).f(&[0.0]);
    let b = read_exact(&w.0, shapes::read_box_shape);
    assert_eq!(b.half_extents, [1.0, 2.0, 3.0]);
    assert_eq!((b.base.material, b.base.radius), (7, 0.1));
    assert_truncation_is_error(&w.0, shapes::read_box_shape);
}

#[test]
fn capsule_and_sphere_shapes() {
    let mut w = W::default();
    w.u32(0).f(&[0.05]).zeros(8);
    w.f(&[0.0, 0.0, -1.0])
        .f(&[0.5])
        .f(&[0.0, 0.0, 1.0])
        .f(&[0.5]);
    let c = read_exact(&w.0, shapes::read_capsule_shape);
    assert_eq!((c.first, c.second), ([0.0, 0.0, -1.0], [0.0, 0.0, 1.0]));
    assert_eq!((c.radius1, c.radius2), (0.5, 0.5));

    let mut w = W::default();
    w.u32(3).f(&[2.5]);
    assert_eq!(read_exact(&w.0, shapes::read_sphere_shape).base.radius, 2.5);
}

#[test]
fn convex_vertices_shape_reads_vertices_and_planes() {
    let mut w = W::default();
    w.u32(0).f(&[0.05]).zeros(24);
    w.u32(2).f(&[1.0, 2.0, 3.0, 0.0]).f(&[4.0, 5.0, 6.0, 0.0]);
    w.u32(1).f(&[0.0, 0.0, 1.0, -3.0]);
    let s = read_exact(&w.0, shapes::read_convex_vertices_shape);
    assert_eq!(s.vertices, [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
    assert_eq!(s.planes, [[0.0, 0.0, 1.0, -3.0]]);
    assert_truncation_is_error(&w.0, shapes::read_convex_vertices_shape);
}

#[test]
fn packed_strips_shape_and_data() {
    let mut w = W::default();
    w.u16(1).u32(0).u32(3).u32(0); // один подчастный: фильтр, вершины, материал
    w.u32(0).zeros(4).f(&[0.1]).zeros(4);
    w.f(&[1.0, 1.0, 1.0, 0.0])
        .f(&[0.1])
        .f(&[1.0, 1.0, 1.0, 0.0]);
    w.i32(9);
    let s = read_exact(&w.0, shapes::read_packed_strips_shape);
    assert_eq!(s.data, BlockRef(Some(9)));
    assert_truncation_is_error(&w.0, shapes::read_packed_strips_shape);

    let mut w = W::default();
    w.u32(1).u16(0).u16(1).u16(2).u16(0).f(&[0.0, 0.0, 1.0]);
    w.u32(3)
        .f(&[0.0, 0.0, 0.0])
        .f(&[1.0, 0.0, 0.0])
        .f(&[0.0, 1.0, 0.0]);
    let d = read_exact(&w.0, shapes::read_packed_strips_data);
    assert_eq!(d.triangles, [[0, 1, 2]]);
    assert_eq!(d.vertices.len(), 3);
    assert_truncation_is_error(&w.0, shapes::read_packed_strips_data);
}

#[test]
fn list_shape_reads_children_and_skips_properties_and_filters() {
    let mut w = W::default();
    w.u32(2).i32(4).i32(5).u32(0).zeros(24).u32(2).u32(0).u32(0);
    let l = read_exact(&w.0, shapes::read_list_shape);
    assert_eq!(l.shapes, [BlockRef(Some(4)), BlockRef(Some(5))]);
    assert_truncation_is_error(&w.0, shapes::read_list_shape);
}

#[test]
fn transform_shape_reads_the_child_and_sixteen_floats() {
    let mut w = W::default();
    w.i32(3).u32(0).f(&[0.1]).zeros(8);
    let m: Vec<f32> = (0..16).map(|i| i as f32).collect();
    w.f(&m);
    let t = read_exact(&w.0, shapes::read_transform_shape);
    assert_eq!(t.shape, BlockRef(Some(3)));
    assert_eq!(t.matrix[15], 15.0);
    assert_truncation_is_error(&w.0, shapes::read_transform_shape);
}

#[test]
fn supported_types_match_read_block() {
    // Тип из списка не должен давать UnsupportedBlock (на пустых данных — любая другая ошибка).
    for name in SUPPORTED_TYPES {
        let mut cur = Cursor::new(&[]);
        let err = read_block(&mut cur, 0, name).unwrap_err();
        assert!(!matches!(err, Error::UnsupportedBlock { .. }), "{name}");
    }
    let mut cur = Cursor::new(&[]);
    assert!(matches!(
        read_block(&mut cur, 0, "NiNoSuchBlock").unwrap_err(),
        Error::UnsupportedBlock { .. }
    ));
}

#[test]
fn tri_shape_data_reads_triangles_and_skips_match_groups() {
    let mut w = W::default();
    w.i32(0).u16(3).u8(0).u8(0);
    w.u8(1).f(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    w.u16(0); // data_flags: без UV и тангенты
    w.u8(0); // нормалей нет
    w.f(&[0.0, 0.0, 0.0, 1.0]); // сфера
    w.u8(0); // цветов нет
    w.u16(0).i32(-1); // consistency, additional data
    w.u16(1).u32(3).u8(1); // 1 треугольник, 3 индекса, треугольники есть
    w.u16(0).u16(1).u16(2);
    w.u16(1).u16(2).u16(0).u16(1); // одна группа совпадений из двух вершин
    let d = read_exact(&w.0, geometry::read_tri_shape_data);
    assert_eq!(d.triangles, [[0, 1, 2]]);
    assert!(d.strips.is_empty());
    assert_eq!(d.vertices.len(), 3);
    assert_truncation_is_error(&w.0, geometry::read_tri_shape_data);
}

/// Блок из `anim::IGNORED_TYPES`, прочитанный через `read_block`, ровно до конца.
fn read_ignored_exact(bytes: &[u8], type_name: &str) {
    let block = read_exact(bytes, |cur| read_block(cur, 0, type_name));
    assert!(
        matches!(block, Block::Ignored(i) if i.type_name == type_name),
        "{type_name}: {block:?}"
    );
    assert_truncation_is_error(bytes, |cur| read_block(cur, 0, type_name));
}

/// `NiTimeController`: следующий, флаги, частота, фаза, начало, конец, цель.
fn time_controller(w: &mut W) -> &mut W {
    w.i32(-1).u16(0x48).f(&[1.0, 0.0, 0.0, 2.0]).i32(0)
}

#[test]
fn controller_manager_and_sequence_are_read_to_the_end() {
    let mut w = W::default();
    time_controller(&mut w).u8(1).u32(2).i32(3).i32(4).i32(5);
    read_ignored_exact(&w.0, "NiControllerManager");

    let mut w = W::default();
    w.s("Open").u32(2).u32(1);
    for i in 0..2 {
        // интерполятор, контроллер, приоритет, палитра, пять смещений
        w.i32(i)
            .i32(9)
            .u8(26)
            .i32(7)
            .u32(0)
            .u32(5)
            .u32(10)
            .u32(15)
            .u32(20);
    }
    w.f(&[1.0])
        .i32(8)
        .u32(0)
        .f(&[1.0, 0.0, 2.5])
        .i32(1)
        .s("Door01")
        .i32(7);
    read_ignored_exact(&w.0, "NiControllerSequence");
}

#[test]
fn palettes_and_text_keys_are_read_to_the_end() {
    let mut w = W::default();
    w.i32(0).u32(2).s("Door").i32(1).s("Frame").i32(2);
    read_ignored_exact(&w.0, "NiDefaultAVObjectPalette");

    let mut w = W::default();
    w.s("Door\0NiTransformController\0").u32(26);
    read_ignored_exact(&w.0, "NiStringPalette");

    let mut w = W::default();
    w.s("").u32(2).f(&[0.0]).s("start").f(&[2.5]).s("end");
    read_ignored_exact(&w.0, "NiTextKeyExtraData");
}

#[test]
fn controllers_read_their_own_tails() {
    let mut w = W::default();
    time_controller(&mut w).u16(2).i32(1).i32(2);
    read_ignored_exact(&w.0, "NiMultiTargetTransformController");

    for name in [
        "NiTransformController",
        "NiAlphaController",
        "NiVisController",
    ] {
        let mut w = W::default();
        time_controller(&mut w).i32(3);
        read_ignored_exact(&w.0, name);
    }

    let mut w = W::default();
    time_controller(&mut w).i32(3).u16(1);
    read_ignored_exact(&w.0, "NiMaterialColorController");

    let mut w = W::default();
    time_controller(&mut w).i32(3).u8(0).u32(0).u32(1);
    read_ignored_exact(&w.0, "NiTextureTransformController");

    let mut w = W::default();
    time_controller(&mut w).i32(3).u32(0).u32(2).i32(4).i32(5);
    read_ignored_exact(&w.0, "NiFlipController");

    let mut w = W::default();
    time_controller(&mut w)
        .u16(0)
        .i32(4)
        .u8(1)
        .u32(1)
        .i32(5)
        .u32(1)
        .u32(0);
    read_ignored_exact(&w.0, "NiGeomMorpherController");

    let mut w = W::default();
    w.u32(2).u32(3).u8(1);
    for name in ["Base", "Smile"] {
        w.s(name).zeros(3 * 12);
    }
    read_ignored_exact(&w.0, "NiMorphData");
}

#[test]
fn interpolators_read_their_values() {
    let mut w = W::default();
    w.f(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0]).i32(2);
    read_ignored_exact(&w.0, "NiTransformInterpolator");

    let mut w = W::default();
    w.f(&[0.5]).i32(2);
    read_ignored_exact(&w.0, "NiFloatInterpolator");

    // Значение 2 — «не задано»: это байт, не bool, и отказа быть не должно.
    let mut w = W::default();
    w.u8(2).i32(2);
    read_ignored_exact(&w.0, "NiBoolInterpolator");

    let mut w = W::default();
    w.f(&[1.0, 2.0, 3.0]).i32(-1);
    read_ignored_exact(&w.0, "NiPoint3Interpolator");

    let mut w = W::default();
    w.u16(3).i32(1).f(&[0.0, 0.0]).u16(0).i32(1).i32(2);
    read_ignored_exact(&w.0, "NiPathInterpolator");
}

#[test]
fn blend_interpolators_read_items_only_without_the_manager_flag() {
    // Флаг 0: полный набор полей и два элемента по 17 байт.
    let mut w = W::default();
    w.u8(0).u8(2).f(&[0.0]).u8(1).u8(0).u8(0).u8(0).f(&[0.0; 4]);
    for i in 0..2 {
        w.i32(i).f(&[1.0, 0.5]).u8(0).f(&[0.0]);
    }
    w.f(&[0.25]);
    read_ignored_exact(&w.0, "NiBlendFloatInterpolator");

    // Флаг 1 (управляется менеджером): после порога веса — сразу значение.
    let mut w = W::default();
    w.u8(1).u8(4).f(&[0.0]).f(&[1.0, 2.0, 3.0]);
    read_ignored_exact(&w.0, "NiBlendPoint3Interpolator");

    let mut w = W::default();
    w.u8(1).u8(0).f(&[0.0]);
    read_ignored_exact(&w.0, "NiBlendTransformInterpolator");
}

#[test]
fn key_data_sizes_follow_the_interpolation_type() {
    // Кватернионы TBC (4 + 16 + 12), сдвиги квадратичные (4 + 3 × 12), масштабов нет.
    let mut w = W::default();
    w.u32(2).u32(3).zeros(2 * 32);
    w.u32(1).u32(2).zeros(4 + 36);
    w.u32(0);
    read_ignored_exact(&w.0, "NiTransformData");

    // XYZ-вращение: три группы ключей, у каждой свой тип.
    let mut w = W::default();
    w.u32(1).u32(4);
    w.u32(1).u32(1).zeros(8);
    w.u32(0);
    w.u32(2).u32(3).zeros(2 * (8 + 12));
    w.u32(0).u32(0);
    read_ignored_exact(&w.0, "NiKeyframeData");

    let mut w = W::default();
    w.u32(2).u32(1).zeros(2 * 8);
    read_ignored_exact(&w.0, "NiFloatData");

    let mut w = W::default();
    w.u32(3).u32(5).zeros(3 * 5);
    read_ignored_exact(&w.0, "NiBoolData");

    let mut w = W::default();
    w.u32(1).u32(1).zeros(16);
    read_ignored_exact(&w.0, "NiPosData");

    let mut w = W::default();
    w.u32(1).u32(1).zeros(20);
    read_ignored_exact(&w.0, "NiColorData");
}

#[test]
fn havok_constraints_and_phantoms_have_fixed_sizes() {
    for (name, own) in [
        ("bhkLimitedHingeConstraint", 124),
        ("bhkRagdollConstraint", 120),
        ("bhkPrismaticConstraint", 140),
        ("bhkHingeConstraint", 80),
        ("bhkStiffSpringConstraint", 36),
    ] {
        let mut w = W::default();
        w.u32(2).i32(1).i32(2).u32(1).zeros(own);
        read_ignored_exact(&w.0, name);
    }

    let mut w = W::default();
    w.i32(3).u32(0).zeros(20).zeros(8).f(&[0.0; 16]);
    read_ignored_exact(&w.0, "bhkSimpleShapePhantom");

    let mut w = W::default();
    w.i32(0).u16(1).i32(2);
    read_ignored_exact(&w.0, "bhkSPCollisionObject");
}

#[test]
fn unsupported_properties_are_read_and_dropped() {
    let mut w = W::default();
    w.net("").u16(3).u32(3);
    read_ignored_exact(&w.0, "NiZBufferProperty");

    let mut w = W::default();
    w.net("").u16(0).f(&[1.0, 0.5, 0.5, 0.5]);
    read_ignored_exact(&w.0, "NiFogProperty");

    let mut w = W::default();
    w.net("").u16(1);
    read_ignored_exact(&w.0, "NiDitherProperty");
}

#[test]
fn billboard_node_is_a_node_with_a_mode() {
    let mut w = W::default();
    w.av("Flame").u32(1).i32(1).u32(0).u16(2);
    let block = read_exact(&w.0, |cur| read_block(cur, 0, "NiBillboardNode"));
    let Block::Node(node) = block else {
        panic!("узел: {block:?}");
    };
    assert_eq!(node.av.net.name, "Flame");
    assert_eq!(node.children, [BlockRef(Some(1))]);
}

#[test]
fn every_ignored_type_is_listed_as_supported() {
    for name in IGNORED_TYPES {
        assert!(SUPPORTED_TYPES.contains(name), "{name}");
    }
}

#[test]
fn lights_in_the_mesh_are_read_and_dropped() {
    let light = |w: &mut W| {
        w.av("Light").u8(1).u32(1).i32(0).f(&[1.0]).f(&[0.5; 9]);
    };
    for name in ["NiDirectionalLight", "NiAmbientLight"] {
        let mut w = W::default();
        light(&mut w);
        read_ignored_exact(&w.0, name);
    }
    let mut w = W::default();
    light(&mut w);
    w.f(&[0.0, 1.0, 0.0]);
    read_ignored_exact(&w.0, "NiPointLight");
    let mut w = W::default();
    light(&mut w);
    w.f(&[0.0, 1.0, 0.0]).f(&[45.0, 1.0]);
    read_ignored_exact(&w.0, "NiSpotLight");
}

#[test]
fn version_10_2_reads_ps2_fields_in_texture_descriptions() {
    // TexDesc: флаг наличия, источник, clamp, filter, набор UV, у 10.2.0.0 — PS2 L и PS2 K,
    // флаг преобразования.
    let mut w = W::default();
    w.u8(1).i32(1).u32(0).u32(2).u32(0).u16(0).u16(0xFFB5).u8(0);
    let mut cur = Cursor::new(&w.0);
    cur.set_version(header::VERSION_10_2_0_0, 9);
    let desc = properties::read_opt_tex_desc(&mut cur).unwrap();
    assert_eq!(cur.remaining(), 0, "PS2-поля прочитаны");
    assert_eq!(desc.map(|d| d.source), Some(BlockRef(Some(1))));

    // Контрольный опыт: те же байты как 20.0.0.4 — поля PS2 лишние, флаг преобразования
    // читается из них и курсор до конца не доходит.
    let mut cur = Cursor::new(&w.0);
    let _ = properties::read_opt_tex_desc(&mut cur);
    assert_ne!(cur.remaining(), 0);
}

// --- Частицы (T2.3b) ----------------------------------------------------------------

/// Как [`read_ignored_exact`], но у файла версия `version` и `BSVER` `bsver`.
fn read_ignored_exact_at(bytes: &[u8], type_name: &str, version: u32, bsver: u32) {
    let read = |cur: &mut Cursor<'_>| {
        cur.set_version(version, bsver);
        read_block(cur, 0, type_name)
    };
    let block = read_exact(bytes, read);
    assert!(
        matches!(block, Block::Ignored(i) if i.type_name == type_name),
        "{type_name}: {block:?}"
    );
    assert_truncation_is_error(bytes, read);
}

#[test]
fn every_particle_type_is_listed_as_supported() {
    for name in PARTICLE_TYPES {
        assert!(SUPPORTED_TYPES.contains(name), "{name}");
        assert!(!IGNORED_TYPES.contains(name), "{name} в двух списках");
    }
}

#[test]
fn particle_system_is_geometry_with_modifier_list() {
    let mut w = W::default();
    w.av("Fire").i32(5).i32(-1).u8(0); // данные, скиннинг, шейдера нет
    w.u8(1).u32(2).i32(6).i32(7); // world space, два модификатора
    read_ignored_exact(&w.0, "NiParticleSystem");
    read_ignored_exact(&w.0, "NiMeshParticleSystem");
    assert!(is_particle_system("NiParticleSystem"));
    assert!(!is_particle_system("NiPSysData"));
}

/// `NiGeometryData` на две вершины, без нормалей, цветов и UV.
fn particle_geometry(w: &mut W) -> &mut W {
    w.i32(0).u16(2).u8(0).u8(0);
    w.u8(1).f(&[0.0; 6]); // вершины
    w.u16(0).u8(0); // data flags, нормалей нет
    w.f(&[0.0, 0.0, 0.0, 1.0]).u8(0); // сфера, цветов нет
    w.u16(0) // consistency
}

#[test]
fn particle_data_20_0_0_4_has_rotation_angles_and_short_particle_info() {
    let mut w = W::default();
    particle_geometry(&mut w).i32(-1); // additional data
    w.u8(1).f(&[1.0, 2.0]); // радиусы
    w.u16(2); // активных
    w.u8(0); // размеров нет
    w.u8(1).f(&[0.0; 8]); // вращения, два кватерниона
    w.u8(1).f(&[0.5, 0.5]); // углы
    w.u8(0); // осей нет
    w.zeros(2 * 28); // NiParticleInfo без оси вращения
    w.u8(1).f(&[3.0, 4.0]); // скорости вращения
    w.u16(0).u16(0);
    read_ignored_exact(&w.0, "NiPSysData");

    // NiMeshPSysData: пул, заполнение, поколения, меши.
    w.u32(8).u8(1).u32(2).u32(0).u32(1).i32(9);
    read_ignored_exact(&w.0, "NiMeshPSysData");
}

#[test]
fn particle_data_10_2_has_rotation_axis_in_particle_info_and_no_angles() {
    let mut w = W::default();
    particle_geometry(&mut w); // без additional data
    w.u8(0).u16(2).u8(0).u8(0); // радиусов, размеров, вращений нет
    w.zeros(2 * 40); // NiParticleInfo с осью вращения
    w.u16(0).u16(0); // скоростей вращения нет вовсе (с 20.0.0.2)
    read_ignored_exact_at(&w.0, "NiPSysData", header::VERSION_10_2_0_0, 9);
}

/// `NiPSysModifier`: имя, порядок, система, включён.
fn modifier(w: &mut W) -> &mut W {
    w.s("Mod").u32(3).i32(1).u8(1)
}

#[test]
fn particle_modifiers_are_read_to_the_end() {
    let mut w = W::default();
    modifier(&mut w).u8(0).i32(-1);
    read_ignored_exact(&w.0, "NiPSysAgeDeathModifier");

    let mut w = W::default();
    modifier(&mut w).u16(1);
    read_ignored_exact(&w.0, "NiPSysBoundUpdateModifier");

    let mut w = W::default();
    modifier(&mut w);
    read_ignored_exact(&w.0, "NiPSysPositionModifier");

    let mut w = W::default();
    modifier(&mut w).u16(0).f(&[1.0]).u16(1).u16(1).f(&[0.0; 4]);
    read_ignored_exact(&w.0, "NiPSysSpawnModifier");

    for name in ["NiPSysColorModifier", "NiPSysColliderManager"] {
        let mut w = W::default();
        modifier(&mut w).i32(4);
        read_ignored_exact(&w.0, name);
    }

    let mut w = W::default();
    modifier(&mut w)
        .i32(2)
        .f(&[0.0, 0.0, -1.0, 0.0, 9.8])
        .u32(0)
        .f(&[0.0, 1.0]);
    read_ignored_exact(&w.0, "NiPSysGravityModifier");

    let mut w = W::default();
    modifier(&mut w).f(&[0.5]).u16(0).f(&[1.0]).u16(0);
    read_ignored_exact(&w.0, "NiPSysGrowFadeModifier");

    let mut w = W::default();
    modifier(&mut w)
        .i32(2)
        .f(&[1.0, 0.0, 0.0, 0.05, 100.0, 200.0]);
    read_ignored_exact(&w.0, "NiPSysDragModifier");

    let mut w = W::default();
    modifier(&mut w).u32(2).i32(5).i32(6);
    read_ignored_exact(&w.0, "NiPSysMeshUpdateModifier");

    for name in ["BSWindModifier", "BSParentVelocityModifier"] {
        let mut w = W::default();
        modifier(&mut w).f(&[0.25]);
        read_ignored_exact(&w.0, name);
    }
}

#[test]
fn rotation_modifier_fields_since_20_0_0_2_are_absent_in_10_2() {
    let mut w = W::default();
    modifier(&mut w)
        .f(&[1.0, 0.1, 0.0, 0.2])
        .u8(1)
        .u8(1)
        .f(&[1.0, 0.0, 0.0]);
    read_ignored_exact(&w.0, "NiPSysRotationModifier");

    let mut w = W::default();
    modifier(&mut w).f(&[1.0]).u8(1).f(&[1.0, 0.0, 0.0]);
    read_ignored_exact_at(&w.0, "NiPSysRotationModifier", header::VERSION_10_2_0_0, 9);
}

/// `NiPSysEmitter`: скорость, углы, цвет, радиус, (разброс радиуса), жизнь.
fn emitter(w: &mut W, radius_variation: bool) -> &mut W {
    modifier(w)
        .f(&[10.0, 1.0, 0.0, 0.5, 0.0, 3.1])
        .f(&[1.0; 4])
        .f(&[2.0]);
    if radius_variation {
        w.f(&[0.5]);
    }
    w.f(&[1.5, 0.25])
}

#[test]
fn emitters_are_read_to_the_end_in_both_versions() {
    let mut w = W::default();
    emitter(&mut w, true).i32(0).f(&[1.0, 2.0, 3.0]);
    read_ignored_exact(&w.0, "NiPSysBoxEmitter");

    let mut w = W::default();
    emitter(&mut w, false).i32(0).f(&[1.0, 2.0, 3.0]);
    read_ignored_exact_at(&w.0, "NiPSysBoxEmitter", header::VERSION_10_2_0_0, 9);

    let mut w = W::default();
    emitter(&mut w, true).i32(0).f(&[1.0, 2.0]);
    read_ignored_exact(&w.0, "NiPSysCylinderEmitter");

    let mut w = W::default();
    emitter(&mut w, true).i32(0).f(&[1.0]);
    read_ignored_exact(&w.0, "NiPSysSphereEmitter");

    let mut w = W::default();
    emitter(&mut w, true)
        .u32(1)
        .i32(3)
        .u32(0)
        .u32(2)
        .f(&[1.0, 0.0, 0.0]);
    read_ignored_exact(&w.0, "NiPSysMeshEmitter");
}

#[test]
fn planar_collider_is_read_to_the_end() {
    let mut w = W::default();
    w.f(&[0.5]).u8(0).u8(1).i32(-1).i32(2).i32(-1).i32(3);
    w.f(&[10.0, 20.0]).f(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    read_ignored_exact(&w.0, "NiPSysPlanarCollider");
}

#[test]
fn particle_controllers_are_read_to_the_end() {
    let mut w = W::default();
    time_controller(&mut w).i32(4).s("Emitter").i32(5);
    read_ignored_exact(&w.0, "NiPSysEmitterCtlr");

    for name in ["NiPSysUpdateCtlr", "NiPSysResetOnLoopCtlr"] {
        let mut w = W::default();
        time_controller(&mut w);
        read_ignored_exact(&w.0, name);
    }

    for name in [
        "NiPSysModifierActiveCtlr",
        "NiPSysGravityStrengthCtlr",
        "NiPSysEmitterSpeedCtlr",
        "NiPSysEmitterInitialRadiusCtlr",
        "NiPSysEmitterLifeSpanCtlr",
        "NiPSysEmitterDeclinationCtlr",
        "NiPSysEmitterDeclinationVarCtlr",
    ] {
        let mut w = W::default();
        time_controller(&mut w).i32(4).s("Gravity");
        read_ignored_exact(&w.0, name);
    }
}

#[test]
fn malleable_constraint_wraps_a_known_constraint() {
    let mut w = W::default();
    w.zeros(16)
        .u32(2)
        .zeros(16)
        .zeros(7 * 16 + 3 * 4)
        .f(&[0.1, 0.9]);
    read_ignored_exact(&w.0, "bhkMalleableConstraint");

    let mut w = W::default();
    w.zeros(16).u32(5).zeros(64);
    let mut cur = Cursor::new(&w.0);
    let err = read_block(&mut cur, 0, "bhkMalleableConstraint").unwrap_err();
    assert_eq!(
        err,
        Error::UnknownValue {
            at: 16,
            field: "bhkMalleableConstraint constraint type",
            value: 5
        }
    );
}

#[test]
fn blend_collision_wireframe_and_blend_controller_are_read_to_the_end() {
    let mut w = W::default();
    w.i32(0).u16(1).i32(2).f(&[1.0, 1.0]);
    read_ignored_exact(&w.0, "bhkBlendCollisionObject");
    w.f(&[0.0, 0.0]);
    read_ignored_exact_at(&w.0, "bhkBlendCollisionObject", 0x1400_0004, 8);

    let mut w = W::default();
    w.net("wire").u16(1);
    read_ignored_exact(&w.0, "NiWireframeProperty");

    let mut w = W::default();
    time_controller(&mut w).u32(0);
    read_ignored_exact(&w.0, "bhkBlendController");
}

#[test]
fn multi_sphere_shape_reads_its_spheres() {
    let mut w = W::default();
    w.u32(7)
        .zeros(12)
        .u32(2)
        .f(&[0.0, 0.0, 1.0, 0.5])
        .f(&[0.0, 0.0, 2.0, 0.25]);
    let shape = read_exact(&w.0, shapes::read_multi_sphere_shape);
    assert_eq!(shape.material, 7);
    assert_eq!(shape.spheres, [[0.0, 0.0, 1.0, 0.5], [0.0, 0.0, 2.0, 0.25]]);
    assert_truncation_is_error(&w.0, shapes::read_multi_sphere_shape);
}

// --- Старые 10.x (T2.3b) ------------------------------------------------------------

/// Как [`read_exact`], но у файла версия `version` и `BSVER` `bsver`.
fn read_exact_at<T>(
    bytes: &[u8],
    version: u32,
    bsver: u32,
    read: impl Fn(&mut Cursor<'_>) -> Result<T>,
) -> T {
    let with_version = |cur: &mut Cursor<'_>| {
        cur.set_version(version, bsver);
        read(cur)
    };
    let value = read_exact(bytes, with_version);
    assert_truncation_is_error(bytes, with_version);
    value
}

/// Файл 10.0.1.0: без пользовательской версии и заголовка Bethesda, перед каждым блоком `u32`.
fn old_file(separators: &[u32]) -> Vec<u8> {
    let mut w = W::default();
    w.0.extend_from_slice(b"NetImmerse File Format, Version 10.0.1.0\n");
    w.u32(header::VERSION_10_0_1_0).u32(2);
    w.u16(2).s("NiNode").s("BSXFlags");
    w.u16(0).u16(1).u32(0);
    w.u32(separators[0]);
    w.0.extend_from_slice(&node_body());
    w.u32(separators[1]);
    w.0.extend_from_slice(&bsx_body());
    w.u32(1).i32(0);
    w.0
}

#[test]
fn old_10x_blocks_are_preceded_by_a_zero_u32() {
    let nif = parse(&old_file(&[0, 0])).unwrap();
    assert!(matches!(nif.blocks[1], Block::BsxFlags(_)));

    assert!(matches!(
        parse(&old_file(&[0, 7])),
        Err(Error::InBlock { index: 1, source, .. })
            if matches!(*source, Error::UnknownValue { value: 7, .. })
    ));
}

#[test]
fn geometry_data_before_10_1_has_no_group_id_or_keep_flags() {
    let mut w = W::default();
    w.u16(3); // вершин; Group ID (с 10.1.0.114) и флагов (с 10.1.0.0) нет
    w.u8(1).f(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    w.u16(0).u8(0); // data flags, нормалей нет
    w.f(&[0.0, 0.0, 0.0, 1.0]).u8(0); // сфера, цветов нет
    w.u16(0); // consistency; Additional Data (с 20.0.0.4) нет
    w.u16(1).u32(3); // треугольников, индексов; `Has Triangles` (с 10.1.0.0) нет
    w.u16(0).u16(1).u16(2);
    w.u16(0); // групп нет
    let d = read_exact_at(
        &w.0,
        header::VERSION_10_0_1_0,
        0,
        geometry::read_tri_shape_data,
    );
    assert_eq!(d.group_id, 0);
    assert_eq!(d.triangles, [[0, 1, 2]]);

    let mut w = W::default();
    w.u16(3)
        .u8(1)
        .f(&[0.0; 9])
        .u16(0)
        .u8(0)
        .f(&[0.0; 4])
        .u8(0)
        .u16(0);
    w.u16(1).u16(1).u16(3); // треугольников, одна полоса из трёх; `Has Points` (с 10.0.1.3) нет
    w.u16(0).u16(1).u16(2);
    let d = read_exact_at(
        &w.0,
        header::VERSION_10_0_1_2,
        3,
        geometry::read_tri_strips_data,
    );
    assert_eq!(d.strips, [vec![0, 1, 2]]);

    // 10.1.0.106: флаги сохранения и сжатия есть, `Group ID` ещё нет.
    let mut w = W::default();
    w.u16(0)
        .u8(0)
        .u8(0)
        .u8(0)
        .u16(0)
        .u8(0)
        .f(&[0.0; 4])
        .u8(0)
        .u16(0);
    w.u16(0).u32(0).u8(0).u16(0);
    read_exact_at(
        &w.0,
        header::VERSION_10_1_0_106,
        5,
        geometry::read_tri_shape_data,
    );
}

#[test]
fn properties_before_10_0_1_3_carry_flags_and_textures_lack_direct_render() {
    let mut w = W::default();
    w.net("mat")
        .u16(1)
        .f(&[1.0; 3])
        .f(&[0.5; 3])
        .f(&[0.0; 3])
        .f(&[0.0; 3]);
    w.f(&[10.0, 1.0]);
    let m = read_exact_at(
        &w.0,
        header::VERSION_10_0_1_2,
        3,
        properties::read_material_property,
    );
    assert_eq!(m.diffuse, [0.5; 3]);

    let mut w = W::default();
    w.net("tex").u16(0).u32(2).u32(1); // флаги (до 10.0.1.2), apply, один слот
    w.u8(1).i32(4).u32(3).u32(2).u32(0).u16(0).u16(0); // base без преобразования
    w.u8(0).u8(0).u8(0).u8(0); // dark, detail, gloss, glow
    w.u32(0); // шейдерных нет
    let t = read_exact_at(
        &w.0,
        header::VERSION_10_0_1_0,
        0,
        properties::read_texturing_property,
    );
    assert_eq!(t.base.map(|d| d.source), Some(BlockRef(Some(4))));

    let mut w = W::default();
    w.net("t").u8(1).s("textures\\a.dds"); // внешняя: имя, ссылки нет
    w.u32(6).u32(1).u32(3).u8(1); // форматы, static; Direct Render (с 10.1.0.103) нет
    let s = read_exact_at(
        &w.0,
        header::VERSION_10_0_1_0,
        0,
        properties::read_source_texture,
    );
    assert_eq!(s.file_name, "textures\\a.dds");
    assert_eq!(s.pixel_data, BlockRef::NONE);
    assert!(s.direct_render);

    let mut w = W::default();
    w.net("t").u8(1).s("a.dds").i32(-1); // 10.1.0.101: ссылка есть, Direct Render нет
    w.u32(6).u32(1).u32(3).u8(1);
    read_exact_at(
        &w.0,
        header::VERSION_10_1_0_101,
        4,
        properties::read_source_texture,
    );
}

#[test]
fn controller_sequence_10_1_0_106_keeps_strings_in_its_blocks() {
    let mut w = W::default();
    w.s("Open").u32(1).u32(1); // имя, один блок, grow by
    w.i32(3).i32(4).i32(-1).u16(0).u8(26); // интерполятор, контроллер, смешивание, приоритет
    for s in ["Door", "", "NiTransformController", "", ""] {
        w.s(s);
    }
    w.f(&[1.0]).i32(-1).u32(0).f(&[1.0, 0.0, 0.0, 2.0]); // вес, ключи, цикл, частота…конец
    w.u8(0).i32(1).s("Door"); // назад, менеджер, корень накопления; палитры нет
    read_ignored_exact_at(&w.0, "NiControllerSequence", header::VERSION_10_1_0_106, 5);
}

#[test]
fn interpolators_and_controllers_10_1_0_106_have_old_fields() {
    // NiTransformInterpolator: NiQuatTransform с тремя флагами `TRS Valid`, данные.
    let mut w = W::default();
    w.f(&[0.0; 8]).u8(1).u8(1).u8(1).i32(2);
    read_ignored_exact_at(
        &w.0,
        "NiTransformInterpolator",
        header::VERSION_10_1_0_106,
        5,
    );

    // Смешивающий до 10.1.0.109: `u16`-размеры, элементы по 20 байт, `i32`-приоритеты.
    let mut w = W::default();
    w.u16(1).u16(1).i32(3).f(&[1.0, 1.0]).i32(0).f(&[0.0]);
    w.u8(0).f(&[0.0]).u8(0).u16(1).u16(0).i32(0).i32(0);
    w.f(&[0.0; 8]).u8(1).u8(1).u8(1); // значение-преобразование
    read_ignored_exact_at(
        &w.0,
        "NiBlendTransformInterpolator",
        header::VERSION_10_1_0_106,
        5,
    );

    // NiTransformController: `Manager Controlled` (10.1.0.104 … 10.1.0.108), интерполятор.
    let mut w = W::default();
    time_controller(&mut w).u8(1).i32(5);
    read_ignored_exact_at(&w.0, "NiTransformController", header::VERSION_10_1_0_106, 5);

    // До 10.1.0.104 интерполятора нет, есть ссылка на данные.
    let mut w = W::default();
    time_controller(&mut w).i32(5);
    read_ignored_exact_at(&w.0, "NiKeyframeController", header::VERSION_10_0_1_2, 3);
}

#[test]
fn light_10_1_0_101_has_affected_nodes_but_no_switch() {
    let mut w = W::default();
    w.av("Sun").u32(0).f(&[1.0]).f(&[0.5; 9]);
    read_ignored_exact_at(&w.0, "NiDirectionalLight", header::VERSION_10_1_0_101, 4);
}

#[test]
fn rigid_body_before_10_1_lacks_the_first_cinfo_fields() {
    let mut w = W::default();
    w.i32(2).u32(0); // форма, `u32` до 10.0.1.2
    w.u8(1).u8(0).u16(0); // фильтр
    w.zeros(20); // bhkWorldObjectCInfo
    w.zeros(4); // bhkEntityCInfo
    w.zeros(4); // unused 04
    w.f(&[1.0, 2.0, 3.0, 0.0]).f(&[0.0, 0.0, 0.0, 1.0]); // сдвиг, поворот
    w.zeros(16 + 16 + 48 + 16); // скорости, тензор, центр
    w.f(&[0.0, 0.1, 0.05, 0.3, 0.3]); // масса, демпфирование, трение, упругость
    w.u8(7).u8(1).u8(0).u8(0).zeros(12); // движение…, unused 05
    w.u32(0).u32(0); // связей нет, флаги
    let body = read_exact_at(&w.0, header::VERSION_10_0_1_2, 3, |cur| {
        read_rigid_body(cur, true)
    });
    assert_eq!(body.shape, BlockRef(Some(2)));
    assert_eq!(body.translation, [1.0, 2.0, 3.0, 0.0]);
    assert_eq!(body.filter.layer, 1);
}

#[test]
fn old_havok_shapes_are_read() {
    let mut w = W::default();
    w.zeros(8).f(&[0.1]).zeros(8).f(&[1.0, 1.0, 1.0, 0.0]);
    w.u32(1).zeros(12).zeros(12).u32(1).i32(5);
    let shape = read_exact_at(&w.0, header::VERSION_10_0_1_0, 0, havok::read_mesh_shape);
    assert_eq!(shape.strips_data, [BlockRef(Some(5))]);

    let mut w = W::default();
    w.i32(2).u32(4).f(&[0.1]).f(&[0.0, 0.0, 1.0]);
    let sweep = read_exact(&w.0, shapes::read_convex_sweep_shape);
    assert_eq!(sweep.shape, BlockRef(Some(2)));

    let mut w = W::default();
    w.i32(3).zeros(12).f(&[1.0]).u32(2).u8(9).u8(9); // MOPP без смещения (до 10.1.0.0)
    let mopp = read_exact_at(&w.0, header::VERSION_10_0_1_2, 3, havok::read_mopp_shape);
    assert_eq!(mopp.mopp_size, 2);
}

#[test]
fn boolean_and_integer_extra_data_are_read_to_the_end() {
    let mut w = W::default();
    w.s("flag").u8(1);
    read_ignored_exact(&w.0, "NiBooleanExtraData");
    let mut w = W::default();
    w.s("count").u32(7);
    read_ignored_exact(&w.0, "NiIntegerExtraData");
}

/// Файл из одних блоков анимации для `motion::sequences`: типы и байты блоков подряд. Сами
/// блоки в `SourceNif` второму проходу не нужны — только заголовок и смещения.
fn motion_file(version: u32, blocks: &[(&str, Vec<u8>)]) -> (Vec<u8>, SourceNif, Vec<usize>) {
    let mut bytes = Vec::new();
    let mut offsets = Vec::new();
    for (_, block) in blocks {
        offsets.push(bytes.len());
        bytes.extend_from_slice(block);
    }
    let source = SourceNif {
        header: NifHeader {
            line: String::new(),
            version,
            user_version: 11,
            user_version_2: 11,
            export_info: Default::default(),
            block_types: blocks.iter().map(|(t, _)| (*t).to_owned()).collect(),
            block_type_index: (0..blocks.len())
                .map(|i| u16::try_from(i).unwrap())
                .collect(),
            groups: Vec::new(),
            body_offset: 0,
        },
        blocks: Vec::new(),
        roots: Vec::new(),
    };
    (bytes, source, offsets)
}

/// T2.6: значения анимации двери читаются вторым проходом — имя узла из палитры, поза
/// интерполятора, первый и последний ключ; наклоны квадратичных ключей пропускаются.
#[test]
fn motion_reads_the_open_sequence_of_a_door() {
    use motion::{Rotations, sequences};

    let mut seq = W::default();
    seq.s("Open").u32(2).u32(1);
    // интерполятор, контроллер, приоритет, палитра, смещение имени узла и ещё четыре
    seq.i32(2)
        .i32(9)
        .u8(26)
        .i32(1)
        .u32(6)
        .u32(13)
        .u32(13)
        .u32(13)
        .u32(13);
    seq.i32(4)
        .i32(9)
        .u8(26)
        .i32(1)
        .u32(0)
        .u32(13)
        .u32(13)
        .u32(13)
        .u32(13);
    seq.f(&[1.0])
        .i32(-1)
        .u32(0)
        .f(&[1.0, 0.25, 2.5])
        .i32(8)
        .s("Door01")
        .i32(1);

    let mut palette = W::default();
    palette.s("Frame\0Door01\0\0").u32(14);

    let mut interp = W::default();
    interp
        .f(&[48.8, 2.9, 0.0])
        .f(&[-f32::MAX, -f32::MAX, -f32::MAX, -f32::MAX])
        .f(&[1.0])
        .i32(3);

    let mut data = W::default();
    // Два линейных ключа поворота: время, w, x, y, z.
    data.u32(2).u32(1);
    data.f(&[0.0, 1.0, 0.0, 0.0, 0.0]);
    data.f(&[2.5, 0.66, 0.0, 0.0, -0.75]);
    // Два квадратичных ключа сдвига: время, значение и два наклона по три числа.
    data.u32(2).u32(2);
    data.f(&[0.0, 48.8, 2.9, 0.0]).zeros(24);
    data.f(&[2.5, 48.8, 2.9, 1.5]).zeros(24);
    data.u32(0);

    let mut float_interp = W::default();
    float_interp.f(&[0.5]).i32(-1);

    let (bytes, source, offsets) = motion_file(
        0x1400_0004,
        &[
            ("NiControllerSequence", seq.0),
            ("NiStringPalette", palette.0),
            ("NiTransformInterpolator", interp.0),
            ("NiTransformData", data.0),
            ("NiFloatInterpolator", float_interp.0),
        ],
    );
    let found = sequences(&bytes, &source, &offsets).unwrap();
    assert_eq!(found.len(), 1);
    let open = &found[0];
    assert_eq!(open.name, "Open");
    assert_eq!((open.start, open.stop), (0.25, 2.5));
    assert_eq!(
        open.tracks
            .iter()
            .map(|t| t.node.as_str())
            .collect::<Vec<_>>(),
        ["Door01", "Frame"]
    );
    let keys = open.tracks[0].keys.as_ref().unwrap();
    assert_eq!(keys.pose_translation, Some([48.8, 2.9, 0.0]));
    assert_eq!(keys.pose_rotation, None, "−MAX — «не задано»");
    assert_eq!(
        keys.rotations,
        Rotations::Quat(vec![
            (0.0, [1.0, 0.0, 0.0, 0.0]),
            (2.5, [0.66, 0.0, 0.0, -0.75])
        ])
    );
    assert_eq!(
        keys.translations,
        [(0.0, [48.8, 2.9, 0.0]), (2.5, [48.8, 2.9, 1.5])]
    );
    assert_eq!(open.tracks[1].keys, None, "не NiTransformInterpolator");

    // Усечение файла в любом месте — ошибка или меньше данных, но не паника.
    for cut in 0..bytes.len() {
        let _ = sequences(&bytes[..cut], &source, &offsets);
    }
}

/// Дорожки Эйлера и раскладка 10.1.0.106: блок управления несёт строки сам, у интерполятора —
/// три байта `TRS Valid`, у последовательности — фаза и «проигрывать назад».
#[test]
fn motion_reads_euler_keys_and_the_inline_names_of_10_1() {
    use motion::{Rotations, sequences};

    let mut seq = W::default();
    seq.s("Open").u32(1).u32(1);
    // интерполятор, контроллер, смешивающий, его индекс, приоритет, пять строк
    seq.i32(1).i32(9).i32(-1).u16(0).u8(26);
    seq.s("Gate").s("").s("NiTransformController").s("").s("");
    seq.f(&[1.0])
        .i32(-1)
        .u32(0)
        .f(&[1.0, 0.0, 0.0, 4.0])
        .u8(0)
        .i32(8)
        .s("Gate");

    let mut interp = W::default();
    interp
        .f(&[0.0, 0.0, 0.0])
        .f(&[1.0, 0.0, 0.0, 0.0])
        .f(&[1.0])
        .u8(1)
        .u8(1)
        .u8(1)
        .i32(2);

    let mut data = W::default();
    // XYZ: число ключей (любое ненулевое), тип 4, три группы; у Z — два ключа TBC.
    data.u32(1).u32(4);
    data.u32(0);
    data.u32(0);
    data.u32(2).u32(3);
    data.f(&[0.0, 0.0]).zeros(12);
    data.f(&[4.0, -1.5]).zeros(12);
    data.u32(0);
    data.u32(0);

    let (bytes, source, offsets) = motion_file(
        0x0A01_006A,
        &[
            ("NiControllerSequence", seq.0),
            ("NiTransformInterpolator", interp.0),
            ("NiKeyframeData", data.0),
        ],
    );
    let found = sequences(&bytes, &source, &offsets).unwrap();
    assert_eq!(found[0].tracks[0].node, "Gate");
    assert_eq!((found[0].start, found[0].stop), (0.0, 4.0));
    let keys = found[0].tracks[0].keys.as_ref().unwrap();
    assert_eq!(keys.pose_rotation, Some([1.0, 0.0, 0.0, 0.0]));
    assert_eq!(
        keys.rotations,
        Rotations::Euler([Vec::new(), Vec::new(), vec![(0.0, 0.0), (4.0, -1.5)]])
    );
    assert!(keys.translations.is_empty());

    // До 10.1.0.106 блок управления другой: последовательности не читаются, а не читаются криво.
    let (bytes, mut old, offsets) =
        motion_file(0x0A01_0065, &[("NiControllerSequence", vec![0; 4])]);
    old.header.version = 0x0A01_0065;
    assert_eq!(sequences(&bytes, &old, &offsets), Ok(Vec::new()));
}

/// Смещения блоков — те самые места, с которых `read_block` читал блок.
#[test]
fn parse_with_offsets_points_at_block_starts() {
    let bytes = file(&["NiNode", "BSXFlags"], &[0, 1], &[node_body(), bsx_body()]);
    let (source, offsets) = parse_with_offsets(&bytes).unwrap();
    assert_eq!(
        offsets,
        [
            source.header.body_offset,
            source.header.body_offset + node_body().len()
        ]
    );
    assert_eq!(parse(&bytes).unwrap(), source);
}

/// Общая часть воздействия `NiPSysModifier`: имя, порядок, владелец, включено.
fn psys_modifier<'a>(w: &'a mut W, name: &str, active: bool) -> &'a mut W {
    w.s(name).u32(1).i32(0).u8(u8::from(active))
}

/// `NiParticleSystem` с контроллером `controller` и списком воздействий.
fn particle_system(controller: i32, modifiers: &[i32]) -> Vec<u8> {
    let mut w = W::default();
    w.s("Fire").u32(0).i32(controller);
    w.u16(0x000E)
        .f(&[1.0, 2.0, 3.0])
        .f(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
        .f(&[1.0])
        .u32(0)
        .i32(-1);
    // данные, скин, шейдера нет; затем «в мире» и воздействия
    w.i32(1).i32(-1).u8(0).u8(1);
    w.u32(u32::try_from(modifiers.len()).unwrap());
    for m in modifiers {
        w.i32(*m);
    }
    w.0
}

/// Начало `NiPSysData` — `NiGeometryData` с пулом из `pool` вершин без массивов.
fn psys_data(pool: u16) -> Vec<u8> {
    let mut w = W::default();
    w.i32(0).u16(pool).u8(0).u8(0).u8(0).u16(0).u8(0);
    w.f(&[0.0, 0.0, 0.0, 10.0]).u8(0).u16(0).i32(-1);
    w.0
}

fn box_emitter(name: &str) -> Vec<u8> {
    let mut w = W::default();
    psys_modifier(&mut w, name, true);
    // скорость и её разброс, склонение, плоский угол с разбросами
    w.f(&[20.0, 10.0, 0.1, 0.28, 0.2, 3.0]);
    w.f(&[1.0, 0.5, 0.25, 1.0]).f(&[8.0, 0.0]).f(&[1.5, 0.5]);
    w.i32(4).f(&[1.0, 2.0, 3.0]);
    w.0
}

/// `NiPSysEmitterCtlr`: флаги, окно, интерполятор, имя эмиттера.
fn emitter_controller(flags: u16, interpolator: i32, emitter: &str) -> Vec<u8> {
    let mut w = W::default();
    w.i32(-1).u16(flags).f(&[1.0, 0.0, 0.0, 0.95]).i32(0);
    w.i32(interpolator).s(emitter).i32(-1);
    w.0
}

/// T2.7: система частиц читается со значениями — эмиттер, темп рождения из своего
/// интерполятора, пул, воздействия; выключенное воздействие остаётся выключенным.
#[test]
fn psys_reads_a_self_driven_particle_system() {
    use psys::{EmitterShape, ModifierKind, systems};

    let mut rate = W::default();
    rate.f(&[13.5]).i32(-1);

    let mut grow = W::default();
    psys_modifier(&mut grow, "NiPSysGrowFadeModifier:3", true)
        .f(&[0.15])
        .u16(0)
        .f(&[1.35])
        .u16(0);

    let mut color = W::default();
    psys_modifier(&mut color, "NiPSysColorModifier:4", true).i32(7);
    let mut color_data = W::default();
    color_data.u32(2).u32(1);
    color_data.f(&[0.0, 1.0, 1.0, 1.0, 1.0]);
    color_data.f(&[1.0, 1.0, 0.0, 0.0, 0.0]);

    let mut gravity = W::default();
    psys_modifier(&mut gravity, "NiPSysGravityModifier:5", true)
        .i32(4)
        .f(&[0.0, 0.0, -1.0])
        .f(&[0.5, 16.0])
        .u32(1)
        .f(&[3.0, 1.0]);

    let mut rotation = W::default();
    psys_modifier(&mut rotation, "NiPSysRotationModifier:6", true)
        .f(&[2.0])
        .f(&[0.5, 0.0, 0.0])
        .u8(1)
        .u8(1)
        .f(&[1.0, 0.0, 0.0]);

    let mut age = W::default();
    psys_modifier(&mut age, "NiPSysAgeDeathModifier:2", true)
        .u8(1)
        .i32(11);
    let mut spawn = W::default();
    psys_modifier(&mut spawn, "NiPSysSpawnModifier:1", true)
        .u16(2)
        .f(&[0.5])
        .u16(1)
        .u16(3)
        .f(&[0.2, 0.3, 0.0, 0.0]);

    let mut drag = W::default();
    psys_modifier(&mut drag, "NiPSysDragModifier:7", false)
        .i32(-1)
        .f(&[0.0, 0.0, 1.0])
        .f(&[0.5, 100.0, 1.0]);
    let mut position = W::default();
    psys_modifier(&mut position, "NiPSysPositionModifier:8", true);

    let (bytes, source, offsets) = motion_file(
        0x1400_0004,
        &[
            (
                "NiParticleSystem",
                particle_system(3, &[2, 10, 11, 5, 6, 8, 9, 12, 13]),
            ),
            ("NiPSysData", psys_data(20)),
            ("NiPSysBoxEmitter", box_emitter("NiPSysBoxEmitter:0")),
            (
                "NiPSysEmitterCtlr",
                emitter_controller(0x0008, 14, "NiPSysBoxEmitter:0"),
            ),
            ("NiNode", node_body()),
            ("NiPSysGrowFadeModifier", grow.0),
            ("NiPSysColorModifier", color.0),
            ("NiColorData", color_data.0),
            ("NiPSysGravityModifier", gravity.0),
            ("NiPSysRotationModifier", rotation.0),
            ("NiPSysAgeDeathModifier", age.0),
            ("NiPSysSpawnModifier", spawn.0),
            ("NiPSysDragModifier", drag.0),
            ("NiPSysPositionModifier", position.0),
            ("NiFloatInterpolator", rate.0),
        ],
    );
    let found = systems(&bytes, &source, &offsets).unwrap();
    assert_eq!(found.len(), 1);
    let s = &found[0];
    assert_eq!((s.index, s.av.net.name.as_str()), (0, "Fire"));
    assert!(s.world_space && !s.mesh_particles);
    assert_eq!(s.pool, 20);

    let e = s.emitter.as_ref().unwrap();
    assert_eq!(e.name, "NiPSysBoxEmitter:0");
    assert_eq!((e.speed, e.speed_variation), (20.0, 10.0));
    assert_eq!((e.declination, e.declination_variation), (0.1, 0.28));
    assert_eq!((e.planar_angle, e.planar_angle_variation), (0.2, 3.0));
    assert_eq!(e.color, [1.0, 0.5, 0.25, 1.0]);
    assert_eq!(
        (e.radius, e.life_span, e.life_span_variation),
        (8.0, 1.5, 0.5)
    );
    assert_eq!(e.object, BlockRef(Some(4)));
    assert_eq!(
        e.shape,
        EmitterShape::Box {
            width: 1.0,
            height: 2.0,
            depth: 3.0
        }
    );

    let emission = s.emission.as_ref().unwrap();
    assert_eq!(
        (emission.flags, emission.start, emission.stop),
        (8, 0.0, 0.95)
    );
    assert_eq!(emission.mean_rate(), Some(13.5));
    assert!(!emission.triggered && !emission.once && emission.sequence.is_none());

    let kinds: Vec<&ModifierKind> = s.modifiers.iter().map(|m| &m.kind).collect();
    assert_eq!(
        kinds,
        [
            &ModifierKind::AgeDeath {
                spawn_on_death: true
            },
            &ModifierKind::Spawn {
                generations: 2,
                percentage: 0.5,
                min: 1,
                max: 3,
                speed_chaos: 0.2,
                direction_chaos: 0.3
            },
            &ModifierKind::GrowFade {
                grow: 0.15,
                fade: 1.35
            },
            &ModifierKind::Color {
                keys: vec![(0.0, [1.0; 4]), (1.0, [1.0, 0.0, 0.0, 0.0])]
            },
            &ModifierKind::Gravity {
                object: BlockRef(Some(4)),
                axis: [0.0, 0.0, -1.0],
                decay: 0.5,
                strength: 16.0,
                force_type: 1,
                turbulence: 3.0
            },
            &ModifierKind::Rotation {
                speed: 2.0,
                random_axis: true,
                axis: [1.0, 0.0, 0.0]
            },
            &ModifierKind::Other("NiPSysDragModifier"),
            &ModifierKind::Housekeeping,
        ]
    );
    assert!(!s.modifiers[6].active, "сопротивление выключено");

    // Усечение файла в любом месте — ошибка или меньше данных, но не паника.
    for cut in 0..bytes.len() {
        let _ = systems(&bytes[..cut], &source, &offsets);
    }
}

/// Контроллер без своего интерполятора: темп и окно — из последовательности менеджера. `Idle`
/// в петле — постоянный эффект; только `Forward` или зажатая `Idle` — эффект по событию.
#[test]
fn psys_takes_the_birth_rate_from_an_ambient_sequence_or_marks_the_system_triggered() {
    use psys::systems;

    let sequence = |name: &str, cycle: u32, interpolator: i32| {
        let mut w = W::default();
        w.s(name).u32(1).u32(1);
        // интерполятор, контроллер (блок 3), приоритет, палитра, пять смещений
        w.i32(interpolator).i32(3).u8(26).i32(-1);
        w.u32(0).u32(0).u32(0).u32(0).u32(0);
        w.f(&[1.0])
            .i32(-1)
            .u32(cycle)
            .f(&[1.0, 0.5, 4.5])
            .i32(-1)
            .s("Root")
            .i32(-1);
        w.0
    };
    let interpolator = |data: i32| {
        let mut w = W::default();
        w.f(&[-f32::MAX]).i32(data);
        w.0
    };
    let keys = |values: &[(f32, f32)]| {
        let mut w = W::default();
        w.u32(u32::try_from(values.len()).unwrap()).u32(1);
        for (t, v) in values {
            w.f(&[*t, *v]);
        }
        w.0
    };
    let file = |sequences: Vec<(&'static str, Vec<u8>)>| {
        let mut blocks = vec![
            ("NiParticleSystem", particle_system(3, &[2])),
            ("NiPSysData", psys_data(50)),
            ("NiPSysBoxEmitter", box_emitter("Emitter")),
            // Управляется менеджером: своего интерполятора нет.
            (
                "NiPSysEmitterCtlr",
                emitter_controller(0x0028, -1, "Emitter"),
            ),
            ("NiNode", node_body()),
            ("NiFloatInterpolator", interpolator(6)),
            ("NiFloatData", keys(&[(0.0, 0.0), (1.0, 12.0), (3.0, 12.0)])),
            ("NiFloatInterpolator", interpolator(8)),
            ("NiFloatData", keys(&[(0.0, 40.0)])),
        ];
        blocks.extend(sequences);
        motion_file(0x1400_0004, &blocks)
    };

    // `Forward` стоит в файле первой, но мир сам проигрывает `Idle`.
    let (bytes, source, offsets) = file(vec![
        ("NiControllerSequence", sequence("Forward", 2, 7)),
        ("NiControllerSequence", sequence("idle", 0, 5)),
    ]);
    let found = systems(&bytes, &source, &offsets).unwrap();
    let e = found[0].emission.as_ref().unwrap();
    assert_eq!(e.sequence.as_deref(), Some("idle"));
    assert!(!e.triggered && !e.once);
    assert_eq!((e.start, e.stop), (0.5, 4.5), "окно — у последовательности");
    assert_eq!(e.mean_rate(), Some(10.0), "(6 + 24) / 3");

    // Только действие: частицы — по событию.
    let (bytes, source, offsets) = file(vec![("NiControllerSequence", sequence("Forward", 2, 7))]);
    let found = systems(&bytes, &source, &offsets).unwrap();
    let e = found[0].emission.as_ref().unwrap();
    assert_eq!(e.sequence.as_deref(), Some("Forward"));
    assert!(e.triggered && e.once);
    assert_eq!(e.mean_rate(), Some(40.0));

    // Зажатая `SpecialIdle` — разовая вспышка, тоже по событию.
    let (bytes, source, offsets) = file(vec![(
        "NiControllerSequence",
        sequence("SpecialIdle", 2, 5),
    )]);
    let found = systems(&bytes, &source, &offsets).unwrap();
    assert!(found[0].emission.as_ref().unwrap().triggered);

    // Контроль: последовательностей нет вовсе — темпа нет, но и «по событию» сказать нельзя.
    let (bytes, source, offsets) = file(Vec::new());
    let found = systems(&bytes, &source, &offsets).unwrap();
    let e = found[0].emission.as_ref().unwrap();
    assert_eq!(e.mean_rate(), None);
    assert!(!e.triggered);
}

/// T11.5b: блоки B-сплайнов и уровни детализации скелета читаются до конца.
#[test]
fn spline_blocks_and_bone_lod_are_read_to_the_end() {
    // Начало, конец, данные, базис; поза; три смещения опорных точек.
    let mut w = W::default();
    w.f(&[0.0, 2.0]).i32(3).i32(4);
    w.f(&[0.0; 3]).f(&[1.0, 0.0, 0.0, 0.0]).f(&[1.0]);
    w.u32(0).u32(12).u32(0xFFFF);
    read_ignored_exact(&w.0, "NiBSplineTransformInterpolator");
    // Сжатый — ещё шесть чисел: смещение и множитель сдвига, поворота, масштаба.
    w.f(&[0.0, 10.0, 0.0, 1.0, 0.0, 1.0]);
    read_ignored_exact(&w.0, "NiBSplineCompTransformInterpolator");

    let mut w = W::default();
    w.f(&[0.0, 2.0])
        .i32(3)
        .i32(4)
        .f(&[0.5])
        .u32(0)
        .f(&[0.0, 1.0]);
    read_ignored_exact(&w.0, "NiBSplineCompFloatInterpolator");

    // Два `f32` и три `i16`.
    let mut w = W::default();
    w.u32(2).f(&[1.0, 2.0]).u32(3).u16(1).u16(2).u16(3);
    read_ignored_exact(&w.0, "NiBSplineData");

    let mut w = W::default();
    w.u32(7);
    read_ignored_exact(&w.0, "NiBSplineBasisData");

    // Два уровня: три кости и одна.
    let mut w = W::default();
    time_controller(&mut w).u32(0).u32(2).u32(2);
    w.u32(3).i32(1).i32(2).i32(3);
    w.u32(1).i32(1);
    read_ignored_exact(&w.0, "NiBSBoneLODController");
}

#[test]
fn bs_bound_reads_center_and_half_extents() {
    let mut w = W::default();
    w.s("BBX").f(&[0.0, 0.0, 35.0]).f(&[16.0, 63.0, 35.0]);
    let b = read_exact(&w.0, extra::read_bs_bound);
    assert_eq!(b.name, "BBX");
    assert_eq!(b.center, [0.0, 0.0, 35.0]);
    assert_eq!(b.half_extents, [16.0, 63.0, 35.0]);
    assert_truncation_is_error(&w.0, extra::read_bs_bound);
    assert!(matches!(
        read_exact(&w.0, |cur| read_block(cur, 0, "BSBound")),
        Block::BsBound(_)
    ));
}

/// T11.5b: клип существа — метки, корень накопления, дорожка на сжатом сплайне и дорожка на
/// ключах с масштабом.
#[test]
fn clip_reads_marks_accum_root_keys_and_compact_splines() {
    use clip::{Channel, Rotation, clips};

    let mut seq = W::default();
    seq.s("Forward").u32(2).u32(1);
    // интерполятор, контроллер, приоритет, палитра, смещение имени узла и ещё четыре
    for (interpolator, name_at) in [(2, 0), (5, 6)] {
        seq.i32(interpolator).i32(-1).u8(26).i32(1).u32(name_at);
        seq.u32(17).u32(17).u32(17).u32(17);
    }
    // вес, метки, цикл, частота, начало, конец, менеджер, корень накопления, палитра
    seq.f(&[1.0])
        .i32(6)
        .u32(0)
        .f(&[1.0, 0.0, 2.0])
        .i32(-1)
        .s("Bip01")
        .i32(1);

    let mut palette = W::default();
    palette.s("Bip01\0Bip01 Tail\0").u32(17);

    // Сплайн: сдвиг — с нулевого числа, поворота и масштаба нет; сдвиг = i16 / 32767 × 50 + 100.
    let mut spline = W::default();
    spline.f(&[0.0, 2.0]).i32(3).i32(4);
    spline.f(&[0.0, 0.0, 5.0]).f(&[-f32::MAX; 4]).f(&[1.0]);
    spline.u32(0).u32(0xFFFF).u32(0xFFFF);
    spline.f(&[100.0, 50.0, 0.0, 1.0, 0.0, 1.0]);

    let mut data = W::default();
    data.u32(0).u32(12);
    for y in [-32767i16, 0, 0, 32767] {
        for v in [0i16, y, 0] {
            data.0.extend_from_slice(&v.to_le_bytes());
        }
    }

    let mut basis = W::default();
    basis.u32(4);

    let mut keyed = W::default();
    keyed
        .f(&[1.0, 2.0, 3.0])
        .f(&[1.0, 0.0, 0.0, 0.0])
        .f(&[-f32::MAX])
        .i32(7);

    let mut marks = W::default();
    marks.s("").u32(3);
    marks.f(&[0.0]).s("start");
    marks.f(&[0.5]).s("Enum: Left");
    marks.f(&[2.0]).s("end");

    let mut keys = W::default();
    keys.u32(0);
    keys.u32(2).u32(1);
    keys.f(&[0.0, 1.0, 2.0, 3.0]).f(&[2.0, 1.0, 2.0, 9.0]);
    keys.u32(1).u32(1).f(&[0.0, 2.0]);

    let (bytes, source, offsets) = motion_file(
        0x1400_0004,
        &[
            ("NiControllerSequence", seq.0),
            ("NiStringPalette", palette.0),
            ("NiBSplineCompTransformInterpolator", spline.0),
            ("NiBSplineData", data.0),
            ("NiBSplineBasisData", basis.0),
            ("NiTransformInterpolator", keyed.0),
            ("NiTextKeyExtraData", marks.0),
            ("NiTransformData", keys.0),
        ],
    );
    let found = clips(&bytes, &source, &offsets).unwrap();
    assert_eq!(found.len(), 1);
    let clip = &found[0];
    assert_eq!(clip.name, "Forward");
    assert_eq!(clip.accum_root, "Bip01");
    assert_eq!((clip.cycle, clip.frequency), (0, 1.0));
    assert_eq!((clip.start, clip.stop), (0.0, 2.0));
    assert_eq!(
        clip.text_keys,
        [
            (0.0, "start".to_owned()),
            (0.5, "Enum: Left".to_owned()),
            (2.0, "end".to_owned())
        ]
    );

    let root = &clip.tracks[0];
    assert_eq!(root.node, "Bip01");
    assert_eq!(root.interpolator, "NiBSplineCompTransformInterpolator");
    let motion = root.motion.as_ref().unwrap();
    assert_eq!(motion.pose_translation, Some([0.0, 0.0, 5.0]));
    assert_eq!(motion.pose_rotation, None, "−MAX — «не задано»");
    assert_eq!(motion.rotation, Rotation::None);
    assert_eq!(motion.scale, Channel::None);
    let Channel::Spline(travel) = &motion.translation else {
        panic!("сдвиг — сплайн: {:?}", motion.translation);
    };
    assert_eq!((travel.start, travel.stop), (0.0, 2.0));
    assert_eq!(
        travel.points,
        [
            [100.0, 50.0, 100.0],
            [100.0, 100.0, 100.0],
            [100.0, 100.0, 100.0],
            [100.0, 150.0, 100.0]
        ]
    );

    let tail = &clip.tracks[1];
    assert_eq!(tail.node, "Bip01 Tail");
    let motion = tail.motion.as_ref().unwrap();
    assert_eq!(motion.pose_rotation, Some([1.0, 0.0, 0.0, 0.0]));
    assert_eq!(motion.pose_scale, None);
    assert_eq!(motion.rotation, Rotation::None);
    assert_eq!(
        motion.translation,
        Channel::Keys(vec![(0.0, [1.0, 2.0, 3.0]), (2.0, [1.0, 2.0, 9.0])])
    );
    assert_eq!(motion.scale, Channel::Keys(vec![(0.0, [2.0])]));

    // Смещение опорных точек за концом данных — ошибка, а не чтение мимо.
    let mut bad = bytes.clone();
    let handle_at = offsets[2] + 16 + 32;
    bad[handle_at..handle_at + 4].copy_from_slice(&1u32.to_le_bytes());
    assert!(matches!(
        clips(&bad, &source, &offsets),
        Err(Error::UnknownValue { value: 1, .. })
    ));

    // Усечение файла в любом месте — ошибка или меньше данных, но не паника.
    for cut in 0..bytes.len() {
        let _ = clips(&bytes[..cut], &source, &offsets);
    }
}

/// Кубический B-сплайн, прижатый к концам: на краях — крайние опорные точки, четыре точки —
/// кривая Безье, между точками на прямой — точка той же прямой.
#[test]
fn spline_is_clamped_cubic_and_degrades_with_few_points() {
    use clip::Spline;

    let bezier = Spline {
        start: 1.0,
        stop: 3.0,
        points: vec![[0.0], [10.0], [20.0], [40.0]],
    };
    assert_eq!(bezier.at(1.0), Some([0.0]));
    assert_eq!(bezier.at(3.0), Some([40.0]));
    // Середина кривой Безье: (P0 + 3·P1 + 3·P2 + P3) / 8.
    let mid = bezier.at(2.0).unwrap()[0];
    assert!((mid - 16.25).abs() < 1e-4, "{mid}");
    // За пределами отрезка — значение на краю.
    assert_eq!(bezier.at(-5.0), Some([0.0]));
    assert_eq!(bezier.at(9.0), Some([40.0]));

    // Семь точек на прямой y = 2x: кривая идёт по той же прямой и не поворачивает назад.
    let line = Spline {
        start: 0.0,
        stop: 1.0,
        points: (0..7u8)
            .map(|i| [f32::from(i), 2.0 * f32::from(i)])
            .collect(),
    };
    let mut before = -1.0;
    for step in 0..=20u8 {
        let [x, y] = line.at(f32::from(step) / 20.0).unwrap();
        assert!((y - 2.0 * x).abs() < 1e-4, "{x} {y}");
        assert!(x > before, "{x} после {before}");
        before = x;
    }
    assert_eq!(line.at(1.0), Some([6.0, 12.0]));

    // Все точки равны — кривая стоит.
    let still = Spline {
        start: 0.0,
        stop: 2.0,
        points: vec![[3.0, 4.0]; 9],
    };
    for step in 0..=8u8 {
        let v = still.at(f32::from(step) / 4.0).unwrap();
        assert!((v[0] - 3.0).abs() < 1e-5 && (v[1] - 4.0).abs() < 1e-5);
    }

    // Меньше четырёх точек — степень ниже: две точки — отрезок, одна — постоянная, ни одной — нет.
    let two = Spline {
        start: 0.0,
        stop: 1.0,
        points: vec![[0.0], [8.0]],
    };
    assert_eq!(two.at(0.5), Some([4.0]));
    let one = Spline {
        start: 0.0,
        stop: 1.0,
        points: vec![[7.0]],
    };
    assert_eq!(one.at(0.5), Some([7.0]));
    let none: Spline<1> = Spline {
        start: 0.0,
        stop: 1.0,
        points: Vec::new(),
    };
    assert_eq!(none.at(0.5), None);
    // Отрезок нулевой длины — начало, а не деление на ноль.
    let instant = Spline {
        start: 2.0,
        stop: 2.0,
        points: vec![[1.0], [2.0], [3.0], [4.0]],
    };
    assert_eq!(instant.at(2.0), Some([1.0]));
}
