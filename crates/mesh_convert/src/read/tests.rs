//! Synthetic block tests: the bytes are assembled by hand following the layout in `nif.xml`, so the
//! test catches a mismatch between the code and the layout, not between the code and itself. The
//! check on real files is `tests/real_blocks.rs`.

use super::cursor::Cursor;
use super::havok::{read_collision_object, read_rigid_body};
use super::*;

/// A byte writer.
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
    /// A `NiAVObject` with noticeable values, so a mixed-up field order is visible.
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

/// Parses a block and requires the cursor to stop exactly at the end.
fn read_exact<T>(bytes: &[u8], read: impl FnOnce(&mut Cursor<'_>) -> Result<T>) -> T {
    let mut cur = Cursor::new(bytes);
    let value = read(&mut cur).unwrap();
    assert_eq!(cur.remaining(), 0, "the block was not parsed to the end");
    value
}

/// Any truncation of a block is an error, not a panic.
fn assert_truncation_is_error<T>(bytes: &[u8], read: impl Fn(&mut Cursor<'_>) -> Result<T>) {
    for cut in 0..bytes.len() {
        let mut cur = Cursor::new(&bytes[..cut]);
        assert!(read(&mut cur).is_err(), "truncation to {cut} bytes passed");
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

/// Strips: `[0,1,2,3]` and `[2,3,0]` — 2 + 1 = 3 triangles.
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
    w.f(&[0.5, 0.5, 0.0]).f(&[1.5]); // bounding sphere
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
    w.u16(0).u16(1).u16(5).u8(0); // 1 strip of length 5, Has Points = 0
    let data = read_exact(&w.0, geometry::read_tri_strips_data);
    assert!(data.vertices.is_empty() && data.uv_sets.is_empty());
    assert!(data.strips.is_empty());
}

#[test]
fn a_huge_declared_strip_count_is_an_error_not_an_allocation() {
    let mut bytes = strips_data(false, false, 0);
    // after the block header and the arrays, the strip count is the last u16 before the lengths;
    // simpler to put a deliberately large vertex count
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
    w.u8(0); // decal 0: texture_count 7 covers only it (needs > 6)
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
    // texture_count 9 covers decal 0..2 (needs > 6, > 7, > 8), not decal 3
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
    w.u8(1).u8(0).u16(0); // HavokFilter: layer, flags, group
    w.zeros(4).u8(1).zeros(3).zeros(12); // world info
    w.u8(1).u8(0).u16(0xFFFF); // bhkEntityCInfo
    w.zeros(4); // unused 01
    w.u8(1).u8(0).u16(0); // filter in CInfo
    w.zeros(4); // unused 02
    w.u8(1).u8(0).u16(0xFFFF); // response, unused 03, delay
    w.zeros(4); // unused 04
    w.f(&[10.0, 20.0, 30.0, 0.0]); // translation
    w.f(&[0.0, 0.0, 0.707, 0.707]); // rotation
    w.zeros(32).zeros(48).zeros(16); // velocities, inertia, center
    w.f(&[0.0, 0.1, 0.05, 0.5, 0.4, 104.4, 31.57, 0.15]);
    w.u8(7).u8(0).u8(0).u8(1); // motion, deactivator, solver, quality
    w.zeros(12);
    w.u32(1).i32(11); // constraints
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
// The whole file
// ---------------------------------------------------------------------------

/// Builds a NIF 20.0.0.4: the header, the block bodies, the footer with root `0`.
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
        other => panic!("expected TrailingData, got {other:?}"),
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
    // NiTriStrips with boolean `Has Shader` = 2: the layout drifted inside block 1.
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
        other => panic!("expected InBlock, got {other:?}"),
    }
}

#[test]
fn every_truncation_of_a_whole_file_is_an_error_not_a_panic() {
    let bytes = file(&["NiNode", "BSXFlags"], &[0, 1], &[node_body(), bsx_body()]);
    for cut in 0..bytes.len() {
        assert!(
            parse(&bytes[..cut]).is_err(),
            "truncation to {cut} bytes passed"
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

// --- T1.3w: the remaining properties and shapes -----------------------------------------

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
    w.u16(1).u32(0).u32(3).u32(0); // one subpart: filter, vertices, material
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
    // A type from the list must not give UnsupportedBlock (on empty data any other error is fine).
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
    w.u16(0); // data_flags: no UV and no tangents
    w.u8(0); // no normals
    w.f(&[0.0, 0.0, 0.0, 1.0]); // sphere
    w.u8(0); // no colors
    w.u16(0).i32(-1); // consistency, additional data
    w.u16(1).u32(3).u8(1); // 1 triangle, 3 indices, triangles present
    w.u16(0).u16(1).u16(2);
    w.u16(1).u16(2).u16(0).u16(1); // one match group of two vertices
    let d = read_exact(&w.0, geometry::read_tri_shape_data);
    assert_eq!(d.triangles, [[0, 1, 2]]);
    assert!(d.strips.is_empty());
    assert_eq!(d.vertices.len(), 3);
    assert_truncation_is_error(&w.0, geometry::read_tri_shape_data);
}

/// A block from `anim::IGNORED_TYPES` read through `read_block`, exactly to the end.
fn read_ignored_exact(bytes: &[u8], type_name: &str) {
    let block = read_exact(bytes, |cur| read_block(cur, 0, type_name));
    assert!(
        matches!(block, Block::Ignored(i) if i.type_name == type_name),
        "{type_name}: {block:?}"
    );
    assert_truncation_is_error(bytes, |cur| read_block(cur, 0, type_name));
}

/// `NiTimeController`: next, flags, frequency, phase, start, stop, target.
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
        // interpolator, controller, priority, palette, five offsets
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

    // Value 2 is "not set": this is a byte, not a bool, and there must be no refusal.
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
    // Flag 0: the full set of fields and two items of 17 bytes.
    let mut w = W::default();
    w.u8(0).u8(2).f(&[0.0]).u8(1).u8(0).u8(0).u8(0).f(&[0.0; 4]);
    for i in 0..2 {
        w.i32(i).f(&[1.0, 0.5]).u8(0).f(&[0.0]);
    }
    w.f(&[0.25]);
    read_ignored_exact(&w.0, "NiBlendFloatInterpolator");

    // Flag 1 (manager-controlled): right after the weight threshold comes the value.
    let mut w = W::default();
    w.u8(1).u8(4).f(&[0.0]).f(&[1.0, 2.0, 3.0]);
    read_ignored_exact(&w.0, "NiBlendPoint3Interpolator");

    let mut w = W::default();
    w.u8(1).u8(0).f(&[0.0]);
    read_ignored_exact(&w.0, "NiBlendTransformInterpolator");
}

#[test]
fn key_data_sizes_follow_the_interpolation_type() {
    // TBC quaternions (4 + 16 + 12), quadratic translations (4 + 3 × 12), no scales.
    let mut w = W::default();
    w.u32(2).u32(3).zeros(2 * 32);
    w.u32(1).u32(2).zeros(4 + 36);
    w.u32(0);
    read_ignored_exact(&w.0, "NiTransformData");

    // XYZ rotation: three key groups, each with its own type.
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
        panic!("node: {block:?}");
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
    // TexDesc: presence flag, source, clamp, filter, UV set, PS2 L and PS2 K in 10.2.0.0, transform
    // flag.
    let mut w = W::default();
    w.u8(1).i32(1).u32(0).u32(2).u32(0).u16(0).u16(0xFFB5).u8(0);
    let mut cur = Cursor::new(&w.0);
    cur.set_version(header::VERSION_10_2_0_0, 9);
    let desc = properties::read_opt_tex_desc(&mut cur).unwrap();
    assert_eq!(cur.remaining(), 0, "the PS2 fields were read");
    assert_eq!(desc.map(|d| d.source), Some(BlockRef(Some(1))));

    // Control: the same bytes as 20.0.0.4 — the PS2 fields are extra, the transform flag is read
    // from them and the cursor does not reach the end.
    let mut cur = Cursor::new(&w.0);
    let _ = properties::read_opt_tex_desc(&mut cur);
    assert_ne!(cur.remaining(), 0);
}

// --- Particles (T2.3b) ----------------------------------------------------------------

/// Like [`read_ignored_exact`], but the file has version `version` and `BSVER` `bsver`.
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
        assert!(!IGNORED_TYPES.contains(name), "{name} is in two lists");
    }
}

#[test]
fn particle_system_is_geometry_with_modifier_list() {
    let mut w = W::default();
    w.av("Fire").i32(5).i32(-1).u8(0); // data, skinning, no shader
    w.u8(1).u32(2).i32(6).i32(7); // world space, two modifiers
    read_ignored_exact(&w.0, "NiParticleSystem");
    read_ignored_exact(&w.0, "NiMeshParticleSystem");
    assert!(is_particle_system("NiParticleSystem"));
    assert!(!is_particle_system("NiPSysData"));
}

/// A `NiGeometryData` for two vertices, without normals, colors and UV.
fn particle_geometry(w: &mut W) -> &mut W {
    w.i32(0).u16(2).u8(0).u8(0);
    w.u8(1).f(&[0.0; 6]); // vertices
    w.u16(0).u8(0); // data flags, no normals
    w.f(&[0.0, 0.0, 0.0, 1.0]).u8(0); // sphere, no colors
    w.u16(0) // consistency
}

#[test]
fn particle_data_20_0_0_4_has_rotation_angles_and_short_particle_info() {
    let mut w = W::default();
    particle_geometry(&mut w).i32(-1); // additional data
    w.u8(1).f(&[1.0, 2.0]); // radii
    w.u16(2); // active ones
    w.u8(0); // no sizes
    w.u8(1).f(&[0.0; 8]); // rotations, two quaternions
    w.u8(1).f(&[0.5, 0.5]); // angles
    w.u8(0); // no axes
    w.zeros(2 * 28); // NiParticleInfo without a rotation axis
    w.u8(1).f(&[3.0, 4.0]); // rotation speeds
    w.u16(0).u16(0);
    read_ignored_exact(&w.0, "NiPSysData");

    // NiMeshPSysData: pool, fill, generations, meshes.
    w.u32(8).u8(1).u32(2).u32(0).u32(1).i32(9);
    read_ignored_exact(&w.0, "NiMeshPSysData");
}

#[test]
fn particle_data_10_2_has_rotation_axis_in_particle_info_and_no_angles() {
    let mut w = W::default();
    particle_geometry(&mut w); // no additional data
    w.u8(0).u16(2).u8(0).u8(0); // no radii, sizes, rotations
    w.zeros(2 * 40); // NiParticleInfo with a rotation axis
    w.u16(0).u16(0); // no rotation speeds at all (since 20.0.0.2)
    read_ignored_exact_at(&w.0, "NiPSysData", header::VERSION_10_2_0_0, 9);
}

/// `NiPSysModifier`: name, order, system, active.
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

/// `NiPSysEmitter`: speed, angles, color, radius, (radius variation), lifetime.
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

// --- Old 10.x (T2.3b) ------------------------------------------------------------------

/// Like [`read_exact`], but the file has version `version` and `BSVER` `bsver`.
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

/// A 10.0.1.0 file: without a user version and the Bethesda header, a `u32` before each block.
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
    w.u16(3); // vertices; no Group ID (since 10.1.0.114) and no flags (since 10.1.0.0)
    w.u8(1).f(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    w.u16(0).u8(0); // data flags, no normals
    w.f(&[0.0, 0.0, 0.0, 1.0]).u8(0); // sphere, no colors
    w.u16(0); // consistency; no Additional Data (since 20.0.0.4)
    w.u16(1).u32(3); // triangles, indices; no `Has Triangles` (since 10.1.0.0)
    w.u16(0).u16(1).u16(2);
    w.u16(0); // no groups
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
    w.u16(1).u16(1).u16(3); // triangles, one strip of three; no `Has Points` (since 10.0.1.3)
    w.u16(0).u16(1).u16(2);
    let d = read_exact_at(
        &w.0,
        header::VERSION_10_0_1_2,
        3,
        geometry::read_tri_strips_data,
    );
    assert_eq!(d.strips, [vec![0, 1, 2]]);

    // 10.1.0.106: the keep and compress flags are present, `Group ID` is not yet.
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
    w.net("tex").u16(0).u32(2).u32(1); // flags (up to 10.0.1.2), apply, one slot
    w.u8(1).i32(4).u32(3).u32(2).u32(0).u16(0).u16(0); // base without a transform
    w.u8(0).u8(0).u8(0).u8(0); // dark, detail, gloss, glow
    w.u32(0); // no shader slots
    let t = read_exact_at(
        &w.0,
        header::VERSION_10_0_1_0,
        0,
        properties::read_texturing_property,
    );
    assert_eq!(t.base.map(|d| d.source), Some(BlockRef(Some(4))));

    let mut w = W::default();
    w.net("t").u8(1).s("textures\\a.dds"); // external: name, no reference
    w.u32(6).u32(1).u32(3).u8(1); // formats, static; no Direct Render (since 10.1.0.103)
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
    w.net("t").u8(1).s("a.dds").i32(-1); // 10.1.0.101: reference present, no Direct Render
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
    w.s("Open").u32(1).u32(1); // name, one block, grow by
    w.i32(3).i32(4).i32(-1).u16(0).u8(26); // interpolator, controller, blend, priority
    for s in ["Door", "", "NiTransformController", "", ""] {
        w.s(s);
    }
    w.f(&[1.0]).i32(-1).u32(0).f(&[1.0, 0.0, 0.0, 2.0]); // weight, keys, cycle, frequency…stop
    w.u8(0).i32(1).s("Door"); // backwards, manager, accumulation root; no palette
    read_ignored_exact_at(&w.0, "NiControllerSequence", header::VERSION_10_1_0_106, 5);
}

#[test]
fn interpolators_and_controllers_10_1_0_106_have_old_fields() {
    // NiTransformInterpolator: NiQuatTransform with three `TRS Valid` flags, data.
    let mut w = W::default();
    w.f(&[0.0; 8]).u8(1).u8(1).u8(1).i32(2);
    read_ignored_exact_at(
        &w.0,
        "NiTransformInterpolator",
        header::VERSION_10_1_0_106,
        5,
    );

    // Blend before 10.1.0.109: `u16` sizes, items of 20 bytes, `i32` priorities.
    let mut w = W::default();
    w.u16(1).u16(1).i32(3).f(&[1.0, 1.0]).i32(0).f(&[0.0]);
    w.u8(0).f(&[0.0]).u8(0).u16(1).u16(0).i32(0).i32(0);
    w.f(&[0.0; 8]).u8(1).u8(1).u8(1); // transform value
    read_ignored_exact_at(
        &w.0,
        "NiBlendTransformInterpolator",
        header::VERSION_10_1_0_106,
        5,
    );

    // NiTransformController: `Manager Controlled` (10.1.0.104 … 10.1.0.108), interpolator.
    let mut w = W::default();
    time_controller(&mut w).u8(1).i32(5);
    read_ignored_exact_at(&w.0, "NiTransformController", header::VERSION_10_1_0_106, 5);

    // Before 10.1.0.104 there is no interpolator, there is a data reference.
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
    w.i32(2).u32(0); // shape, `u32` up to 10.0.1.2
    w.u8(1).u8(0).u16(0); // filter
    w.zeros(20); // bhkWorldObjectCInfo
    w.zeros(4); // bhkEntityCInfo
    w.zeros(4); // unused 04
    w.f(&[1.0, 2.0, 3.0, 0.0]).f(&[0.0, 0.0, 0.0, 1.0]); // translation, rotation
    w.zeros(16 + 16 + 48 + 16); // velocities, tensor, center
    w.f(&[0.0, 0.1, 0.05, 0.3, 0.3]); // mass, damping, friction, restitution
    w.u8(7).u8(1).u8(0).u8(0).zeros(12); // motion…, unused 05
    w.u32(0).u32(0); // no constraints, flags
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
    w.i32(3).zeros(12).f(&[1.0]).u32(2).u8(9).u8(9); // MOPP without an offset (before 10.1.0.0)
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

/// A file made only of animation blocks for `motion::sequences`: the block types and bytes in a
/// row. The second pass does not need the blocks in `SourceNif` — only the header and the offsets.
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

/// T2.6: door animation values are read in a second pass — the node name from the palette, the
/// interpolator pose, the first and last key; the tangents of quadratic keys are skipped.
#[test]
fn motion_reads_the_open_sequence_of_a_door() {
    use motion::{Rotations, sequences};

    let mut seq = W::default();
    seq.s("Open").u32(2).u32(1);
    // interpolator, controller, priority, palette, node name offset and four more
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
    // Two linear rotation keys: time, w, x, y, z.
    data.u32(2).u32(1);
    data.f(&[0.0, 1.0, 0.0, 0.0, 0.0]);
    data.f(&[2.5, 0.66, 0.0, 0.0, -0.75]);
    // Two quadratic translation keys: time, value and two tangents of three numbers each.
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
    assert_eq!(keys.pose_rotation, None, "−MAX means \"not set\"");
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
    assert_eq!(open.tracks[1].keys, None, "not a NiTransformInterpolator");

    // Truncating the file anywhere gives an error or less data, but not a panic.
    for cut in 0..bytes.len() {
        let _ = sequences(&bytes[..cut], &source, &offsets);
    }
}

/// Euler tracks and the 10.1.0.106 layout: the controlled block carries the strings itself, the
/// interpolator has three `TRS Valid` bytes, the sequence has the phase and "play backwards".
#[test]
fn motion_reads_euler_keys_and_the_inline_names_of_10_1() {
    use motion::{Rotations, sequences};

    let mut seq = W::default();
    seq.s("Open").u32(1).u32(1);
    // interpolator, controller, blend, its index, priority, five strings
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
    // XYZ: number of keys (any non-zero), type 4, three groups; Z has two TBC keys.
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

    // Before 10.1.0.106 the controlled block is different: sequences are not read, rather than read
    // wrongly.
    let (bytes, mut old, offsets) =
        motion_file(0x0A01_0065, &[("NiControllerSequence", vec![0; 4])]);
    old.header.version = 0x0A01_0065;
    assert_eq!(sequences(&bytes, &old, &offsets), Ok(Vec::new()));
}

/// Block offsets are exactly the places `read_block` read the block from.
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

/// The common part of a `NiPSysModifier`: name, order, owner, active.
fn psys_modifier<'a>(w: &'a mut W, name: &str, active: bool) -> &'a mut W {
    w.s(name).u32(1).i32(0).u8(u8::from(active))
}

