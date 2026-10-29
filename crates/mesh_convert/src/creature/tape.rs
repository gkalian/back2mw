//! Creature animation tape (T11.5d): Oblivion clips → one Morrowind key tape with group text keys.
//!
//! Oblivion has a separate clip for each action (`read::clip`); in Morrowind all of a creature's
//! animation is one tape: a `NiKeyframeController` on each bone and `Group: key` text keys on the
//! accumulation root that cut it into groups (`docs/en/formats/tes3.md`, "Creature model and
//! animation"). Clips are placed on the tape one after another, one key step apart; which group
//! comes from which clip is set by `data/creature_groups.tsv` (`super::groups`).
//!
//! **Keys.** Each clip track — keys, Euler angles or a B-spline — is sampled at the [`KEY_STEP`]
//! step (1/15 s, as in vanilla: owner's decision 3) into linear rotation, translation and scale
//! keys; the values are the bone's local transforms, as in Oblivion. A bone without a track in the
//! clip stands in the skeleton's pose. Keys equal to both neighbors are dropped.
//!
//! **Accumulation root.** In vanilla creatures every group starts with the root at the same point,
//! and in attacks and hit reactions the root stays in place (a census of 33 creatures of
//! `Morrowind.bsa`, 2026-10-02: 31 of 31 walks, 30 of 31 runs and 94 of 96 attacks start in place;
//! the root is still in 88 attacks of 96 and in 51 hit reactions of 61). Oblivion clips work
//! differently: a clip starts wherever it likes (the dog's run is 30 units ahead), and an attack's
//! lunge and recoil are written as a root shift — there the engine itself turns it into movement.
//! So for each segment the root's shift in the ground plane is measured from the point where the
//! root stands in the first frame of `Idle`, and for the kinds `attack`, `cast-…`, `turn` and
//! `still` the root stays at that point (`Span::pin_root`).
//!
//! **Loop.** In vanilla the pose at `Loop Stop` equals the pose at `Loop Start` (a difference of up
//! to 0.07°), in Oblivion clips it does not (3…5.5° for the dog: the splines are lossily
//! compressed). The last [`CLOSE_SECONDS`] of a loop are brought to the pose of its start; the
//! root's path is not touched.
//!
//! **Death** (owner's decision 1). Oblivion creatures have no death clip — they die as a ragdoll.
//! The `Death1` group is built: from the first frame of `Idle` the body topples sideways over one
//! second around the longitudinal axis running along the ground at the edge of the bounds.
//! `Knockout`, `DeathKnockDown` and `DeathKnockOut` lie on the same segment, as with the vanilla
//! guar; after it the creature gets up — the end of `Knockout`.

use std::collections::{BTreeSet, HashMap};

use tes3::nif::glam::{Affine3A, Quat, Vec3};
use tes3::nif::{
    NiExtraData, NiFloatData, NiFloatKey, NiKeyframeController, NiKeyframeData, NiLinFloatKey,
    NiLinPosKey, NiLinRotKey, NiLink, NiNode, NiObjectNET, NiPosData, NiPosKey, NiRotData,
    NiRotKey, NiSequenceStreamHelper, NiStream, NiStringExtraData, NiTextKey, NiTextKeyExtraData,
    NiTimeController, NiTriShape, NiType,
};

use super::groups::{self, GroupKind, GroupRule, STANCES};
use super::{Body, rest_bones};
use crate::geometry::ConvertError;
use crate::read::clip::{BoneMotion, Channel, Clip, Rotation};
use crate::read::{Block, SourceNif};

/// Tape key step, seconds: as in vanilla Morrowind creatures.
pub const KEY_STEP: f32 = 1.0 / 15.0;

/// `NiKeyframeController` flags of vanilla creatures (1,956 of 1,961): active, clamped.
const CONTROLLER_FLAGS: u16 = 0x000C;
/// Duration of our own fall and rise, seconds.
const FALL_SECONDS: f32 = 1.0;
/// How many seconds before the end of a loop the pose starts converging to the start pose.
pub const CLOSE_SECONDS: f32 = 0.2;
/// Tolerance for comparing times.
const TIME_EPSILON: f32 = 1e-4;

/// How many groups can be open at once — from their first key to `Stop`. When parsing the tape
/// keys, the engine and the Construction Set keep open groups in five slots; there is no room for a
/// sixth, and both write "Too many shared anim groups. Up the MAX_OPEN_ANIM_GROUPS in
/// TES3HandlerNew.cpp" (disassembly: `TES Construction Set.exe` 0x509BC1, `Morrowind.exe` 0x4C343C;
/// found by the owner 2026-10-02 — nine idle groups lay on the `Idle` segment). All groups count
/// except `SpellCast` and weapon stances: these are the ones whose keys are `Start`/`Stop` with a
/// loop or `Hit`.
pub const MAX_OPEN_GROUPS: usize = 5;

