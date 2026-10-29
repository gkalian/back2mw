//! Tree model from an `.spt` tag stream: global parameters, levels (trunk, branches, foliage
//! level), foliage maps, collision objects. The meaning of the tags is in `docs/en/formats/spt.md`.
//!
//! Only what the generator and the mesh build need is taken; the rest (LOD, lighting, wind, fronds,
//! shadows) is read by the tokenizer and skipped here.

use super::{SptError, Token, Value, tokens};

/// A SpeedTree Bézier spline: a parameter's value along a branch. `min`…`max` is the range the
/// curve's `y` is mapped into, `variance` the variation, `points` the control points
/// `[x, y, dx, dy, tangent length]`.
#[derive(Debug, Clone, PartialEq)]
pub struct BezierSpline {
    pub min: f32,
    pub max: f32,
    pub variance: f32,
    pub points: Vec<[f32; 5]>,
}

impl BezierSpline {
    /// Parses the text `BezierSpline min max var { N  x y dx dy len … }`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut words = text.split_whitespace();
        if words.next()? != "BezierSpline" {
            return None;
        }
        let mut num = || words.next()?.parse::<f32>().ok();
        let (min, max, variance) = (num()?, num()?, num()?);
        let mut words = text.split_whitespace().skip(4);
        if words.next()? != "{" {
            return None;
        }
        let count: usize = words.next()?.parse().ok()?;
        let mut points = Vec::with_capacity(count.min(64));
        for _ in 0..count {
            let mut p = [0.0f32; 5];
            for slot in &mut p {
                *slot = words.next()?.parse().ok()?;
            }
            points.push(p);
        }
        (words.next()? == "}" && words.next().is_none()).then_some(Self {
            min,
            max,
            variance,
            points,
        })
    }
}

/// The level's root bulges (`16003`…`16012`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Flares {
    /// `16002`: segment packing along the length (exponent).
    pub segment_pack: f32,
    pub count: i32,
    pub balance: f32,
    pub radial_influence: f32,
    pub radial_influence_variance: f32,
    pub radial_exponent: f32,
    pub radial_distance: f32,
    pub radial_distance_variance: f32,
    pub length_distance: f32,
    pub length_distance_variance: f32,
    pub length_exponent: f32,
}

/// A level: the trunk, branches of the next order, or the last one — the foliage level (some fields
/// mean something else there, `docs/en/formats/spt.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    /// `6000`…`6007`, `6017`: disturbance, gravity, flexibility, flexibility profile, length,
    /// radius, radius profile, start angle, gravity profile.
    pub disturbance: BezierSpline,
    pub gravity: BezierSpline,
    pub flexibility: BezierSpline,
    pub flexibility_profile: BezierSpline,
    pub length: BezierSpline,
    pub radius: BezierSpline,
    pub radius_profile: BezierSpline,
    pub start_angle: BezierSpline,
    pub gravity_profile: BezierSpline,
    pub cross_segments: i32,
    pub length_segments: i32,
    /// `6010`…`6012`: where on this level children are born and how often.
    pub child_first: f32,
    pub child_last: f32,
    pub child_frequency: f32,
    pub u_tile: f32,
    pub v_tile: f32,
    pub u_absolute: bool,
    pub v_absolute: bool,
    /// `15002`, `15003`.
    pub random_v_offset: bool,
    pub twist: f32,
    pub flares: Flares,
}

/// A foliage map (`1007`…`1008`).
#[derive(Debug, Clone, PartialEq)]
pub struct LeafMap {
    pub blossom: bool,
    pub color: [f32; 3],
    pub orientation_variance: f32,
    /// The path as the editor wrote it (`C:\…\Leaves.tga`); the game takes the texture from the
    /// `TREE` record.
    pub texture: String,
    pub origin: [f32; 3],
    /// `4005`: card size / 100.
    pub size: [f32; 3],
    /// `4006`: the size without division — derived from `4005` (`docs/en/formats/spt.md`).
    pub size_raw: [f32; 3],
    pub f4007: f32,
}

