//! Двери (T2.6): что и как двигает последовательность `Open` меша Oblivion.
//!
//! У Oblivion дверь открывается анимацией в меше: `Open` поворачивает (или сдвигает) **узел**
//! створки, часто смещённый от начала координат меша. У Morrowind движения в меше нет: движок
//! поворачивает саму ссылку на 90° вокруг вертикали, ось — начало координат меша
//! (`docs/ru/formats/tes3.md`, «Двери»). Поэтому дверь Oblivion в позе файла Morrowind повернёт
//! вокруг чужой точки, а раму, если она в том же меше, — вместе со створкой.
//!
//! Здесь — измерение: [`door_motion`] по ключам `Open` (`crate::read::motion`) находит створки,
//! их петли, ось и угол в кадре меша и считает, сколько геометрии стоит на месте. Правило
//! переноса — [`DoorMotion::hinge`]: одна створка на вертикальной петле — петлю можно унести в
//! начало координат; остальное (две створки, подъём, рама) Morrowind не выразит без решения о
//! структуре записей.

use std::collections::{BTreeSet, HashMap};

use tes3::nif::NiNode;
use tes3::nif::glam::{Affine3A, Mat3A, Quat, Vec3, Vec3A};

use crate::geometry::Converted;
use crate::read::motion::{Rotations, Sequence, TransformKeys};
use crate::read::{Block, SourceNif};
use crate::skin::av_affine;

/// Окончание имени узла, которому Gamebryo отдаёт движение, не накопленное корнем.
pub const NON_ACCUM: &str = " NonAccum";

/// Имя последовательности открытия (без учёта регистра).
pub const OPEN_SEQUENCE: &str = "Open";

/// Поворот меньше этого — не поворот (шум ключей), радианы.
pub const MIN_TURN: f32 = 0.1;

/// Сдвиг меньше этого — не сдвиг, юниты.
pub const MIN_SLIDE: f32 = 4.0;

/// Сдвиг поворачивающейся створки меньше этого — поправка к повороту (калитка `FenceGate`
/// садится на 7 юнитов), а не отдельное движение, юниты.
pub const BIG_SLIDE: f32 = 32.0;

/// Ось считается вертикальной, если |z| оси не меньше этого.
pub const VERTICAL: f32 = 0.9;

/// Петля ближе этого к началу координат меша уже стоит там, где её ждёт Morrowind, юниты.
pub const AT_ORIGIN: f32 = 1.0;

/// Узел, который двигает `Open`.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaf {
    pub node: String,
    /// Блок узла в источнике.
    pub index: usize,
    /// Начало координат узла в закрытом положении, в кадре меша: через него проходит ось петли.
    pub pivot: Vec3,
    /// Ось поворота от закрытого к открытому в кадре меша (единичная; нулевая — поворота нет).
    pub axis: Vec3,
    /// Угол поворота вокруг `axis`, радианы, 0…π.
    pub angle: f32,
    /// Сдвиг узла от закрытого к открытому в кадре меша.
    pub slide: Vec3,
    /// Насколько поза файла отличается от первого ключа `Open`, радианы: 0 — файл лежит в
    /// закрытом положении.
    pub pose_gap: f32,
    /// Форм под узлом.
    pub shapes: usize,
}

impl Leaf {
    /// Створка поворачивается (а не только сдвигается).
    pub fn turns(&self) -> bool {
        self.angle >= MIN_TURN
    }

    /// Створка сдвигается: подъёмная решётка, раздвижная дверь. У поворачивающейся створки
    /// считается только большой сдвиг.
    pub fn slides(&self) -> bool {
        self.slide.length() >= if self.turns() { BIG_SLIDE } else { MIN_SLIDE }
    }

    /// Петля вертикальна — такой поворот Morrowind умеет.
    pub fn vertical(&self) -> bool {
        self.axis.z.abs() >= VERTICAL
    }

    /// Угол вокруг +Z меша со знаком, радианы; у невертикальной оси — 0.
    pub fn turn_about_z(&self) -> f32 {
        if self.vertical() {
            self.angle * self.axis.z.signum()
        } else {
            0.0
        }
    }
}

/// Движение двери по её `Open`.
#[derive(Debug, Clone, PartialEq)]
pub struct DoorMotion {
    /// Длительность `Open`, секунды.
    pub duration: f32,
    /// Узлы, которые `Open` поворачивает или сдвигает, в порядке блоков управления. Только
    /// верхние: узел под другим движущимся узлом (ручка на створке) — часть той же створки.
    pub leaves: Vec<Leaf>,
    /// Движущихся узлов под другими движущимися.
    pub nested: usize,
    /// Формы под движущимися узлами и вне их (рама, порог, стена).
    pub moving_shapes: usize,
    pub fixed_shapes: usize,
}

/// Что Morrowind сможет сделать с дверью.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DoorKind {
    /// `Open` ничего не двигает (или его нет): дверь — как статика.
    Still,
    /// Одна створка на вертикальной петле без сдвига — поворот Morrowind её выражает.
    Hinged,
    /// Несколько створок: у Morrowind ось одна.
    Leaves,
    /// Сдвиг (решётка, раздвижная): у Morrowind только поворот.
    Sliding,
    /// Поворот вокруг невертикальной оси (люк, подъёмный мост).
    Tilting,
}

