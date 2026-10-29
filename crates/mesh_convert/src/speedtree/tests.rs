//! Synthetic `.spt`: the bytes are assembled by hand from the tag table (`docs/en/formats/spt.md`).

use super::*;

/// A tag stream writer.
#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn tag(&mut self, tag: u32) -> &mut Self {
        self.0.extend_from_slice(&tag.to_le_bytes());
        self
    }
    fn int(&mut self, tag: u32, v: i32) -> &mut Self {
        self.tag(tag);
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn float(&mut self, tag: u32, v: f32) -> &mut Self {
        self.tag(tag);
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn floats(&mut self, tag: u32, vs: &[f32]) -> &mut Self {
        self.tag(tag);
        for v in vs {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        self
    }
    fn byte(&mut self, tag: u32, v: u8) -> &mut Self {
        self.tag(tag);
        self.0.push(v);
        self
    }
    fn str(&mut self, tag: u32, s: &str) -> &mut Self {
        self.tag(tag);
        self.0
            .extend_from_slice(&u32::try_from(s.len()).unwrap().to_le_bytes());
        self.0.extend_from_slice(s.as_bytes());
        self
    }
}

const FLAT: &str = "BezierSpline 0 1 0\n{\n\n\t2\n\t0 1 0.7 -0.7 0.08\n\t1 1 0.7 0.7 0.1\n\n}\n";

/// A level with recognizable values: the number of segments around is `cross`.
fn level(w: &mut W, cross: i32) {
    w.tag(1016);
    for tag in 6000..=6007 {
        w.str(tag, FLAT);
    }
    w.int(6008, cross).int(6009, 7);
    w.float(6010, 0.25).float(6011, 0.75).float(6012, 15.0);
    w.float(6013, 2.0)
        .float(6014, 8.0)
        .byte(6015, 1)
        .byte(6016, 0);
    w.str(6017, FLAT);
    w.tag(1017);
}

fn tree_bytes() -> Vec<u8> {
    let mut w = W::default();
    w.str(1000, MAGIC).tag(1002);
    w.str(2000, "C:\\Bark.tga")
        .int(2005, 301_409)
        .float(2006, 180.0);
    w.float(2007, 10.0).int(1014, 2);
    level(&mut w, 20);
    level(&mut w, 6);
    w.tag(1003).tag(1004).float(3007, 0.5).int(3008, 2);
    w.int(1006, 1).tag(1007);
    w.byte(4000, 0)
        .floats(4001, &[1.0, 1.0, 1.0])
        .float(4002, 0.2);
    w.str(4003, "Leaves.tga").floats(4004, &[0.5, 0.7, 0.0]);
    w.floats(4005, &[0.06, 0.06, 0.0])
        .floats(4006, &[9.6, 9.6, 0.0]);
    w.float(4007, 1.0).tag(1008).tag(1005).tag(1001);
    w.tag(12000).floats(12002, &[0.0, 0.0, 5.0, 2.0]);
    w.floats(12003, &[0.0, 0.0, 1.0, 0.5, 10.0]).tag(12001);
    w.tag(15000);
    w.byte(15002, 1)
        .float(15003, 0.0)
        .byte(15002, 0)
        .float(15003, 2.0);
    w.tag(15001).tag(16000);
    for pack in [2.0, 1.0] {
        w.float(16002, pack).int(16003, 3);
        for tag in 16004..=16012 {
            w.float(tag, 0.5);
        }
    }
    w.tag(16001).int(16013, 444);
    w.0
}

#[test]
fn tokens_read_every_value_type_to_the_end() {
    let mut w = W::default();
    w.str(1000, MAGIC).tag(1002).byte(2002, 0).int(2005, 7);
    w.float(2006, 180.0)
        .floats(12004, &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    w.tag(10002);
    w.0.extend_from_slice(&1u32.to_le_bytes());
    for v in 0..8u8 {
        w.0.extend_from_slice(&f32::from(v).to_le_bytes());
    }
    let t = tokens(&w.0).unwrap();
    let values: Vec<_> = t.iter().map(|t| t.value.clone()).collect();
    assert_eq!(
        values,
        [
            Value::Str(MAGIC.to_owned()),
            Value::Marker,
            Value::Byte(0),
            Value::Int(7),
            Value::Float(180.0),
            Value::Floats(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]),
            Value::Quads(vec![[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]]),
        ]
    );
    assert_eq!(t[2].at, 4 + 4 + MAGIC.len() + 4);
}

#[test]
fn unknown_tag_truncation_and_missing_magic_are_errors() {
    let mut w = W::default();
    w.str(1000, MAGIC).tag(99_999);
    assert_eq!(
        tokens(&w.0),
        Err(SptError::UnknownTag {
            at: 4 + 4 + MAGIC.len(),
            tag: 99_999
        })
    );

    // Truncation in the middle of a value is an error; a stream without an error is possible only
    // when truncated exactly at the start of a tag (the format has no sizes, such truncation cannot
    // be told apart).
    let bytes = tree_bytes();
    let starts: Vec<usize> = tokens(&bytes).unwrap().iter().map(|t| t.at).collect();
    for cut in 1..bytes.len() {
        if tokens(&bytes[..cut]).is_ok() {
            assert!(starts.contains(&cut), "truncation to {cut} bytes passed");
        }
    }

    let mut w = W::default();
    w.int(2005, 1);
    assert_eq!(tokens(&w.0), Err(SptError::NotSpt));
    assert_eq!(tokens(&[]), Err(SptError::NotSpt));
}

#[test]
fn spline_text_is_parsed_with_all_points() {
    let s = BezierSpline::parse(FLAT).unwrap();
    assert_eq!((s.min, s.max, s.variance), (0.0, 1.0, 0.0));
    assert_eq!(
        s.points,
        [[0.0, 1.0, 0.7, -0.7, 0.08], [1.0, 1.0, 0.7, 0.7, 0.1]]
    );
    // An exponent with three digits — the editor writes it that way (`-6.24607e-008`).
    let s = BezierSpline::parse("BezierSpline 0 1 0 { 1 1 -6.24607e-008 0 1 0.3 }").unwrap();
    assert!(s.points[0][1] < 0.0);
    assert!(BezierSpline::parse("BezierSpline 0 1 0 { 2 0 1 0 1 0.1 }").is_none());
    assert!(BezierSpline::parse("Spline 0 1 0 { 0 }").is_none());
}

#[test]
fn model_collects_levels_leaf_maps_collision_and_per_level_sets() {
    let tree = parse(&tree_bytes()).unwrap();
    assert_eq!(tree.bark_texture, "C:\\Bark.tga");
    assert_eq!(
        (tree.seed, tree.size, tree.size_variance),
        (301_409, 180.0, 10.0)
    );
    assert_eq!(tree.levels.len(), 2);
    let [trunk, branch] = &tree.levels[..] else {
        unreachable!()
    };
    assert_eq!((trunk.cross_segments, branch.cross_segments), (20, 6));
    assert_eq!(trunk.child_frequency, 15.0);
    assert!(trunk.u_absolute && !trunk.v_absolute);
    assert!(trunk.random_v_offset && !branch.random_v_offset);
    assert_eq!((trunk.twist, branch.twist), (0.0, 2.0));
    assert_eq!(
        (trunk.flares.segment_pack, branch.flares.segment_pack),
        (2.0, 1.0)
    );
    assert_eq!(trunk.flares.count, 3);
    assert_eq!(tree.flare_seed, 444);
    assert_eq!(tree.leaf_collision, 2);
    assert_eq!(tree.leaf_maps.len(), 1);
    assert_eq!(tree.leaf_maps[0].size, [0.06, 0.06, 0.0]);
    assert_eq!(tree.leaf_maps[0].texture, "Leaves.tga");
    assert_eq!(
        tree.collision,
        [
            Collision::Sphere {
                center: [0.0, 0.0, 5.0],
                radius: 2.0
            },
            Collision::Capsule {
                center: [0.0, 0.0, 1.0],
                radius: 0.5,
                length: 10.0
            },
        ]
    );
}

#[test]
fn level_count_must_match_the_declaration_and_per_level_sets() {
    let mut bytes = tree_bytes();
    // 1014 = 2 → 3: fewer levels than declared.
    let pos = bytes
        .windows(8)
        .position(|w| w == [0xf6, 0x03, 0, 0, 2, 0, 0, 0])
        .unwrap();
    bytes[pos + 4] = 3;
    assert!(matches!(parse(&bytes), Err(SptError::Structure { .. })));

    let mut w = W::default();
    w.str(1000, MAGIC).tag(1016).str(6000, "not a spline");
    assert!(matches!(parse(&w.0), Err(SptError::BadSpline { .. })));

    let mut w = W::default();
    w.str(1000, MAGIC).tag(1016).int(2005, 1);
    assert!(matches!(parse(&w.0), Err(SptError::Structure { .. })));
}

// --- RNG and spline (T2.4b) -----------------------------------------------------------

use super::rng::Rng;
use super::spline::Spline;

#[test]
fn rng_core_is_park_miller_and_draws_come_through_the_shuffle_table() {
    // Park–Miller with seed 1: 16807, 282475249, 1622650073, … — the first 128 go into the table.
    let mut a = 1i32;
    let mut raw = Vec::new();
    for _ in 0..131 {
        a = i32::try_from(i64::from(a) * 16_807 % 2_147_483_647).unwrap();
        raw.push(a);
    }
    assert_eq!(&raw[..3], &[16_807, 282_475_249, 1_622_650_073]);
    let unit = |v: i32| {
        #[expect(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let out = (f64::from(v as f32) * 4.656_612_873_077_393e-10) as f32;
        out
    };
    let mut rng = Rng::new(1);
    // The first output: the index from the 129th core value, the table cell is returned.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = (f64::from(unit(raw[128])) * 128.0) as usize;
    assert_eq!(rng.random01(), unit(raw[index]));
    // The second: the index from the 131st, cell `index` now holds the 130th value.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let second = (f64::from(unit(raw[130])) * 128.0) as usize;
    let expected = if second == index {
        unit(raw[129])
    } else {
        unit(raw[second])
    };
    assert_eq!(rng.random01(), expected);

    // A seed below 1 acts as 1; reseeding reproduces the stream.
    let mut zero = Rng::new(0);
    let mut one = Rng::new(1);
    let first: Vec<f32> = (0..50).map(|_| one.random01()).collect();
    assert_eq!((0..50).map(|_| zero.random01()).collect::<Vec<_>>(), first);
    one.seed(1);
    assert_eq!((0..50).map(|_| one.random01()).collect::<Vec<_>>(), first);

    let mut r = Rng::new(301_409);
    for _ in 0..1000 {
        let v = r.range(-2.0, 3.0);
        assert!((-2.0..3.0).contains(&v), "{v}");
    }
}

/// The line `y = x`: tangents along the diagonal, the length is a third of the segment.
const LINE: &str = "BezierSpline 10 20 0.5 { 2 0 0 1 1 0.4714 1 1 1 1 0.4714 }";

#[test]
fn spline_table_follows_the_curve_and_scales_to_min_max() {
    let s = Spline::new(&BezierSpline::parse(LINE).unwrap());
    for x in [0.0f32, 0.1, 0.25, 0.5, 0.9] {
        assert!((s.curve(x) - x).abs() < 0.005, "{x}: {}", s.curve(x));
    }
    // The last entry is the curve point at t = 499/500, not the end.
    assert!((s.curve(1.0) - 0.998).abs() < 1e-3);
    assert!((s.value(0.5) - 15.0).abs() < 0.05);
    // Outside 0…1 — a clamp, not reading past the table.
    assert_eq!(s.curve(-3.0), s.curve(0.0));
    assert_eq!(s.curve(7.0), s.curve(1.0));
}

#[test]
fn evaluation_always_draws_from_the_generator() {
    let flat = Spline::new(&BezierSpline::parse(FLAT).unwrap());
    let mut a = Rng::new(5);
    let mut b = Rng::new(5);
    // Zero variation — a number is still drawn, the stream shifts.
    assert_eq!(flat.evaluate(0.3, &mut a), flat.value(0.3));
    b.random01();
    assert_eq!(a.random01(), b.random01());

    let line = Spline::new(&BezierSpline::parse(LINE).unwrap());
    let mut rng = Rng::new(9);
    let mut probe = Rng::new(9);
    let v = line.evaluate(0.5, &mut rng);
    assert_eq!(v, line.value(0.5) + probe.range(-0.5, 0.5));
}

#[test]
fn a_single_point_spline_is_nan_like_the_engine_and_zero_tangents_stay_zero() {
    // In the engine all curve points are zero, and table interpolation divides 0 by 0. The game's
    // `.spt` files have no such splines (`real_speedtree.rs`).
    let one = Spline::new(&BezierSpline::parse("BezierSpline 2 4 0 { 1 0 0.5 1 0 0.1 }").unwrap());
    assert!(one.value(0.5).is_nan());
    let s =
        Spline::new(&BezierSpline::parse("BezierSpline 0 1 0 { 2 0 0 0 0 0 1 1 0 0 0 }").unwrap());
    assert!((s.curve(0.5) - 0.5).abs() < 0.01);
}