/// A collision object (`12002`…`12004`), in SpeedTree units.
#[derive(Debug, Clone, PartialEq)]
pub enum Collision {
    Sphere {
        center: [f32; 3],
        radius: f32,
    },
    Capsule {
        center: [f32; 3],
        radius: f32,
        length: f32,
    },
    Box {
        min: [f32; 3],
        max: [f32; 3],
    },
}

/// The whole tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    pub bark_texture: String,
    /// `2005`: the main seed (0 means random).
    pub seed: i32,
    /// `2006`, `2007`: size in SpeedTree feet and its variation.
    pub size: f32,
    pub size_variance: f32,
    /// `16013`: the bulge seed.
    pub flare_seed: i32,
    pub levels: Vec<Level>,
    /// `3007`, `3008`: foliage placement tolerance and collision mode (0 none, 1 branch, 2 tree).
    pub leaf_tolerance: f32,
    pub leaf_collision: i32,
    pub leaf_maps: Vec<LeafMap>,
    pub collision: Vec<Collision>,
    /// `8003`, `8005`: bark and foliage materials — diffuse, ambient, specular, emissive (3 each),
    /// shininess.
    pub bark_material: Vec<f32>,
    pub leaf_material: Vec<f32>,
    /// `10002`: foliage map rectangles in the texture atlas — one per map, four corners `(u, v)` in
    /// TGA axes (v up). Empty means each map has the whole texture.
    pub leaf_quads: Vec<[f32; 8]>,
    /// Blossom rules: `3000` position threshold, `3001` depth, `3002` weight.
    pub blossom_distance: f32,
    pub blossom_depth: i32,
    pub blossom_weight: f32,
}

fn structure(t: &Token, what: &str) -> SptError {
    SptError::Structure {
        at: t.at,
        what: lang::tr!("тег {}: {what}", "tag {}: {what}", t.tag),
    }
}

fn float(t: &Token) -> Result<f32, SptError> {
    match t.value {
        Value::Float(v) => Ok(v),
        _ => Err(structure(t, lang::tr("ожидалось f32", "expected an f32"))),
    }
}

fn int(t: &Token) -> Result<i32, SptError> {
    match t.value {
        Value::Int(v) => Ok(v),
        _ => Err(structure(
            t,
            lang::tr("ожидалось целое", "expected an integer"),
        )),
    }
}

fn byte(t: &Token) -> Result<bool, SptError> {
    match t.value {
        Value::Byte(v) => Ok(v != 0),
        _ => Err(structure(t, lang::tr("ожидался байт", "expected a byte"))),
    }
}

fn floats<const N: usize>(t: &Token) -> Result<[f32; N], SptError> {
    match &t.value {
        Value::Floats(v) if v.len() >= N => Ok(v[..N].try_into().expect("the length is checked")),
        _ => Err(structure(
            t,
            lang::tr("ожидались f32", "expected f32 values"),
        )),
    }
}

fn string(t: &Token) -> Result<String, SptError> {
    match &t.value {
        Value::Str(s) => Ok(s.clone()),
        _ => Err(structure(
            t,
            lang::tr("ожидалась строка", "expected a string"),
        )),
    }
}

fn spline(t: &Token) -> Result<BezierSpline, SptError> {
    let text = string(t)?;
    BezierSpline::parse(&text).ok_or(SptError::BadSpline { at: t.at, text })
}

/// A level being assembled: splines by slots 0…8 (`6000`…`6007`, `6017`).
#[derive(Default)]
struct LevelBuilder {
    splines: [Option<BezierSpline>; 9],
    ints: [i32; 2],
    floats: [f32; 5],
    abs: [bool; 2],
}