/// Groups of our own fall ([`GroupKind::Fall`]) beyond the rule's own group.
const FALL_GROUPS: [&str; 3] = ["Knockout", "DeathKnockDown", "DeathKnockOut"];
const _: () = assert!(FALL_GROUPS.len() < MAX_OPEN_GROUPS);

/// Whether a group takes an open slot: every kind except spells (the engine does not count
/// `SpellCast`).
fn holds_a_slot(kind: GroupKind) -> bool {
    !matches!(
        kind,
        GroupKind::CastSelf | GroupKind::CastTouch | GroupKind::CastTarget
    )
}

/// A clip of a creature folder: the file name without path and extension (it holds the stance) and
/// the clip itself.
#[derive(Debug, Clone, Copy)]
pub struct FolderClip<'a> {
    pub file: &'a str,
    pub clip: &'a Clip,
}

/// A tape track: the keys of one bone, times from the start of the tape.
#[derive(Debug, Clone, PartialEq)]
pub struct TapeTrack {
    pub bone: String,
    pub rotations: Vec<(f32, Quat)>,
    pub translations: Vec<(f32, Vec3)>,
    /// Empty if the bone's scale is one everywhere.
    pub scales: Vec<(f32, f32)>,
}

/// The tape and what went into it.
#[derive(Debug, Clone, PartialEq)]
pub struct Tape {
    /// The accumulation root: the text keys hang on it.
    pub accum_root: String,
    pub tracks: Vec<TapeTrack>,
    /// Text keys by time; the value is one or more lines separated by `\r\n`.
    pub marks: Vec<(f32, String)>,
    pub length: f32,
    /// Written groups, in table order.
    pub groups: Vec<String>,
    /// Table groups for which the folder has no clip.
    pub missing: Vec<String>,
    /// Groups not written because [`MAX_OPEN_GROUPS`] groups already lie on their clip's segment.
    pub crowded: Vec<String>,
    /// Clip files that went into the tape, in tape order.
    pub clips: Vec<String>,
    /// Tracks of the included clips whose target is not a skeleton bone (morphs, particles): not
    /// carried over.
    pub dropped_tracks: usize,
    /// `Sound: <EDID>` keys of the included clips: not carried over (a sound ID is needed — T11.6).
    pub dropped_sounds: usize,
}

/// A bone pose: what a key stores.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pose {
    rotation: Quat,
    translation: Vec3,
    scale: f32,
}

impl Pose {
    fn of(t: Affine3A) -> Self {
        let (scale, rotation, translation) = t.to_scale_rotation_translation();
        Self {
            rotation: rotation.normalize(),
            translation,
            scale: scale.x,
        }
    }

    fn affine(self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(
            Vec3::splat(self.scale),
            self.rotation,
            self.translation,
        )
    }
}

/// NIF quaternion (`w, x, y, z`) → `glam`.
fn quat(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[1], q[2], q[3], q[0]).normalize()
}

/// The value between neighboring keys, linear; before the first and after the last — the edge one.
fn lerp_keys<const N: usize>(keys: &[(f32, [f32; N])], time: f32) -> Option<[f32; N]> {
    let first = keys.first()?;
    if time <= first.0 {
        return Some(first.1);
    }
    for pair in keys.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if time <= b.0 {
            let span = b.0 - a.0;
            let f = if span > 0.0 { (time - a.0) / span } else { 1.0 };
            return Some(std::array::from_fn(|i| a.1[i] + (b.1[i] - a.1[i]) * f));
        }
    }
    keys.last().map(|k| k.1)
}

/// Rotation from three NIF Euler angles: around X, then Y, then Z (the order was checked on
/// creature clips: the first `idle` frame against the skeleton pose, `real_creature_tapes`).
pub fn euler(angles: [f32; 3]) -> Quat {
    Quat::from_rotation_z(angles[2])
        * Quat::from_rotation_y(angles[1])
        * Quat::from_rotation_x(angles[0])
}

fn rotation_at(rotation: &Rotation, time: f32) -> Option<Quat> {
    match rotation {
        Rotation::None => None,
        Rotation::Quat(keys) => {
            let first = keys.first()?;
            if time <= first.0 {
                return Some(quat(first.1));
            }
            for pair in keys.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if time <= b.0 {
                    let span = b.0 - a.0;
                    let f = if span > 0.0 { (time - a.0) / span } else { 1.0 };
                    return Some(quat(a.1).slerp(quat(b.1), f));
                }
            }
            keys.last().map(|k| quat(k.1))
        }
        Rotation::Euler(axes) => {
            if axes.iter().all(Vec::is_empty) {
                return None;
            }
            let angle = |axis: &Vec<(f32, f32)>| {
                let keys: Vec<(f32, [f32; 1])> = axis.iter().map(|k| (k.0, [k.1])).collect();
                lerp_keys(&keys, time).map_or(0.0, |v| v[0])
            };
            Some(euler([angle(&axes[0]), angle(&axes[1]), angle(&axes[2])]))
        }
        Rotation::Spline(spline) => spline.at(time).map(quat),
    }
}

