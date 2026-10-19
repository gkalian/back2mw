//! Лента анимации существа (T11.5d): клипы Oblivion → одна лента ключей Morrowind с метками
//! групп.
//!
//! У Oblivion на каждое действие свой клип (`read::clip`), у Morrowind вся анимация существа —
//! одна лента: `NiKeyframeController` на каждой кости и текстовые ключи `Группа: метка` на корне
//! накопления, которые режут её на группы (`docs/ru/formats/tes3.md`, «Модель и анимация
//! существа»). Клипы встают на ленту подряд, через один шаг ключей; какая группа из какого клипа —
//! `data/creature_groups.tsv` (`super::groups`).
//!
//! **Ключи.** Каждая дорожка клипа — ключи, углы Эйлера или B-сплайн — снимается с шагом
//! [`KEY_STEP`] (1/15 с, как у ванили: решение владельца 3) в линейные ключи поворота, сдвига и
//! масштаба; значения — локальные преобразования кости, как и у Oblivion. Кость без дорожки в
//! клипе стоит в позе скелета. Ключи, равные обоим соседям, выбрасываются.
//!
//! **Корень накопления.** У ванильных существ каждая группа начинается с корнем в одной и той
//! же точке, а в ударах и реакции на удар корень стоит на месте (перепись 33 существ
//! `Morrowind.bsa`, 2026-10-02: с места начинаются 31 из 31 ходьбы, 30 из 31 бега, 94 из 96
//! ударов; корень неподвижен в 88 ударах из 96 и в 51 реакции на удар из 61). Клипы Oblivion
//! устроены иначе: клип начинается, где ему удобно (бег собаки — в 30 единицах впереди), а выпад
//! удара и отскок записаны сдвигом корня — там его превращает в движение сам движок. Поэтому
//! сдвиг корня в плоскости земли у каждого отрезка отсчитывается от точки, где корень стоит в
//! первом кадре `Idle`, а у видов `attack`, `cast-…`, `turn` и `still` корень в этой точке и
//! остаётся (`Span::pin_root`).
//!
//! **Петля.** У ванили поза в `Loop Stop` равна позе в `Loop Start` (расхождение до 0.07°), у
//! клипов Oblivion — нет (у собаки 3…5.5°: сплайны сжаты с потерями). Последние
//! [`CLOSE_SECONDS`] петли доводятся до позы её начала; путь корня при этом не трогается.
//!
//! **Смерть** (решение владельца 1). Клипа смерти у существ Oblivion нет — они умирают
//! рэгдоллом. Группа `Death1` строится: из первого кадра `Idle` тело за секунду заваливается
//! набок вокруг продольной оси, проходящей по земле у края габаритов. На том же отрезке, как у
//! ванильного гуара, лежат `Knockout`, `DeathKnockDown` и `DeathKnockOut`; за ним существо
//! встаёт — конец `Knockout`.

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

/// Шаг ключей ленты, секунды: как у ванильных существ Morrowind.
pub const KEY_STEP: f32 = 1.0 / 15.0;

/// Флаги `NiKeyframeController` ванильных существ (1 956 из 1 961): активен, зажат.
const CONTROLLER_FLAGS: u16 = 0x000C;
/// Длительность своего падения и подъёма, секунды.
const FALL_SECONDS: f32 = 1.0;
/// За сколько секунд до конца петли поза начинает сводиться к позе начала.
pub const CLOSE_SECONDS: f32 = 0.2;
/// Допуск сравнения времён.
const TIME_EPSILON: f32 = 1e-4;

/// Сколько групп могут быть открыты разом — от первой своей метки до `Stop`. Разбирая метки
/// ленты, движок и Construction Set держат открытые группы в пяти ячейках; шестой места нет, и
/// оба пишут «Too many shared anim groups. Up the MAX_OPEN_ANIM_GROUPS in TES3HandlerNew.cpp»
/// (дизассемблер: `TES Construction Set.exe` 0x509BC1, `Morrowind.exe` 0x4C343C; найдено
/// владельцем 2026-10-02 — на отрезке `Idle` лежало девять групп простоя). Считаются все группы,
/// кроме `SpellCast` и стоек с оружием: это те, чьи метки — `Start`/`Stop` с петлёй или `Hit`.
pub const MAX_OPEN_GROUPS: usize = 5;

/// Группы своего падения ([`GroupKind::Fall`]) сверх группы самого правила.
const FALL_GROUPS: [&str; 3] = ["Knockout", "DeathKnockDown", "DeathKnockOut"];
const _: () = assert!(FALL_GROUPS.len() < MAX_OPEN_GROUPS);

