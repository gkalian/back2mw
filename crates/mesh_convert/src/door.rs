//! Doors (T2.6): what the `Open` sequence of an Oblivion mesh moves, and how.
//!
//! In Oblivion a door opens by an animation in the mesh: `Open` rotates (or slides) the leaf's
//! **node**, often offset from the mesh origin. In Morrowind there is no motion in the mesh: the
//! engine rotates the reference itself by 90° around the vertical, the axis being the mesh origin
//! (`docs/en/formats/tes3.md`, "Doors"). So Morrowind will rotate an Oblivion door in the file's
//! pose around someone else's point, and a frame in the same mesh along with the leaf.
//!
//! Here we measure: [`door_motion`] finds the leaves from the `Open` keys (`crate::read::motion`),
//! their hinges, axis and angle in the mesh frame, and counts how much geometry stays in place. The
//! conversion rule is [`DoorMotion::hinge`]: one leaf on a vertical hinge — the hinge can be moved
//! to the origin; the rest (two leaves, lifting, a frame) Morrowind cannot express without a
//! decision about the record structure.

use std::collections::{BTreeSet, HashMap};

use tes3::nif::NiNode;
use tes3::nif::glam::{Affine3A, Mat3A, Quat, Vec3, Vec3A};

use crate::geometry::Converted;
use crate::read::motion::{Rotations, Sequence, TransformKeys};
use crate::read::{Block, SourceNif};
use crate::skin::av_affine;

/// Name suffix of the node to which Gamebryo gives the motion not accumulated by the root.
pub const NON_ACCUM: &str = " NonAccum";

/// Name of the opening sequence (case-insensitive).
pub const OPEN_SEQUENCE: &str = "Open";

/// A rotation smaller than this is not a rotation (key noise), radians.
pub const MIN_TURN: f32 = 0.1;

/// A translation smaller than this is not a translation, units.
pub const MIN_SLIDE: f32 = 4.0;

/// A translation of a rotating leaf smaller than this is a correction to the rotation (the
/// `FenceGate` gate sinks by 7 units), not a separate motion, units.
pub const BIG_SLIDE: f32 = 32.0;

/// An axis counts as vertical if the axis's |z| is at least this.
pub const VERTICAL: f32 = 0.9;

/// A hinge closer than this to the mesh origin already stands where Morrowind expects it, units.
pub const AT_ORIGIN: f32 = 1.0;

/// A node that `Open` moves.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaf {
    pub node: String,
    /// The node's block in the source.
    pub index: usize,
    /// The node's origin in the closed position, in the mesh frame: the hinge axis passes through
    /// it.
    pub pivot: Vec3,
    /// Rotation axis from closed to open in the mesh frame (unit; zero means no rotation).
    pub axis: Vec3,
    /// Rotation angle around `axis`, radians, 0…π.
    pub angle: f32,
    /// The node's translation from closed to open in the mesh frame.
    pub slide: Vec3,
    /// How much the file's pose differs from the first `Open` key, radians: 0 means the file lies
    /// in the closed position.
    pub pose_gap: f32,
    /// Shapes under the node.
    pub shapes: usize,
}

impl Leaf {
    /// The leaf rotates (not only slides).
    pub fn turns(&self) -> bool {
        self.angle >= MIN_TURN
    }

    /// The leaf slides: a portcullis, a sliding door. For a rotating leaf only a large translation
    /// counts.
    pub fn slides(&self) -> bool {
        self.slide.length() >= if self.turns() { BIG_SLIDE } else { MIN_SLIDE }
    }

    /// The hinge is vertical — Morrowind can do such a rotation.
    pub fn vertical(&self) -> bool {
        self.axis.z.abs() >= VERTICAL
    }

    /// Signed angle around the mesh's +Z, radians; 0 for a non-vertical axis.
    pub fn turn_about_z(&self) -> f32 {
        if self.vertical() {
            self.angle * self.axis.z.signum()
        } else {
            0.0
        }
    }
}