/// A `NiParticleSystem` with controller `controller` and a list of modifiers.
fn particle_system(controller: i32, modifiers: &[i32]) -> Vec<u8> {
    let mut w = W::default();
    w.s("Fire").u32(0).i32(controller);
    w.u16(0x000E)
        .f(&[1.0, 2.0, 3.0])
        .f(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
        .f(&[1.0])
        .u32(0)
        .i32(-1);
    // data, skin, no shader; then "in world" and the modifiers
    w.i32(1).i32(-1).u8(0).u8(1);
    w.u32(u32::try_from(modifiers.len()).unwrap());
    for m in modifiers {
        w.i32(*m);
    }
    w.0
}

/// The start of `NiPSysData` — a `NiGeometryData` with a pool of `pool` vertices without arrays.
fn psys_data(pool: u16) -> Vec<u8> {
    let mut w = W::default();
    w.i32(0).u16(pool).u8(0).u8(0).u8(0).u16(0).u8(0);
    w.f(&[0.0, 0.0, 0.0, 10.0]).u8(0).u16(0).i32(-1);
    w.0
}

fn box_emitter(name: &str) -> Vec<u8> {
    let mut w = W::default();
    psys_modifier(&mut w, name, true);
    // speed and its variation, declination, planar angle with variations
    w.f(&[20.0, 10.0, 0.1, 0.28, 0.2, 3.0]);
    w.f(&[1.0, 0.5, 0.25, 1.0]).f(&[8.0, 0.0]).f(&[1.5, 0.5]);
    w.i32(4).f(&[1.0, 2.0, 3.0]);
    w.0
}

/// `NiPSysEmitterCtlr`: flags, window, interpolator, emitter name.
fn emitter_controller(flags: u16, interpolator: i32, emitter: &str) -> Vec<u8> {
    let mut w = W::default();
    w.i32(-1).u16(flags).f(&[1.0, 0.0, 0.0, 0.95]).i32(0);
    w.i32(interpolator).s(emitter).i32(-1);
    w.0
}

/// T2.7: a particle system is read with its values — the emitter, the birth rate from its own
/// interpolator, the pool, the modifiers; a disabled modifier stays disabled.
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
    assert!(!s.modifiers[6].active, "drag is disabled");

    // Truncating the file anywhere gives an error or less data, but not a panic.
    for cut in 0..bytes.len() {
        let _ = systems(&bytes[..cut], &source, &offsets);
    }
}