fn channel_at<const N: usize>(channel: &Channel<N>, time: f32) -> Option<[f32; N]> {
    match channel {
        Channel::None => None,
        Channel::Keys(keys) => lerp_keys(keys, time),
        Channel::Spline(spline) => spline.at(time),
    }
}

/// A bone's pose at clip time `time`: a channel without keys stands in the interpolator's pose, and
/// if that is not set either — in the skeleton's pose.
fn sample(motion: &BoneMotion, rest: Pose, time: f32) -> Pose {
    Pose {
        rotation: rotation_at(&motion.rotation, time)
            .or(motion.pose_rotation.map(quat))
            .unwrap_or(rest.rotation),
        translation: channel_at(&motion.translation, time)
            .or(motion.pose_translation)
            .map_or(rest.translation, Vec3::from_array),
        scale: channel_at(&motion.scale, time)
            .map(|v| v[0])
            .or(motion.pose_scale)
            .unwrap_or(rest.scale),
    }
}

/// Where a tape segment comes from.
#[derive(Clone, Copy)]
enum Source<'a> {
    Clip(FolderClip<'a>),
    /// Our own fall and rise.
    Fall,
}

struct Span<'a> {
    start: f32,
    duration: f32,
    source: Source<'a>,
    /// A turn group: the accumulation root's rotation freezes at the first frame.
    hold_root_rotation: bool,
    /// The accumulation root stays in place: the segment holds only groups of the [`pins_root`]
    /// kinds.
    pin_root: bool,
    /// The segment has a loop: its end converges to the start pose.
    close_loop: bool,
}

/// A group kind in which the accumulation root does not walk along the ground (this is how vanilla
/// attacks, spells, turns and hit reactions work).
fn pins_root(kind: GroupKind) -> bool {
    matches!(
        kind,
        GroupKind::Attack
            | GroupKind::CastSelf
            | GroupKind::CastTouch
            | GroupKind::CastTarget
            | GroupKind::Turn
            | GroupKind::Still
    )
}

/// The sequence name names an action: exactly (`Forward`) or with a variant tail after an
/// underscore (`AttackLeft_Cross`, `Idle_HandtoHand`, `CastSelf_A`).
fn names_action(clip_name: &str, action: &str) -> bool {
    let name = clip_name.to_ascii_lowercase();
    name == action
        || name
            .strip_prefix(action)
            .is_some_and(|tail| tail.starts_with('_'))
}

/// The clip for a rule: the first action and the first stance for which the folder has a file.
/// Among several matching ones — first the exact action name, then by file name.
fn choose<'a>(rule: &GroupRule, clips: &[FolderClip<'a>]) -> Option<FolderClip<'a>> {
    for action in &rule.actions {
        for stance in &rule.stances {
            let mut found: Vec<FolderClip<'a>> = clips
                .iter()
                .copied()
                .filter(|c| names_action(&c.clip.name, action))
                .filter(|c| {
                    if stance.is_empty() {
                        !STANCES.iter().any(|s| c.file.starts_with(s))
                    } else {
                        c.file.starts_with(stance.as_str())
                    }
                })
                .collect();
            found.sort_by(|a, b| {
                let exact = |c: &FolderClip<'_>| !c.clip.name.eq_ignore_ascii_case(action);
                exact(a).cmp(&exact(b)).then(a.file.cmp(b.file))
            });
            if let Some(first) = found.first() {
                return Some(*first);
            }
        }
    }
    None
}

/// Key times of a segment of length `duration`: at the step and at the very end.
fn key_times(duration: f32, step: f32) -> Vec<f32> {
    let mut out = Vec::new();
    let mut k = 0u32;
    loop {
        #[expect(clippy::cast_precision_loss, reason = "a clip has hundreds of keys")]
        let t = k as f32 * step;
        if t > duration + TIME_EPSILON {
            break;
        }
        out.push(t.min(duration));
        k += 1;
    }
    if out.last().is_none_or(|last| duration - last > TIME_EPSILON) {
        out.push(duration);
    }
    out
}

/// Time of the clip's `Hit` key: a line starting with `hit` (it can be `Hit.`, `Hit -at xy`).
fn hit_time(clip: &Clip) -> Option<f32> {
    clip.text_keys.iter().find_map(|(time, text)| {
        text.lines()
            .any(|line| line.trim().to_ascii_lowercase().starts_with("hit"))
            .then_some(*time)
    })
}

/// Body rotation when falling by angle `angle`: around the forward axis (Y) running along the
/// ground `half_width` to the side — the body tips over its edge.
fn fall(angle: f32, half_width: f32) -> Affine3A {
    let pivot = Vec3::new(half_width, 0.0, 0.0);
    Affine3A::from_translation(pivot)
        * Affine3A::from_rotation_y(angle)
        * Affine3A::from_translation(-pivot)
}