/// A door's motion from its `Open`.
#[derive(Debug, Clone, PartialEq)]
pub struct DoorMotion {
    /// Duration of `Open`, seconds.
    pub duration: f32,
    /// Nodes that `Open` rotates or slides, in controller block order. Only the top ones: a node
    /// under another moving node (a handle on a leaf) is part of the same leaf.
    pub leaves: Vec<Leaf>,
    /// Moving nodes under other moving ones.
    pub nested: usize,
    /// Shapes under the moving nodes and outside them (frame, threshold, wall).
    pub moving_shapes: usize,
    pub fixed_shapes: usize,
}

/// What Morrowind can do with the door.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DoorKind {
    /// `Open` moves nothing (or is absent): the door is like a static.
    Still,
    /// One leaf on a vertical hinge without translation — Morrowind's rotation expresses it.
    Hinged,
    /// Several leaves: Morrowind has one axis.
    Leaves,
    /// Translation (portcullis, sliding door): Morrowind has only rotation.
    Sliding,
    /// Rotation around a non-vertical axis (hatch, drawbridge).
    Tilting,
}

/// What the conversion does with the door — for the report and the census.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DoorFate {
    /// The mesh has no `Open`: the Oblivion door has nothing to open with (a teleport or closed
    /// forever).
    NoOpen,
    /// `Open` is present but moves nothing.
    Still,
    /// One leaf, the hinge is already at the mesh origin: Morrowind will rotate it correctly.
    AtOrigin,
    /// One leaf on an offset hinge, no frame: the mesh origin is moved to the hinge.
    Rehinged,
    /// One leaf, but the mesh has stationary geometry: Morrowind will rotate it along with the
    /// leaf.
    Framed,
    /// Several leaves.
    Leaves,
    /// Translation: a portcullis, a sliding door.
    Sliding,
    /// Rotation around a non-vertical axis.
    Tilting,
}

/// The door's fate from its motion; `None` means there is no `Open` sequence.
pub fn fate(motion: Option<&DoorMotion>) -> DoorFate {
    let Some(m) = motion else {
        return DoorFate::NoOpen;
    };
    match m.kind() {
        DoorKind::Still => DoorFate::Still,
        DoorKind::Leaves => DoorFate::Leaves,
        DoorKind::Sliding => DoorFate::Sliding,
        DoorKind::Tilting => DoorFate::Tilting,
        DoorKind::Hinged if m.hinge().is_some() => DoorFate::Rehinged,
        DoorKind::Hinged if m.fixed_shapes > 0 => DoorFate::Framed,
        DoorKind::Hinged => DoorFate::AtOrigin,
    }
}

impl DoorMotion {
    /// The leaf on its hinge opens counterclockwise (seen from above), while Morrowind rotates a
    /// door only clockwise: such a door will open the other way.
    pub fn opens_counter_clockwise(&self) -> bool {
        self.kind() == DoorKind::Hinged && self.leaves[0].turn_about_z() > 0.0
    }

    pub fn kind(&self) -> DoorKind {
        match self.leaves.as_slice() {
            [] => DoorKind::Still,
            [leaf] if leaf.slides() => DoorKind::Sliding,
            [leaf] if leaf.vertical() => DoorKind::Hinged,
            [_] => DoorKind::Tilting,
            many if many.iter().any(Leaf::slides) => DoorKind::Sliding,
            many if many.iter().all(Leaf::vertical) => DoorKind::Leaves,
            _ => DoorKind::Tilting,
        }
    }

    /// The hinge of the only leaf, if it can be moved to the mesh origin: one leaf on a vertical
    /// hinge, and nothing in the mesh stands outside it (otherwise the frame would rotate too).
    /// `None` means a door of another kind or the hinge is already at the origin.
    pub fn hinge(&self) -> Option<Vec3> {
        if self.kind() != DoorKind::Hinged || self.fixed_shapes > 0 {
            return None;
        }
        let pivot = self.leaves[0].pivot;
        (pivot.truncate().length() >= AT_ORIGIN).then_some(Vec3::new(pivot.x, pivot.y, 0.0))
    }
}

/// NIF quaternion (`w, x, y, z`) → `glam`.
fn quat(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[1], q[2], q[3], q[0]).normalize()
}

/// Rotation without scale.
fn rotation_of(a: &Affine3A) -> Quat {
    Quat::from_mat3a(&Mat3A::from_cols(
        a.matrix3.x_axis.normalize_or_zero(),
        a.matrix3.y_axis.normalize_or_zero(),
        a.matrix3.z_axis.normalize_or_zero(),
    ))
    .normalize()
}