/// A controller without its own interpolator: the rate and window come from the manager's sequence.
/// A looped `Idle` is a constant effect; only `Forward` or a clamped `Idle` is an event effect.
#[test]
fn psys_takes_the_birth_rate_from_an_ambient_sequence_or_marks_the_system_triggered() {
    use psys::systems;

    let sequence = |name: &str, cycle: u32, interpolator: i32| {
        let mut w = W::default();
        w.s(name).u32(1).u32(1);
        // interpolator, controller (block 3), priority, palette, five offsets
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
            // Manager-controlled: no interpolator of its own.
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

    // `Forward` comes first in the file, but the world plays `Idle` by itself.
    let (bytes, source, offsets) = file(vec![
        ("NiControllerSequence", sequence("Forward", 2, 7)),
        ("NiControllerSequence", sequence("idle", 0, 5)),
    ]);
    let found = systems(&bytes, &source, &offsets).unwrap();
    let e = found[0].emission.as_ref().unwrap();
    assert_eq!(e.sequence.as_deref(), Some("idle"));
    assert!(!e.triggered && !e.once);
    assert_eq!(
        (e.start, e.stop),
        (0.5, 4.5),
        "the window belongs to the sequence"
    );
    assert_eq!(e.mean_rate(), Some(10.0), "(6 + 24) / 3");

    // An action only: the particles are an event effect.
    let (bytes, source, offsets) = file(vec![("NiControllerSequence", sequence("Forward", 2, 7))]);
    let found = systems(&bytes, &source, &offsets).unwrap();
    let e = found[0].emission.as_ref().unwrap();
    assert_eq!(e.sequence.as_deref(), Some("Forward"));
    assert!(e.triggered && e.once);
    assert_eq!(e.mean_rate(), Some(40.0));

    // A clamped `SpecialIdle` is a one-off burst, also an event effect.
    let (bytes, source, offsets) = file(vec![(
        "NiControllerSequence",
        sequence("SpecialIdle", 2, 5),
    )]);
    let found = systems(&bytes, &source, &offsets).unwrap();
    assert!(found[0].emission.as_ref().unwrap().triggered);

    // Control: no sequences at all — no rate, but "on event" cannot be claimed either.
    let (bytes, source, offsets) = file(Vec::new());
    let found = systems(&bytes, &source, &offsets).unwrap();
    let e = found[0].emission.as_ref().unwrap();
    assert_eq!(e.mean_rate(), None);
    assert!(!e.triggered);
}

/// T11.5b: B-spline blocks and skeleton levels of detail are read to the end.
#[test]
fn spline_blocks_and_bone_lod_are_read_to_the_end() {
    // Start, stop, data, basis; pose; three control point offsets.
    let mut w = W::default();
    w.f(&[0.0, 2.0]).i32(3).i32(4);
    w.f(&[0.0; 3]).f(&[1.0, 0.0, 0.0, 0.0]).f(&[1.0]);
    w.u32(0).u32(12).u32(0xFFFF);
    read_ignored_exact(&w.0, "NiBSplineTransformInterpolator");
    // Compressed — six more numbers: offset and multiplier of translation, rotation, scale.
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

    // Two `f32` and three `i16`.
    let mut w = W::default();
    w.u32(2).f(&[1.0, 2.0]).u32(3).u16(1).u16(2).u16(3);
    read_ignored_exact(&w.0, "NiBSplineData");

    let mut w = W::default();
    w.u32(7);
    read_ignored_exact(&w.0, "NiBSplineBasisData");

    // Two levels: three bones and one.
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

/// T11.5b: a creature clip — text keys, the accumulation root, a track on a compressed spline and a
/// track on keys with scale.
#[test]
fn clip_reads_marks_accum_root_keys_and_compact_splines() {
    use clip::{Channel, Rotation, clips};

    let mut seq = W::default();
    seq.s("Forward").u32(2).u32(1);
    // interpolator, controller, priority, palette, node name offset and four more
    for (interpolator, name_at) in [(2, 0), (5, 6)] {
        seq.i32(interpolator).i32(-1).u8(26).i32(1).u32(name_at);
        seq.u32(17).u32(17).u32(17).u32(17);
    }
    // weight, text keys, cycle, frequency, start, stop, manager, accumulation root, palette
    seq.f(&[1.0])
        .i32(6)
        .u32(0)
        .f(&[1.0, 0.0, 2.0])
        .i32(-1)
        .s("Bip01")
        .i32(1);

    let mut palette = W::default();
    palette.s("Bip01\0Bip01 Tail\0").u32(17);

    // Spline: translation from number zero, no rotation and scale; translation = i16 / 32767 × 50 + 100.
    //
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
    assert_eq!(motion.pose_rotation, None, "−MAX means \"not set\"");
    assert_eq!(motion.rotation, Rotation::None);
    assert_eq!(motion.scale, Channel::None);
    let Channel::Spline(travel) = &motion.translation else {
        panic!("translation is a spline: {:?}", motion.translation);
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

    // A control point offset past the end of the data is an error, not a read out of bounds.
    let mut bad = bytes.clone();
    let handle_at = offsets[2] + 16 + 32;
    bad[handle_at..handle_at + 4].copy_from_slice(&1u32.to_le_bytes());
    assert!(matches!(
        clips(&bad, &source, &offsets),
        Err(Error::UnknownValue { value: 1, .. })
    ));

    // Truncating the file anywhere gives an error or less data, but not a panic.
    for cut in 0..bytes.len() {
        let _ = clips(&bytes[..cut], &source, &offsets);
    }
}

/// A cubic B-spline clamped at the ends: at the edges — the edge control points, four points — a
/// Bézier curve, between points on a line — a point on the same line.
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
    // The middle of a Bézier curve: (P0 + 3·P1 + 3·P2 + P3) / 8.
    let mid = bezier.at(2.0).unwrap()[0];
    assert!((mid - 16.25).abs() < 1e-4, "{mid}");
    // Outside the range — the value at the edge.
    assert_eq!(bezier.at(-5.0), Some([0.0]));
    assert_eq!(bezier.at(9.0), Some([40.0]));

    // Seven points on the line y = 2x: the curve follows the same line and does not turn back.
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
        assert!(x > before, "{x} after {before}");
        before = x;
    }
    assert_eq!(line.at(1.0), Some([6.0, 12.0]));

    // All points are equal — the curve stands still.
    let still = Spline {
        start: 0.0,
        stop: 2.0,
        points: vec![[3.0, 4.0]; 9],
    };
    for step in 0..=8u8 {
        let v = still.at(f32::from(step) / 4.0).unwrap();
        assert!((v[0] - 3.0).abs() < 1e-5 && (v[1] - 4.0).abs() < 1e-5);
    }

    // Fewer than four points — a lower degree: two points give a segment, one a constant, none
    // nothing.
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
    // A zero-length range gives the start, not a division by zero.
    let instant = Spline {
        start: 2.0,
        stop: 2.0,
        points: vec![[1.0], [2.0], [3.0], [4.0]],
    };
    assert_eq!(instant.at(2.0), Some([1.0]));
}