/// Что перенос делает с дверью — для отчёта и переписи.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DoorFate {
    /// В меше нет `Open`: двери Oblivion открывать нечем (телепорт или закрыта навсегда).
    NoOpen,
    /// `Open` есть, но ничего не двигает.
    Still,
    /// Одна створка, петля уже в начале координат меша: Morrowind повернёт как надо.
    AtOrigin,
    /// Одна створка на смещённой петле, рамы нет: начало координат меша переносится в петлю.
    Rehinged,
    /// Одна створка, но в меше есть неподвижная геометрия: Morrowind повернёт её вместе со
    /// створкой.
    Framed,
    /// Несколько створок.
    Leaves,
    /// Сдвиг: решётка, раздвижная дверь.
    Sliding,
    /// Поворот вокруг невертикальной оси.
    Tilting,
}

/// Участь двери по её движению; `None` — последовательности `Open` нет.
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
    /// Створка на петле открывается против часовой стрелки (если смотреть сверху), а Morrowind
    /// поворачивает дверь только по часовой: такая дверь откроется в другую сторону.
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

    /// Петля единственной створки, если её можно унести в начало координат меша: одна створка на
    /// вертикальной петле, а вне её в меше ничего не стоит (иначе повернулась бы и рама). `None`
    /// — дверь другого вида или петля уже в начале координат.
    pub fn hinge(&self) -> Option<Vec3> {
        if self.kind() != DoorKind::Hinged || self.fixed_shapes > 0 {
            return None;
        }
        let pivot = self.leaves[0].pivot;
        (pivot.truncate().length() >= AT_ORIGIN).then_some(Vec3::new(pivot.x, pivot.y, 0.0))
    }
}

/// Кватернион NIF (`w, x, y, z`) → `glam`.
fn quat(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[1], q[2], q[3], q[0]).normalize()
}

/// Поворот без масштаба.
fn rotation_of(a: &Affine3A) -> Quat {
    Quat::from_mat3a(&Mat3A::from_cols(
        a.matrix3.x_axis.normalize_or_zero(),
        a.matrix3.y_axis.normalize_or_zero(),
        a.matrix3.z_axis.normalize_or_zero(),
    ))
    .normalize()
}

/// Первый и последний поворот дорожки. Углы Эйлера NIF — повороты вокруг X, Y, Z по очереди;
/// у дверей дорожка, как правило, одна, и порядок на результат не влияет.
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

/// Движение двери: `None` — последовательности `Open` в меше нет.
pub fn door_motion(source: &SourceNif, sequences: &[Sequence]) -> Option<DoorMotion> {
    let open = sequences
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(OPEN_SEQUENCE))?;

    // Обход дерева: мировая поза родителя и сам блок, по имени.
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
        // Кадр, в котором записаны ключи. Обычно это родитель узла. У узла `… NonAccum` — нет:
        // Gamebryo делит движение между корнем накопления (родителем) и этим узлом, и в ключах
        // узла лежит всё преобразование пары, а корню остаётся единичное (у `DoorLowerINT03`
        // ключи Z узла начинаются с −π — поворота его родителя в файле). Их кадр — родитель
        // корня накопления.
        let frame = match parent.filter(|_| track.node.ends_with(NON_ACCUM)) {
            Some(accum_root) => parents[&accum_root].1,
            None => parent_world,
        };
        let frame_turn = rotation_of(&frame);
        // Поза узла в файле в том же кадре — с ней сравнивается первый ключ.
        let file_turn = frame_turn.inverse() * rotation_of(&(parent_world * av_affine(av)));

        let ends = turn_ends(keys);
        let (axis, angle, pose_gap) = match ends {
            Some((first, last)) => {
                // Поворот от закрытого к открытому в кадре ключей → в кадр меша.
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
        // Петля — там, где узел стоит в файле. Поза интерполятора для этого не годится: у узла
        // `… NonAccum` она повторяет сдвиг узла-родителя (корня накопления), и с ней петля двери
        // Деменции выходила вдвое дальше настоящей — (97.6, 5.8) вместо (48.8, 2.9), 2026-10-02.
        let pivot = parent_world.transform_point3a(Vec3A::from_array(av.translation));
        // Сдвиг — разность крайних ключей: она от кадра отсчёта ключей не зависит.
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

    // Узел под другим движущимся узлом — деталь той же створки.
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

    // Формы: под каким движущимся узлом (ближайшим сверху) стоит каждая.
    let moving: BTreeSet<usize> = leaves.iter().map(|l| l.index).collect();
    let (mut moving_shapes, mut fixed_shapes) = (0usize, 0usize);
    for &index in parents.keys() {
        let Some(Block::TriStrips(shape) | Block::TriShape(shape)) = source.blocks.get(index)
        else {
            continue;
        };
        // Геометрия только для редактора в игре не видна и рамой не считается.
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

/// Переносит начало координат готового меша в точку `hinge` (в кадре меша): всё, что стоит под
/// корнем, — геометрия, узлы, `RootCollisionNode` — сдвигается на `−hinge`. Сдвигаются дети
/// корня, а не сам корень: учитывает ли ванильный движок преобразование корневого узла у
/// размещённого объекта, не проверено, а сдвиг детей от этого не зависит.
///
/// Чтобы дверь осталась на месте, позицию каждой её ссылки надо сдвинуть на тот же вектор,
/// повёрнутый и растянутый как ссылка (`transform::exterior`).
pub fn rehinge(converted: &mut Converted, hinge: Vec3) {
    let stream = &mut converted.stream;
    for root in stream.roots.clone() {
        let Some(node) = stream.get_as::<_, NiNode>(root) else {
            continue;
        };
        // Точка меша x = t + s·R·x_корня, где R = (девять чисел файла)ᵀ: сдвиг в кадре корня —
        // Rᵀ·hinge / s, а Rᵀ — матрица из чисел файла как есть.
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