/// Fall angle at segment time `time`: a second of accelerating fall, a second of rising.
fn fall_angle(time: f32) -> f32 {
    let quarter = std::f32::consts::FRAC_PI_2;
    if time <= FALL_SECONDS {
        let s = (time / FALL_SECONDS).clamp(0.0, 1.0);
        quarter * s * s
    } else {
        let u = ((time - FALL_SECONDS) / FALL_SECONDS).clamp(0.0, 1.0);
        quarter * (1.0 - u) * (1.0 - u)
    }
}

/// Removes keys equal to both neighbors: two keys hold a constant segment.
fn thin<T: Copy>(keys: &mut Vec<(f32, T)>, same: impl Fn(T, T) -> bool) {
    if keys.len() < 3 {
        return;
    }
    let mut out: Vec<(f32, T)> = Vec::with_capacity(keys.len());
    for (i, key) in keys.iter().enumerate() {
        let inner = i > 0 && i + 1 < keys.len();
        if inner && same(keys[i - 1].1, key.1) && same(key.1, keys[i + 1].1) {
            continue;
        }
        out.push(*key);
    }
    *keys = out;
}

/// Builds the creature tape. `skeleton` is the skeleton file (the same one the body was built
/// from), `clips` are all clips of its folder, `rules` is [`groups::rules`], `step` is the key step
/// ([`KEY_STEP`]). `side` is how far the standing body sticks out toward the fall
/// ([`standing_side`]); `None` means half the width of `BSBound`.
pub fn build_tape(
    skeleton: &SourceNif,
    clips: &[FolderClip<'_>],
    rules: &[GroupRule],
    step: f32,
    side: Option<f32>,
) -> Result<Tape, ConvertError> {
    let bones = rest_bones(skeleton)?;
    let by_name: HashMap<String, usize> = {
        let mut map = HashMap::new();
        for (i, bone) in bones.iter().enumerate() {
            map.entry(bone.name.to_ascii_lowercase()).or_insert(i);
        }
        map
    };
    let rest: Vec<Pose> = bones.iter().map(|b| Pose::of(b.local)).collect();
    let half_width = side
        .or_else(|| {
            skeleton.blocks.iter().find_map(|b| match b {
                Block::BsBound(b) => Some(b.half_extents[0]),
                _ => None,
            })
        })
        .unwrap_or(0.0);

    // Tape segments and group text keys.
    let mut spans: Vec<Span<'_>> = Vec::new();
    let mut span_of_file: HashMap<&str, usize> = HashMap::new();
    let mut marks: Vec<(f32, String)> = Vec::new();
    let mut tape = Tape {
        accum_root: bones[0].name.clone(),
        tracks: Vec::new(),
        marks: Vec::new(),
        length: 0.0,
        groups: Vec::new(),
        missing: Vec::new(),
        crowded: Vec::new(),
        clips: Vec::new(),
        dropped_tracks: 0,
        dropped_sounds: 0,
    };
    // How many groups taking an open slot already lie on a segment (by index in `spans`).
    let mut open_on_span: HashMap<usize, usize> = HashMap::new();
    let mut cursor = 0.0f32;
    let mut idle: Option<FolderClip<'_>> = None;
    for rule in rules {
        if rule.kind == GroupKind::Fall {
            let start = cursor;
            spans.push(Span {
                start,
                duration: 2.0 * FALL_SECONDS,
                source: Source::Fall,
                hold_root_rotation: false,
                pin_root: false,
                close_loop: false,
            });
            cursor += 2.0 * FALL_SECONDS + step;
            let group = &rule.group;
            let down = start + FALL_SECONDS;
            for (time, text) in [
                (start, format!("{group}: Start")),
                (start, "Knockout: Start".to_owned()),
                (start + 3.0 * step, "SoundGen: Scream".to_owned()),
                (start + 3.0 * step, "DeathKnockDown: Start".to_owned()),
                (start + 12.0 * step, "DeathKnockOut: Start".to_owned()),
                (down, format!("{group}: Stop")),
                (down, "Knockout: Loop Start".to_owned()),
                (down, "Knockout: Loop Stop".to_owned()),
                (down, "DeathKnockDown: Stop".to_owned()),
                (down, "DeathKnockOut: Stop".to_owned()),
                (start + 2.0 * FALL_SECONDS, "Knockout: Stop".to_owned()),
            ] {
                marks.push((time, text));
            }
            for name in std::iter::once(group.as_str()).chain(FALL_GROUPS) {
                tape.groups.push(name.to_owned());
            }
            continue;
        }
        let Some(chosen) = choose(rule, clips) else {
            if !tape.missing.contains(&rule.group) && !tape.groups.contains(&rule.group) {
                tape.missing.push(rule.group.clone());
            }
            continue;
        };
        if rule.group.eq_ignore_ascii_case("Idle") {
            idle = Some(chosen);
        }
        let clip = chosen.clip;
        let duration = (clip.stop - clip.start).max(0.0);
        // A sixth group on the same segment is not written: at most five open at once. A group
        // already on the tape (the second `SpellCast` kind) does not take a slot again.
        let known = span_of_file.get(chosen.file).copied();
        let takes_slot = holds_a_slot(rule.kind) && !tape.groups.contains(&rule.group);
        if takes_slot
            && known.is_some_and(|i| open_on_span.get(&i).copied().unwrap_or(0) >= MAX_OPEN_GROUPS)
        {
            if !tape.crowded.contains(&rule.group) {
                tape.crowded.push(rule.group.clone());
            }
            continue;
        }
        let index = match known {
            Some(index) => index,
            None => {
                let start = cursor;
                spans.push(Span {
                    start,
                    duration,
                    source: Source::Clip(chosen),
                    hold_root_rotation: false,
                    pin_root: pins_root(rule.kind),
                    close_loop: false,
                });
                cursor += duration + step;
                tape.clips.push(chosen.file.to_owned());
                // The clip's own text keys — once per segment, however many groups share it.
                for (time, text) in &clip.text_keys {
                    for line in text.lines() {
                        if let Some(to) = groups::mark(line) {
                            marks.push((start + (time - clip.start), to.to_owned()));
                        } else if line.trim().to_ascii_lowercase().starts_with("sound:") {
                            tape.dropped_sounds += 1;
                        }
                    }
                }
                span_of_file.insert(chosen.file, spans.len() - 1);
                spans.len() - 1
            }
        };
        if takes_slot {
            *open_on_span.entry(index).or_default() += 1;
        }
        let span = &mut spans[index];
        // A clip that an attack shares with a group whose root walks does not pin the root.
        span.pin_root &= pins_root(rule.kind);
        span.close_loop |= matches!(rule.kind, GroupKind::Loop | GroupKind::Turn);
        let (start, stop) = (span.start, span.start + span.duration);
        let hit = hit_time(clip).map_or(start + span.duration * 0.5, |t| start + (t - clip.start));
        let group = &rule.group;
        let own: Vec<(f32, String)> = match rule.kind {
            GroupKind::Loop | GroupKind::Turn => vec![
                (start, format!("{group}: Start")),
                (start, format!("{group}: Loop Start")),
                (stop, format!("{group}: Loop Stop")),
                (stop, format!("{group}: Stop")),
            ],
            GroupKind::Once | GroupKind::Still => vec![
                (start, format!("{group}: Start")),
                (stop, format!("{group}: Stop")),
            ],
            GroupKind::Attack => vec![
                (start, format!("{group}: Start")),
                (hit, format!("{group}: Hit")),
                (stop, format!("{group}: Stop")),
            ],
            GroupKind::CastSelf | GroupKind::CastTouch | GroupKind::CastTarget => {
                let range = match rule.kind {
                    GroupKind::CastSelf => "Self",
                    GroupKind::CastTouch => "Touch",
                    _ => "Target",
                };
                vec![
                    (start, format!("{group}: {range} Start")),
                    (hit, format!("{group}: {range} Release")),
                    (stop, format!("{group}: {range} Stop")),
                ]
            }
            GroupKind::Fall => Vec::new(),
        };
        marks.extend(own);
        if rule.kind == GroupKind::Turn {
            span.hold_root_rotation = true;
        }
        if !tape.groups.contains(&rule.group) {
            tape.groups.push(rule.group.clone());
        }
        tape.missing.retain(|g| g != &rule.group);
    }
    tape.length = spans.last().map_or(0.0, |s| s.start + s.duration);

    // Clip tracks by bone name; targets outside the skeleton are counted and dropped.
    let mut clip_tracks: Vec<HashMap<usize, &BoneMotion>> = Vec::with_capacity(spans.len());
    let mut tracked: BTreeSet<usize> = BTreeSet::new();
    let has_fall = spans.iter().any(|s| matches!(s.source, Source::Fall));
    for span in &spans {
        let mut map: HashMap<usize, &BoneMotion> = HashMap::new();
        if let Source::Clip(chosen) = span.source {
            for track in &chosen.clip.tracks {
                match by_name.get(&track.node.to_ascii_lowercase()) {
                    Some(&bone) => {
                        if let Some(motion) = &track.motion {
                            map.entry(bone).or_insert(motion);
                            tracked.insert(bone);
                        }
                    }
                    None => tape.dropped_tracks += 1,
                }
            }
        }
        clip_tracks.push(map);
    }
    if !spans.is_empty() {
        tracked.insert(0);
    }
    if has_fall {
        tracked.extend(
            bones
                .iter()
                .enumerate()
                .filter(|(_, b)| b.parent == Some(0))
                .map(|(i, _)| i),
        );
    }

    // The pose the creature falls from: the first frame of `Idle`, and without it — the skeleton's
    // pose.
    let idle_tracks: HashMap<usize, &BoneMotion> = idle
        .map(|chosen| {
            chosen
                .clip
                .tracks
                .iter()
                .filter_map(|t| {
                    Some((
                        *by_name.get(&t.node.to_ascii_lowercase())?,
                        t.motion.as_ref()?,
                    ))
                })
                .rev()
                .collect()
        })
        .unwrap_or_default();
    let standing = |bone: usize| -> Pose {
        match (idle, idle_tracks.get(&bone)) {
            (Some(chosen), Some(motion)) => sample(motion, rest[bone], chosen.clip.start),
            _ => rest[bone],
        }
    };
    let root_standing = standing(0).affine();
    // The point on the ground where every group starts.
    let home = standing(0).translation;

    for &bone in &tracked {
        let mut track = TapeTrack {
            bone: bones[bone].name.clone(),
            rotations: Vec::new(),
            translations: Vec::new(),
            scales: Vec::new(),
        };
        for (span, tracks) in spans.iter().zip(&clip_tracks) {
            let mut held: Option<Quat> = None;
            let pose_at = |time: f32| -> Pose {
                match span.source {
                    Source::Clip(chosen) => {
                        let raw = |time: f32| match tracks.get(&bone) {
                            Some(motion) => sample(motion, rest[bone], chosen.clip.start + time),
                            None => rest[bone],
                        };
                        let mut pose = raw(time);
                        if bone == 0 {
                            // The path along the ground is measured from the point where the root
                            // stands at idle; for a group with the root in place it is that point
                            // itself.
                            let travel = if span.pin_root {
                                Vec3::ZERO
                            } else {
                                pose.translation - raw(0.0).translation
                            };
                            pose.translation.x = home.x + travel.x;
                            pose.translation.y = home.y + travel.y;
                        }
                        pose
                    }
                    Source::Fall => {
                        let base = standing(bone);
                        if bones[bone].parent == Some(0) {
                            Pose::of(
                                root_standing.inverse()
                                    * fall(fall_angle(time), half_width)
                                    * root_standing
                                    * base.affine(),
                            )
                        } else {
                            base
                        }
                    }
                }
            };
            // How far the loop's end pose is from its start pose: the last keys of the segment make
            // up this difference.
            let closing = span.close_loop.then(|| {
                let (first, last) = (pose_at(0.0), pose_at(span.duration));
                (
                    last.rotation.inverse() * first.rotation,
                    first.translation - last.translation,
                    first.scale - last.scale,
                )
            });
            for time in key_times(span.duration, step) {
                let mut pose = pose_at(time);
                if let Some((rotation, translation, scale)) = closing {
                    let window = CLOSE_SECONDS.min(span.duration * 0.5);
                    let weight = if window > 0.0 {
                        ((time - (span.duration - window)) / window).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    if weight > 0.0 {
                        pose.rotation = pose
                            .rotation
                            .slerp(pose.rotation * rotation, weight)
                            .normalize();
                        // The root's path along the ground is the creature's movement; the loop
                        // does not close it.
                        let shift = if bone == 0 {
                            Vec3::new(0.0, 0.0, translation.z)
                        } else {
                            translation
                        };
                        pose.translation += shift * weight;
                        pose.scale += scale * weight;
                    }
                }
                if bone == 0 && span.hold_root_rotation {
                    pose.rotation = *held.get_or_insert(pose.rotation);
                }
                // Neighboring keys on one side of the sphere: the engine interpolates them
                // directly.
                if let Some((_, last)) = track.rotations.last()
                    && last.dot(pose.rotation) < 0.0
                {
                    pose.rotation = -pose.rotation;
                }
                let at = span.start + time;
                track.rotations.push((at, pose.rotation));
                track.translations.push((at, pose.translation));
                track.scales.push((at, pose.scale));
            }
        }
        thin(&mut track.rotations, |a, b| a.abs_diff_eq(b, 1e-6));
        thin(&mut track.translations, |a, b| a.abs_diff_eq(b, 1e-5));
        if track.scales.iter().all(|(_, s)| (s - 1.0).abs() < 1e-5) {
            track.scales.clear();
        } else {
            thin(&mut track.scales, |a, b| (a - b).abs() < 1e-6);
        }
        tape.tracks.push(track);
    }

    // Text keys: by time, simultaneous ones as one key with several lines.
    marks.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (time, text) in marks {
        match tape.marks.last_mut() {
            Some((last, value)) if (time - *last).abs() < TIME_EPSILON => {
                if !value.split("\r\n").any(|line| line == text) {
                    value.push_str("\r\n");
                    value.push_str(&text);
                }
            }
            _ => tape.marks.push((time, text)),
        }
    }
    Ok(tape)
}

/// How many tape groups are open at once at the worst moment — counted the way the engine counts
/// them: a group takes a slot at its first key and frees it at `Stop`; `SpellCast` and sound keys
/// do not count. More than [`MAX_OPEN_GROUPS`] is an engine warning.
pub fn peak_open_groups(tape: &Tape) -> usize {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut open: BTreeSet<String> = BTreeSet::new();
    let mut peak = 0;
    for (_, text) in &tape.marks {
        for line in text.split("\r\n") {
            let Some((group, mark)) = line.split_once(':') else {
                continue;
            };
            let group = group.trim().to_ascii_lowercase();
            if matches!(group.as_str(), "spellcast" | "soundgen" | "sound") {
                continue;
            }
            if seen.insert(group.clone()) {
                open.insert(group.clone());
                peak = peak.max(open.len());
            }
            if mark.trim().eq_ignore_ascii_case("stop") {
                open.remove(&group);
            }
        }
    }
    peak
}

/// The value of the keys at time `time`: between neighbors — `mix`, beyond the edges — the edge
/// one.
fn key_at<T: Copy>(keys: &[(f32, T)], time: f32, mix: impl Fn(T, T, f32) -> T) -> Option<T> {
    let first = keys.first()?;
    if time <= first.0 {
        return Some(first.1);
    }
    for pair in keys.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if time <= b.0 {
            let span = b.0 - a.0;
            let f = if span > 0.0 { (time - a.0) / span } else { 1.0 };
            return Some(mix(a.1, b.1, f));
        }
    }
    keys.last().map(|k| k.1)
}

/// How far the body standing in the tape's first `Idle` frame sticks out in the +X direction from
/// the model's axis: it tips over this edge when falling. Measured on the mesh itself — the skin is
/// deformed into the frame's pose: the `BSBound` bounds are narrower than a body with legs spread,
/// and a corpse rotated around their edge would sink into the ground. `None` means the body has no
/// shapes.
pub fn standing_side(body: &NiStream, tape: &Tape) -> Option<f32> {
    let time = tape
        .marks
        .iter()
        .find(|(_, text)| {
            text.lines()
                .any(|line| line.eq_ignore_ascii_case("Idle: Start"))
        })
        .map_or(0.0, |(time, _)| *time);
    let mut posed = body.clone();
    let nodes = nodes_by_name(&posed);
    for track in &tape.tracks {
        let Some(&link) = nodes.get(&track.bone.to_ascii_lowercase()) else {
            continue;
        };
        let rotation = key_at(&track.rotations, time, |a, b, f| a.slerp(b, f));
        let translation = key_at(&track.translations, time, |a, b, f| a.lerp(b, f));
        let scale = key_at(&track.scales, time, |a, b, f| a + (b - a) * f);
        let Some(node) = posed.get_mut(link) else {
            continue;
        };
        if let Some(q) = rotation {
            node.base.rotation = tes3::nif::glam::Mat3::from_quat(q).transpose();
        }
        if let Some(t) = translation {
            node.base.translation = t;
        }
        if let Some(s) = scale {
            node.base.scale = s;
        }
    }
    posed.apply_skin_deforms();
    let mut side: Option<f32> = None;
    for (shape, world) in posed.geometries::<NiTriShape>() {
        let Some(data) = posed.get(shape.base.base.geometry_data) else {
            continue;
        };
        for v in &data.vertices {
            let x = world.transform_point3(*v).x;
            side = Some(side.map_or(x, |s| s.max(x)));
        }
    }
    side.map(|s| s.max(0.0))
}

/// Builds the tape for the assembled body and hangs it on it: first a draft, from it the edge of
/// the standing body ([`standing_side`]), and with that — the tape in which the corpse lies on the
/// ground.
pub fn animate(
    body: &mut Body,
    skeleton: &SourceNif,
    clips: &[FolderClip<'_>],
    rules: &[GroupRule],
    step: f32,
) -> Result<Tape, ConvertError> {
    let draft = build_tape(skeleton, clips, rules, step, None)?;
    let tape = match standing_side(&body.converted.stream, &draft) {
        Some(side) => build_tape(skeleton, clips, rules, step, Some(side))?,
        None => draft,
    };
    apply_tape(&mut body.converted.stream, &tape);
    Ok(tape)
}

/// Stream nodes by lower-case name (the first with that name).
fn nodes_by_name(stream: &NiStream) -> HashMap<String, NiLink<NiNode>> {
    let mut out = HashMap::new();
    for (link, node) in stream.objects_of_type_with_link::<NiNode>() {
        out.entry(node.base.base.name.to_ascii_lowercase())
            .or_insert(link);
    }
    out
}

fn keyframe_data(track: &TapeTrack) -> NiKeyframeData {
    NiKeyframeData {
        rotations: NiRotData {
            keys: NiRotKey::LinKey(
                track
                    .rotations
                    .iter()
                    .map(|&(time, value)| NiLinRotKey { time, value })
                    .collect(),
            ),
            ..NiRotData::default()
        },
        translations: NiPosData {
            keys: NiPosKey::LinKey(
                track
                    .translations
                    .iter()
                    .map(|&(time, value)| NiLinPosKey { time, value })
                    .collect(),
            ),
            ..NiPosData::default()
        },
        scales: NiFloatData {
            keys: NiFloatKey::LinKey(
                track
                    .scales
                    .iter()
                    .map(|&(time, value)| NiLinFloatKey { time, value })
                    .collect(),
            ),
            ..NiFloatData::default()
        },
        ..NiKeyframeData::default()
    }
}

fn controller(tape: &Tape) -> NiTimeController {
    NiTimeController {
        flags: CONTROLLER_FLAGS,
        frequency: 1.0,
        phase: 0.0,
        start_time: 0.0,
        stop_time: tape.length,
        ..NiTimeController::default()
    }
}

fn text_keys(tape: &Tape) -> NiTextKeyExtraData {
    NiTextKeyExtraData {
        base: NiExtraData::default(),
        keys: tape
            .marks
            .iter()
            .map(|(time, value)| NiTextKey {
                time: *time,
                value: value.clone(),
            })
            .collect(),
    }
}

/// Hangs the tape on the body: a keyframe controller on each bone with a track and the text keys on
/// the accumulation root — this is how a vanilla creature's ordinary `<name>.nif` is built. Returns
/// the number of hung tracks.
pub fn apply_tape(stream: &mut NiStream, tape: &Tape) -> usize {
    let nodes = nodes_by_name(stream);
    let mut applied = 0;
    for track in &tape.tracks {
        let Some(&node) = nodes.get(&track.bone.to_ascii_lowercase()) else {
            continue;
        };
        let data = stream.insert(keyframe_data(track));
        let link = stream.insert(NiKeyframeController {
            base: NiTimeController {
                target: node.cast(),
                ..controller(tape)
            },
            data,
        });
        if let Some(owner) = stream.get_mut(node) {
            owner.base.base.controller = link.cast();
        }
        applied += 1;
    }
    if let Some(&root) = nodes.get(&tape.accum_root.to_ascii_lowercase()) {
        let keys = stream.insert(text_keys(tape));
        if let Some(owner) = stream.get_mut(root) {
            owner.base.base.extra_data = keys.cast();
        }
    }
    applied
}

/// The pair of files of a vanilla creature: `x<name>.nif` and `x<name>.kf`.
#[derive(Debug)]
pub struct Pair {
    /// The same mesh without keyframe controllers and without text keys.
    pub skeleton: NiStream,
    /// `NiSequenceStreamHelper`: text keys, bone names as strings and controllers with an empty
    /// target.
    pub keys: NiStream,
}

/// Splits the full mesh with a tape ([`apply_tape`]) into the pair the engine takes instead of it.
pub fn split_pair(full: &NiStream, tape: &Tape) -> Pair {
    let mut skeleton = full.clone();
    let controllers: Vec<_> = skeleton
        .objects
        .iter()
        .filter(|(_, o)| matches!(o, NiType::NiKeyframeController(_)))
        .map(|(key, _)| key)
        .collect();
    let marks: Vec<_> = skeleton
        .objects
        .iter()
        .filter(|(_, o)| matches!(o, NiType::NiTextKeyExtraData(_)))
        .map(|(key, _)| key)
        .collect();
    for node in skeleton.objects_of_type_mut::<NiNode>() {
        if controllers.contains(&node.base.base.controller.key) {
            node.base.base.controller = NiLink::default();
        }
        if marks.contains(&node.base.base.extra_data.key) {
            node.base.base.extra_data = NiLink::default();
        }
    }
    skeleton.retain_reachable();

    // `.kf`: the chains are built from the end, so every block already has a reference to the next
    // one.
    let mut keys = NiStream::new();
    let mut next_controller: NiLink<NiTimeController> = NiLink::default();
    let mut next_name: NiLink<NiExtraData> = NiLink::default();
    for track in tape.tracks.iter().rev() {
        let data = keys.insert(keyframe_data(track));
        next_controller = keys
            .insert(NiKeyframeController {
                base: NiTimeController {
                    next: next_controller,
                    ..controller(tape)
                },
                data,
            })
            .cast();
        next_name = keys
            .insert(NiStringExtraData {
                base: NiExtraData {
                    next: next_name,
                    // String length and its `u32` counter — the way vanilla writes it (9 for
                    // `Bip01`).
                    bytes_remaining: u32::try_from(track.bone.len() + 4)
                        .expect("a bone name is a short string"),
                    ..NiExtraData::default()
                },
                value: track.bone.clone(),
            })
            .cast();
    }
    let marks = keys.insert(NiTextKeyExtraData {
        base: NiExtraData {
            next: next_name,
            ..NiExtraData::default()
        },
        ..text_keys(tape)
    });
    let root = keys.insert(NiSequenceStreamHelper {
        base: NiObjectNET {
            extra_data: marks.cast(),
            controller: next_controller,
            ..NiObjectNET::default()
        },
    });
    keys.roots.push(root.cast());
    Pair { skeleton, keys }
}