/// Занимает ли группа ячейку открытых: все виды, кроме заклинаний (`SpellCast` движок не считает).
fn holds_a_slot(kind: GroupKind) -> bool {
    !matches!(
        kind,
        GroupKind::CastSelf | GroupKind::CastTouch | GroupKind::CastTarget
    )
}

/// Клип папки существа: имя файла без пути и расширения (в нём стойка) и сам клип.
#[derive(Debug, Clone, Copy)]
pub struct FolderClip<'a> {
    pub file: &'a str,
    pub clip: &'a Clip,
}

/// Дорожка ленты: ключи одной кости, времена — от начала ленты.
#[derive(Debug, Clone, PartialEq)]
pub struct TapeTrack {
    pub bone: String,
    pub rotations: Vec<(f32, Quat)>,
    pub translations: Vec<(f32, Vec3)>,
    /// Пусто, если масштаб кости везде единица.
    pub scales: Vec<(f32, f32)>,
}

/// Лента и что в неё вошло.
#[derive(Debug, Clone, PartialEq)]
pub struct Tape {
    /// Корень накопления: на нём висят метки.
    pub accum_root: String,
    pub tracks: Vec<TapeTrack>,
    /// Метки по времени; значение — одна или несколько строк через `\r\n`.
    pub marks: Vec<(f32, String)>,
    pub length: f32,
    /// Записанные группы, в порядке таблицы.
    pub groups: Vec<String>,
    /// Группы таблицы, для которых в папке нет клипа.
    pub missing: Vec<String>,
    /// Группы, не записанные оттого, что на отрезке их клипа уже лежит [`MAX_OPEN_GROUPS`] групп.
    pub crowded: Vec<String>,
    /// Файлы клипов, вошедшие в ленту, в порядке ленты.
    pub clips: Vec<String>,
    /// Дорожки вошедших клипов, чья цель — не кость скелета (морфы, частицы): не переносятся.
    pub dropped_tracks: usize,
    /// Метки `Sound: <EDID>` вошедших клипов: не переносятся (нужен ID звука — T11.6).
    pub dropped_sounds: usize,
}

/// Поза кости: то, что хранит ключ.
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

/// Кватернион NIF (`w, x, y, z`) → `glam`.
fn quat(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[1], q[2], q[3], q[0]).normalize()
}

/// Значение между соседними ключами, линейно; до первого и после последнего — крайнее.
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

/// Поворот из трёх углов Эйлера NIF: вокруг X, затем Y, затем Z (порядок сверен на клипах
/// существ: первый кадр `idle` против позы скелета, `real_creature_tapes`).
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

/// Поза кости в момент `time` клипа: канал без ключей стоит в позе интерполятора, а если и она
/// не задана — в позе скелета.
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

/// Откуда отрезок ленты.
#[derive(Clone, Copy)]
enum Source<'a> {
    Clip(FolderClip<'a>),
    /// Своё падение и подъём.
    Fall,
}

struct Span<'a> {
    start: f32,
    duration: f32,
    source: Source<'a>,
    /// Группа поворота: поворот корня накопления замирает на первом кадре.
    hold_root_rotation: bool,
    /// Корень накопления стоит на месте: на отрезке только группы видов [`pins_root`].
    pin_root: bool,
    /// На отрезке есть петля: конец сводится к позе начала.
    close_loop: bool,
}

/// Вид группы, в которой корень накопления не ходит по земле (у ванили так устроены удары,
/// заклинания, повороты и реакция на удар).
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

/// Имя последовательности называет действие: точно (`Forward`) или с хвостом-вариантом после
/// подчёркивания (`AttackLeft_Cross`, `Idle_HandtoHand`, `CastSelf_A`).
fn names_action(clip_name: &str, action: &str) -> bool {
    let name = clip_name.to_ascii_lowercase();
    name == action
        || name
            .strip_prefix(action)
            .is_some_and(|tail| tail.starts_with('_'))
}

/// Клип для правила: первое действие и первая стойка, для которых в папке есть файл. Из
/// нескольких подходящих — сначала точное имя действия, потом по имени файла.
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

/// Времена ключей отрезка длиной `duration`: с шагом и в самом конце.
fn key_times(duration: f32, step: f32) -> Vec<f32> {
    let mut out = Vec::new();
    let mut k = 0u32;
    loop {
        #[expect(
            clippy::cast_precision_loss,
            reason = "число ключей одного клипа — сотни"
        )]
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