impl LevelBuilder {
    fn finish(self, end: &Token) -> Result<Level, SptError> {
        let [d, g, f, fp, l, r, rp, sa, gp] = self.splines;
        let missing = || {
            structure(
                end,
                lang::tr(
                    "у уровня нет одного из сплайнов 6000…6007, 6017",
                    "the level lacks one of the splines 6000…6007, 6017",
                ),
            )
        };
        Ok(Level {
            disturbance: d.ok_or_else(missing)?,
            gravity: g.ok_or_else(missing)?,
            flexibility: f.ok_or_else(missing)?,
            flexibility_profile: fp.ok_or_else(missing)?,
            length: l.ok_or_else(missing)?,
            radius: r.ok_or_else(missing)?,
            radius_profile: rp.ok_or_else(missing)?,
            start_angle: sa.ok_or_else(missing)?,
            gravity_profile: gp.ok_or_else(missing)?,
            cross_segments: self.ints[0],
            length_segments: self.ints[1],
            child_first: self.floats[0],
            child_last: self.floats[1],
            child_frequency: self.floats[2],
            u_tile: self.floats[3],
            v_tile: self.floats[4],
            u_absolute: self.abs[0],
            v_absolute: self.abs[1],
            random_v_offset: false,
            twist: 0.0,
            flares: Flares::default(),
        })
    }
}