/// The first and last rotation of a track. NIF Euler angles are rotations around X, Y, Z in turn;
/// doors usually have one track, and the order does not affect the result.
fn turn_ends(keys: &TransformKeys) -> Option<(Quat, Quat)> {
    match &keys.rotations {
        Rotations::None => None,
        Rotations::Quat(list) => Some((quat(list.first()?.1), quat(list.last()?.1))),
        Rotations::Euler(axes) => {
            if axes.iter().all(Vec::is_empty) {
                return None;
            }
            let turn = |angles: [f32; 3]| {
                Quat::from_rotation_z(angles[2])
                    * Quat::from_rotation_y(angles[1])
                    * Quat::from_rotation_x(angles[0])
            };
            let first = [0, 1, 2].map(|axis| axes[axis].first().map_or(0.0, |k| k.1));
            let last = [0, 1, 2].map(|axis| axes[axis].last().map_or(0.0, |k| k.1));
            Some((turn(first), turn(last)))
        }
    }
}

/// A door's motion: `None` means the mesh has no `Open` sequence.
pub fn door_motion(source: &SourceNif, sequences: &[Sequence]) -> Option<DoorMotion> {
    let open = sequences
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(OPEN_SEQUENCE))?;

    // Tree traversal: the parent's world pose and the block itself, by name.
    let mut parents: HashMap<usize, (Option<usize>, Affine3A)> = HashMap::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut stack: Vec<(usize, Option<usize>, Affine3A)> = source
        .roots
        .iter()
        .filter_map(|r| r.index())
        .map(|i| (i, None, Affine3A::IDENTITY))
        .collect();
    while let Some((index, parent, parent_world)) = stack.pop() {
        if parents.contains_key(&index) {
            continue;
        }
        let (av, children) = match source.blocks.get(index) {
            Some(Block::Node(n)) => (&n.av, n.children.as_slice()),
            Some(Block::TriStrips(s) | Block::TriShape(s)) => (&s.av, [].as_slice()),
            _ => continue,
        };
        parents.insert(index, (parent, parent_world));
        by_name
            .entry(av.net.name.to_ascii_lowercase())
            .or_insert(index);
        let world = parent_world * av_affine(av);
        for c in children.iter().filter_map(|r| r.index()) {
            stack.push((c, Some(index), world));
        }
    }

    let mut leaves: Vec<Leaf> = Vec::new();
    for track in &open.tracks {
        let (Some(keys), Some(&index)) = (
            track.keys.as_ref(),
            by_name.get(&track.node.to_ascii_lowercase()),
        ) else {
            continue;
        };
        if leaves.iter().any(|l| l.index == index) {
            continue;
        }
        let av = match &source.blocks[index] {
            Block::Node(n) => &n.av,
            Block::TriStrips(s) | Block::TriShape(s) => &s.av,
            _ => continue,
        };
        let (parent, parent_world) = parents[&index];
        // The frame the keys are written in. Usually it is the node's parent. Not for a
        // `… NonAccum` node: Gamebryo splits the motion between the accumulation root (the parent)
        // and this node, and the node's keys hold the whole transform of the pair, leaving identity
        // to the root (for `DoorLowerINT03` the node's Z keys start at −π — its parent's rotation
        // in the file). Their frame is the accumulation root's parent.
        let frame = match parent.filter(|_| track.node.ends_with(NON_ACCUM)) {
            Some(accum_root) => parents[&accum_root].1,
            None => parent_world,
        };
        let frame_turn = rotation_of(&frame);
        // The node's pose in the file in the same frame — the first key is compared with it.
        let file_turn = frame_turn.inverse() * rotation_of(&(parent_world * av_affine(av)));

        let ends = turn_ends(keys);
        let (axis, angle, pose_gap) = match ends {
            Some((first, last)) => {
                // Rotation from closed to open in the key frame → into the mesh frame.
                let delta = frame_turn * (last * first.inverse()) * frame_turn.inverse();
                let (mut axis, mut angle) = delta.normalize().to_axis_angle();
                if angle > std::f32::consts::PI {
                    angle = std::f32::consts::TAU - angle;
                    axis = -axis;
                }
                (axis, angle, first.angle_between(file_turn))
            }
            None => (Vec3::ZERO, 0.0, 0.0),
        };
        // The hinge is where the node stands in the file. The interpolator pose does not fit for
        // this: for a `… NonAccum` node it repeats the translation of the parent node (the
        // accumulation root), and with it the hinge of the Dementia door came out twice as far as
        // the real one — (97.6, 5.8) instead of (48.8, 2.9), 2026-10-02.
        let pivot = parent_world.transform_point3a(Vec3A::from_array(av.translation));
        // Translation is the difference of the extreme keys: it does not depend on the key frame.
        let slide = match (keys.translations.first(), keys.translations.last()) {
            (Some(first), Some(last)) => {
                frame.transform_vector3a(Vec3A::from_array(last.1) - Vec3A::from_array(first.1))
            }
            _ => Vec3A::ZERO,
        };
        let leaf = Leaf {
            node: track.node.clone(),
            index,
            pivot: Vec3::from(pivot),
            axis: if angle < MIN_TURN { Vec3::ZERO } else { axis },
            angle,
            slide: Vec3::from(slide),
            pose_gap,
            shapes: 0,
        };
        if leaf.turns() || leaf.slides() {
            leaves.push(leaf);
        }
    }

    // A node under another moving node is a detail of the same leaf.
    let all: BTreeSet<usize> = leaves.iter().map(|l| l.index).collect();
    let before = leaves.len();
    leaves.retain(|leaf| {
        let mut at = parents.get(&leaf.index).and_then(|p| p.0);
        while let Some(i) = at {
            if all.contains(&i) {
                return false;
            }
            at = parents.get(&i).and_then(|p| p.0);
        }
        true
    });
    let nested = before - leaves.len();

    // Shapes: under which moving node (the nearest above) each one stands.
    let moving: BTreeSet<usize> = leaves.iter().map(|l| l.index).collect();
    let (mut moving_shapes, mut fixed_shapes) = (0usize, 0usize);
    for &index in parents.keys() {
        let Some(Block::TriStrips(shape) | Block::TriShape(shape)) = source.blocks.get(index)
        else {
            continue;
        };
        // Editor-only geometry is not visible in the game and does not count as a frame.
        if shape
            .av
            .net
            .name
            .to_ascii_lowercase()
            .starts_with("editormarker")
        {
            continue;
        }
        let mut at = Some(index);
        let mut owner = None;
        while let Some(i) = at {
            if moving.contains(&i) {
                owner = Some(i);
                break;
            }
            at = parents.get(&i).and_then(|p| p.0);
        }
        match owner {
            Some(o) => {
                moving_shapes += 1;
                if let Some(leaf) = leaves.iter_mut().find(|l| l.index == o) {
                    leaf.shapes += 1;
                }
            }
            None => fixed_shapes += 1,
        }
    }
    Some(DoorMotion {
        duration: open.stop - open.start,
        leaves,
        nested,
        moving_shapes,
        fixed_shapes,
    })
}

/// Moves the origin of the finished mesh to point `hinge` (in the mesh frame): everything under the
/// root — geometry, nodes, `RootCollisionNode` — shifts by `−hinge`. The root's children are
/// shifted, not the root itself: whether the vanilla engine takes the root node's transform of a
/// placed object into account is unverified, and shifting the children does not depend on it.
///
/// For the door to stay in place, the position of each of its references must be shifted by the
/// same vector, rotated and scaled like the reference (`transform::exterior`).
pub fn rehinge(converted: &mut Converted, hinge: Vec3) {
    let stream = &mut converted.stream;
    for root in stream.roots.clone() {
        let Some(node) = stream.get_as::<_, NiNode>(root) else {
            continue;
        };
        // A mesh point x = t + s·R·x_root, where R = (the file's nine numbers)ᵀ: the shift in the
        // root frame is Rᵀ·hinge / s, and Rᵀ is the matrix of the file's numbers as is.
        let scale = if node.base.scale == 0.0 {
            1.0
        } else {
            node.base.scale
        };
        let local = node.base.rotation * hinge / scale;
        for child in node.children.clone() {
            if let Some(av) = stream.get_mut(child) {
                av.translation -= local;
            }
        }
    }
}

#[cfg(test)]
mod tests;