/// Время метки `Hit` клипа: строка, начинающаяся с `hit` (бывает `Hit.`, `Hit -at xy`).
fn hit_time(clip: &Clip) -> Option<f32> {
    clip.text_keys.iter().find_map(|(time, text)| {
        text.lines()
            .any(|line| line.trim().to_ascii_lowercase().starts_with("hit"))
            .then_some(*time)
    })
}

/// Поворот тела при падении на угол `angle`: вокруг оси вперёд (Y), проходящей по земле на
/// `half_width` сбоку, — тело опрокидывается через свой край.
fn fall(angle: f32, half_width: f32) -> Affine3A {
    let pivot = Vec3::new(half_width, 0.0, 0.0);
    Affine3A::from_translation(pivot)
        * Affine3A::from_rotation_y(angle)
        * Affine3A::from_translation(-pivot)
}

/// Угол падения в момент `time` отрезка: секунда ускоряющегося падения, секунда подъёма.
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

/// Убирает ключи, равные обоим соседям: постоянный отрезок держат два ключа.
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

/// Строит ленту существа. `skeleton` — файл скелета (тот же, из которого собрано тело), `clips` —
/// все клипы его папки, `rules` — [`groups::rules`], `step` — шаг ключей ([`KEY_STEP`]). `side` —
/// как далеко стоящее тело выступает в сторону падения ([`standing_side`]); `None` — половина
/// ширины `BSBound`.
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

    // Отрезки ленты и метки групп.
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
    // Сколько групп, занимающих ячейку открытых, уже лежит на отрезке (по номеру в `spans`).
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
        // Шестая группа на том же отрезке не пишется: открытых разом — не больше пяти. Группа,
        // которая уже лежит на ленте (второй вид `SpellCast`), ячейку не занимает заново.
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
                // Метки самого клипа — один раз на отрезок, сколько бы групп его ни делило.
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
        // Клип, который делит с ударом группа с ходящим корнем, корень не держит.
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

    // Дорожки клипов по имени кости; цели вне скелета считаются и отбрасываются.
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

    // Поза, из которой существо падает: первый кадр `Idle`, а без него — поза скелета.
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
    // Точка на земле, из которой начинается каждая группа.
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
                            // Путь по земле — от точки, где корень стоит в простое; у группы с
                            // корнем на месте — сама эта точка.
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
            // Насколько поза конца петли отстоит от позы начала: эту разницу добирают последние
            // ключи отрезка.
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
                        // Путь корня по земле — движение существа, его петля не замыкает.
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
                // Соседние ключи — по одну сторону сферы: движок интерполирует их напрямую.
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

    // Метки: по времени, одновременные — одним ключом в несколько строк.
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

/// Сколько групп ленты открыто разом в худший момент — так, как их считает движок: группа
/// занимает ячейку на первой своей метке и освобождает на `Stop`; `SpellCast` и метки звуков не
/// считаются. Больше [`MAX_OPEN_GROUPS`] — предупреждение движка.
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

/// Значение ключей в момент `time`: между соседними — `mix`, за краями — крайнее.
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

/// Как далеко тело, стоящее в первом кадре `Idle` ленты, выступает в сторону +X от оси модели:
/// через этот край оно опрокидывается при падении. Меряется по самому мешу — кожа
/// деформируется в позу кадра: габариты `BSBound` уже тела с расставленными лапами, и труп,
/// повёрнутый вокруг их края, уходил бы в землю. `None` — в теле нет форм.
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

/// Строит ленту для собранного тела и вешает её на него: сначала черновик, по нему — край
/// стоящего тела ([`standing_side`]), и с ним — лента, в которой труп ложится на землю.
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

/// Узлы потока по имени в нижнем регистре (первый с таким именем).
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

/// Вешает ленту на тело: контроллер ключей на каждую кость с дорожкой и метки на корень
/// накопления — так устроен обычный `<имя>.nif` ванильного существа. Возвращает число
/// повешенных дорожек.
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

/// Пара файлов ванильного существа: `x<имя>.nif` и `x<имя>.kf`.
#[derive(Debug)]
pub struct Pair {
    /// Тот же меш без контроллеров ключей и без меток.
    pub skeleton: NiStream,
    /// `NiSequenceStreamHelper`: метки, имена костей строками и контроллеры с пустой целью.
    pub keys: NiStream,
}

/// Делит полный меш с лентой ([`apply_tape`]) на пару, какую движок берёт вместо него.
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

    // `.kf`: цепочки строятся с конца, чтобы у каждого блока уже была ссылка на следующий.
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
                    // Длина строки и её `u32`-счётчик — так пишет ваниль (9 у `Bip01`).
                    bytes_remaining: u32::try_from(track.bone.len() + 4)
                        .expect("имя кости — короткая строка"),
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