/// Parses an `.spt` into a [`Tree`].
pub fn parse(data: &[u8]) -> Result<Tree, SptError> {
    let tokens = tokens(data)?;
    let mut tree = Tree {
        bark_texture: String::new(),
        seed: 0,
        size: 0.0,
        size_variance: 0.0,
        flare_seed: 0,
        levels: Vec::new(),
        leaf_tolerance: 0.0,
        leaf_collision: 0,
        leaf_maps: Vec::new(),
        collision: Vec::new(),
        bark_material: Vec::new(),
        leaf_material: Vec::new(),
        leaf_quads: Vec::new(),
        blossom_distance: 0.0,
        blossom_depth: 0,
        blossom_weight: 0.0,
    };
    let mut declared_levels = None;
    let mut level: Option<LevelBuilder> = None;
    let mut leaf: Option<LeafMap> = None;
    // 15002/15003 and 16002…16012 come in sets of one per level, in level order.
    let mut tiling: Vec<(bool, f32)> = Vec::new();
    let mut flares: Vec<Flares> = Vec::new();

    for t in &tokens {
        if let Some(b) = level.as_mut() {
            match t.tag {
                6000..=6007 => {
                    b.splines[(t.tag - 6000) as usize] = Some(spline(t)?);
                    continue;
                }
                6017 => {
                    b.splines[8] = Some(spline(t)?);
                    continue;
                }
                6008 | 6009 => {
                    b.ints[(t.tag - 6008) as usize] = int(t)?;
                    continue;
                }
                6010..=6014 => {
                    b.floats[(t.tag - 6010) as usize] = float(t)?;
                    continue;
                }
                6015 | 6016 => {
                    b.abs[(t.tag - 6015) as usize] = byte(t)?;
                    continue;
                }
                1017 => {
                    let b = level.take().expect("a level is open");
                    tree.levels.push(b.finish(t)?);
                    continue;
                }
                _ => {
                    return Err(structure(
                        t,
                        lang::tr(
                            "не на своём месте внутри уровня",
                            "out of place inside a level",
                        ),
                    ));
                }
            }
        }
        if let Some(m) = leaf.as_mut() {
            match t.tag {
                4000 => m.blossom = byte(t)?,
                4001 => m.color = floats(t)?,
                4002 => m.orientation_variance = float(t)?,
                4003 => m.texture = string(t)?,
                4004 => m.origin = floats(t)?,
                4005 => m.size = floats(t)?,
                4006 => m.size_raw = floats(t)?,
                4007 => m.f4007 = float(t)?,
                1008 => tree.leaf_maps.push(leaf.take().expect("a map is open")),
                _ => {
                    return Err(structure(
                        t,
                        lang::tr(
                            "не на своём месте внутри карты листвы",
                            "out of place inside the leaf map",
                        ),
                    ));
                }
            }
            continue;
        }
        match t.tag {
            2000 => tree.bark_texture = string(t)?,
            2005 => tree.seed = int(t)?,
            2006 => tree.size = float(t)?,
            2007 => tree.size_variance = float(t)?,
            1014 => declared_levels = Some(int(t)?),
            1016 => level = Some(LevelBuilder::default()),
            1007 => {
                leaf = Some(LeafMap {
                    blossom: false,
                    color: [1.0; 3],
                    orientation_variance: 0.0,
                    texture: String::new(),
                    origin: [0.0; 3],
                    size: [0.0; 3],
                    size_raw: [0.0; 3],
                    f4007: 0.0,
                });
            }
            3000 => tree.blossom_distance = float(t)?,
            3001 => tree.blossom_depth = int(t)?,
            3002 => tree.blossom_weight = float(t)?,
            3007 => tree.leaf_tolerance = float(t)?,
            3008 => tree.leaf_collision = int(t)?,
            8003 => tree.bark_material = floats::<13>(t)?.to_vec(),
            8005 => tree.leaf_material = floats::<13>(t)?.to_vec(),
            10002 => match &t.value {
                Value::Quads(q) => tree.leaf_quads.clone_from(q),
                _ => {
                    return Err(structure(
                        t,
                        lang::tr("ожидались прямоугольники", "expected rectangles"),
                    ));
                }
            },
            12002 => {
                let [x, y, z, radius] = floats(t)?;
                tree.collision.push(Collision::Sphere {
                    center: [x, y, z],
                    radius,
                });
            }
            12003 => {
                let [x, y, z, radius, length] = floats(t)?;
                tree.collision.push(Collision::Capsule {
                    center: [x, y, z],
                    radius,
                    length,
                });
            }
            12004 => {
                let [a, b, c, d, e, f] = floats(t)?;
                tree.collision.push(Collision::Box {
                    min: [a, b, c],
                    max: [d, e, f],
                });
            }
            15002 => tiling.push((byte(t)?, 0.0)),
            15003 => {
                tiling
                    .last_mut()
                    .ok_or_else(|| {
                        structure(t, lang::tr("15003 без 15002", "15003 without 15002"))
                    })?
                    .1 = float(t)?;
            }
            16002 => flares.push(Flares {
                segment_pack: float(t)?,
                ..Flares::default()
            }),
            16003..=16012 => {
                let f = flares.last_mut().ok_or_else(|| {
                    structure(
                        t,
                        lang::tr(
                            "параметр вздутия без 16002",
                            "flare parameter without 16002",
                        ),
                    )
                })?;
                match t.tag {
                    16003 => f.count = int(t)?,
                    16004 => f.balance = float(t)?,
                    16005 => f.radial_influence = float(t)?,
                    16006 => f.radial_influence_variance = float(t)?,
                    16007 => f.radial_exponent = float(t)?,
                    16008 => f.radial_distance = float(t)?,
                    16009 => f.radial_distance_variance = float(t)?,
                    16010 => f.length_distance = float(t)?,
                    16011 => f.length_distance_variance = float(t)?,
                    _ => f.length_exponent = float(t)?,
                }
            }
            16013 => tree.flare_seed = int(t)?,
            _ => {}
        }
    }

    let end = tokens
        .last()
        .expect("the tokenizer does not return an empty stream");
    if level.is_some() || leaf.is_some() {
        return Err(structure(
            end,
            lang::tr(
                "файл кончился внутри блока",
                "the file ended inside a block",
            ),
        ));
    }
    if declared_levels != Some(i32::try_from(tree.levels.len()).unwrap_or(i32::MAX)) {
        return Err(structure(
            end,
            &lang::tr!(
                "уровней {} при заявленных {declared_levels:?}",
                "{} levels with {declared_levels:?} declared",
                tree.levels.len()
            ),
        ));
    }
    if tiling.len() != tree.levels.len() || flares.len() != tree.levels.len() {
        return Err(structure(
            end,
            &lang::tr!(
                "наборов 15002 — {}, 16002 — {}, а уровней {}",
                "15002 sets: {}, 16002 sets: {}, levels: {}",
                tiling.len(),
                flares.len(),
                tree.levels.len()
            ),
        ));
    }
    for ((level, (random, twist)), flare) in tree.levels.iter_mut().zip(tiling).zip(flares) {
        level.random_v_offset = random;
        level.twist = twist;
        level.flares = flare;
    }
    Ok(tree)
}
