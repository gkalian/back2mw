//! Частицы: `NiParticleSystem` Oblivion → эмиттер Morrowind (T2.7).
//!
//! У Oblivion система частиц — узел сцены со списком воздействий `NiPSys*` (эмиттер — одно из
//! них) и контроллерами; у Morrowind — геометрия `NiParticles` под `NiBSParticleNode`, один
//! контроллер `NiParticleSystemController` с числами эмиттера и цепочка воздействий на нём.
//! Числа эмиттера у двух поколений Gamebryo значат одно и то же (скорость, склонение от +Z и
//! угол вокруг Z с разбросами, жизнь, радиус частицы, темп рождения), поэтому переносятся как
//! есть; устройство выхода снято с ванили (`tests/real_vanilla_particles.rs`, 577 систем
//! `Morrowind.bsa`; `docs/ru/formats/tes3.md`, «Частицы»):
//!
//! ```text
//! узел-родитель
//! ├─ NiNode «… Emitter»            пустой, с меткой sgoKeep: кадр эмиттера
//! └─ NiBSParticleNode              флаги 0xAA (0x2A — частицы в мире), NiZBufferProperty 1, sgoKeep
//!    └─ NiAutoNormalParticles      (NiRotatingParticles, если частицы вращаются)
//!       ├─ свойства: текстура, альфа, материал
//!       ├─ данные: пул вершин, размеры, (цвета — если есть цвет по возрасту)
//!       └─ NiParticleSystemController → воздействия: тяготение → вращение → цвет → рост и затухание
//! ```
//!
//! Что не выражается и теряется ([`Loss`]): форма объёма (цилиндр и сфера становятся коробкой),
//! кривая темпа рождения (остаётся средний темп), турбулентность тяготения, сопротивление,
//! ветер, коллайдеры, скорость родителя. Что не переносится вовсе ([`Refusal`]): частицы-меши,
//! эмиттер-меш, система без эмиттера или без контроллера рождения.

use std::collections::HashMap;

use tes3::nif::glam::{Affine3A, Mat3, Quat, Vec3, Vec4};
use tes3::nif::{
    ForceType, NiAVObject, NiAutoNormalParticles, NiAutoNormalParticlesData, NiBSParticleNode,
    NiBound, NiColorData, NiColorKey, NiExtraData, NiGeometry, NiGeometryData, NiGravity,
    NiLinColKey, NiLink, NiNode, NiObjectNET, NiParticleColorModifier, NiParticleGrowFade,
    NiParticleModifier, NiParticleRotation, NiParticleSystemController, NiParticles,
    NiParticlesData, NiPerParticleData, NiProperty, NiRotatingParticles, NiRotatingParticlesData,
    NiStream, NiStringExtraData, NiTimeController, NiZBufferProperty,
};

use crate::read::psys::{EmitterShape, ModifierKind, ParticleSystem};
use crate::read::{Block, SourceNif};
use crate::skin::av_affine;

/// Системы частиц меша по номеру блока `NiParticleSystem`.
#[derive(Debug, Clone, Default)]
pub struct Systems {
    by_index: HashMap<usize, ParticleSystem>,
}

impl Systems {
    /// Читает системы вторым проходом. Не прочиталось — систем нет: меш переносится без
    /// частиц, как раньше.
    pub fn read(bytes: &[u8], source: &SourceNif, offsets: &[usize]) -> Self {
        Self {
            by_index: crate::read::psys::systems(bytes, source, offsets)
                .unwrap_or_default()
                .into_iter()
                .map(|s| (s.index, s))
                .collect(),
        }
    }

    /// Набор из уже прочитанных систем.
    pub fn from_systems(systems: Vec<ParticleSystem>) -> Self {
        Self {
            by_index: systems.into_iter().map(|s| (s.index, s)).collect(),
        }
    }

    pub fn get(&self, index: usize) -> Option<&ParticleSystem> {
        self.by_index.get(&index)
    }

    pub fn len(&self) -> usize {
        self.by_index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_index.is_empty()
    }
}

/// Почему система не переносится.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Refusal {
    /// Частицы — меши (`NiMeshParticleSystem`): у Morrowind частица — спрайт.
    MeshParticles,
    /// Среди воздействий нет эмиттера.
    NoEmitter,
    /// Эмиттер рождает частицы на поверхности меша, а меша нет (ссылки пусты или ведут не на
    /// геометрию).
    MeshEmitter,
    /// Нет `NiPSysEmitterCtlr` или темп рождения не задан: когда и сколько рождать, неизвестно.
    NoBirthRate,
    /// Частицы появляются по событию (последовательность действия или зажатая): ловушка,
    /// открытие врат, вспышка по скрипту. У Morrowind такой эмиттер нечем запустить, а
    /// зацикленный он горел бы всегда; в покое у Oblivion частиц нет — нет и у нас.
    Triggered,
    /// Пул пуст.
    EmptyPool,
}

/// Что потеряно при переносе.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Loss {
    /// Объём — цилиндр или сфера: у Morrowind только коробка, взята описанная.
    VolumeShape,
    /// Частицы рождались на поверхности меша: у Morrowind — в коробке вокруг него; скорость по
    /// нормалям меша стала скоростью вдоль оси эмиттера.
    MeshSurface,
    /// Темп рождения меняется во времени: взят средний.
    RateCurve,
    /// Турбулентность тяготения.
    Turbulence,
    /// Воздействие без пары: сопротивление, ветер, коллайдер, скорость родителя.
    Modifier(&'static str),
}

/// Тяготение Morrowind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gravity {
    pub strength: f32,
    pub decay: f32,
    /// Точечное (к `position`), иначе плоское (вдоль `direction`).
    pub point: bool,
    pub position: Vec3,
    pub direction: Vec3,
}

/// Числа эмиттера Morrowind для одной системы.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub name: String,
    /// Кадр системы и кадр эмиттера относительно узла-родителя системы.
    pub system_frame: Affine3A,
    pub emitter_frame: Affine3A,
    /// Частицы живут в мире, а не в кадре системы.
    pub world_space: bool,
    pub pool: u16,
    pub speed: f32,
    pub speed_variation: f32,
    pub declination: f32,
    pub declination_variation: f32,
    pub planar_angle: f32,
    pub planar_angle_variation: f32,
    pub color: Vec4,
    pub size: f32,
    pub lifespan: f32,
    pub lifespan_variation: f32,
    /// Размеры объёма эмиттера по X, Y, Z его кадра.
    pub volume: Vec3,
    pub birth_rate: f32,
    /// Окно работы контроллера и рождения, секунды.
    pub start: f32,
    pub stop: f32,
    /// Контроллер не зациклен (флаг «зажать»): эффект проигрывается один раз.
    pub once: bool,
    pub grow: Option<(f32, f32)>,
    pub gravity: Vec<Gravity>,
    /// Скорость вращения, случайная ли ось, ось.
    pub rotation: Option<(f32, bool, Vec3)>,
    /// Ключи цвета по доле жизни.
    pub color_keys: Vec<(f32, Vec4)>,
    /// Порождение при смерти: поколений, доля, множитель, разброс скорости и направления.
    pub spawn: Option<(u16, f32, u16, f32, f32)>,
    pub losses: Vec<Loss>,
}

/// Поправка силы тяготения: движок Morrowind ускоряет частицу в 1.6 раза сильнее, чем записано
/// в `NiGravity` (константа OpenMW, `GravityAffector::operate`, подобрана под ваниль; у нас не
/// проверено). Сила Oblivion — само ускорение, поэтому делится.
pub const GRAVITY_MAGIC: f32 = 1.6;

/// Самое короткое окно контроллера, секунды: у нулевого окна движку не из чего сделать цикл.
pub const MIN_WINDOW: f32 = 1.0 / 15.0;

/// Мировые позы узлов и форм источника и их родители.
fn poses(source: &SourceNif) -> HashMap<usize, (Option<usize>, Affine3A)> {
    let mut out: HashMap<usize, (Option<usize>, Affine3A)> = HashMap::new();
    let mut stack: Vec<(usize, Option<usize>, Affine3A)> = source
        .roots
        .iter()
        .filter_map(|r| r.index())
        .map(|i| (i, None, Affine3A::IDENTITY))
        .collect();
    while let Some((index, parent, parent_world)) = stack.pop() {
        if out.contains_key(&index) {
            continue;
        }
        let (av, children) = match source.blocks.get(index) {
            Some(Block::Node(node)) => (&node.av, node.children.as_slice()),
            Some(Block::TriStrips(shape) | Block::TriShape(shape)) => (&shape.av, [].as_slice()),
            _ => continue,
        };
        let world = parent_world * av_affine(av);
        out.insert(index, (parent, world));
        for c in children.iter().filter_map(|r| r.index()) {
            stack.push((c, Some(index), world));
        }
    }
    out
}

/// Коробка вокруг мешей-источников эмиттера: кадр первого меша со сдвигом в центр коробки и её
/// размеры по осям этого кадра. `None` — геометрии нет.
fn mesh_box(
    source: &SourceNif,
    world: &HashMap<usize, (Option<usize>, Affine3A)>,
    meshes: &[crate::read::BlockRef],
) -> Option<(Affine3A, Vec3)> {
    let mut frame: Option<Affine3A> = None;
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for index in meshes.iter().filter_map(|m| m.index()) {
        let Some(Block::TriStrips(shape) | Block::TriShape(shape)) = source.blocks.get(index)
        else {
            continue;
        };
        let Some(Block::TriStripsData(data) | Block::TriShapeData(data)) = source.block(shape.data)
        else {
            continue;
        };
        let Some((_, mesh_world)) = world.get(&index) else {
            continue;
        };
        let first = *frame.get_or_insert(*mesh_world);
        let to_first = first.inverse() * *mesh_world;
        for v in &data.vertices {
            let p = Vec3::from(to_first.transform_point3a(Vec3::from_array(*v).into()));
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let frame = frame.filter(|_| lo.x <= hi.x)?;
    let center = (lo + hi) / 2.0;
    Some((frame * Affine3A::from_translation(center), hi - lo))
}

/// Узел-родитель блока (системы частиц среди детей узлов).
fn parent_of(source: &SourceNif, child: usize) -> Option<usize> {
    source.blocks.iter().position(
        |b| matches!(b, Block::Node(n) if n.children.iter().any(|c| c.index() == Some(child))),
    )
}

/// Что эмиттер Morrowind сделает из системы Oblivion.
pub fn plan(source: &SourceNif, system: &ParticleSystem) -> Result<Plan, Refusal> {
    if system.mesh_particles {
        return Err(Refusal::MeshParticles);
    }
    let emitter = system.emitter.as_ref().ok_or(Refusal::NoEmitter)?;
    let emission = system.emission.as_ref().ok_or(Refusal::NoBirthRate)?;
    if emission.triggered {
        return Err(Refusal::Triggered);
    }
    let world = poses(source);
    let mut losses = Vec::new();
    let mut mesh_frame = None;
    let volume = match emitter.shape {
        EmitterShape::Mesh => {
            let (frame, size) =
                mesh_box(source, &world, &emitter.meshes).ok_or(Refusal::MeshEmitter)?;
            mesh_frame = Some(frame);
            losses.push(Loss::MeshSurface);
            size
        }
        EmitterShape::Box {
            width,
            height,
            depth,
        } => Vec3::new(width, height, depth),
        EmitterShape::Cylinder { radius, height } => {
            if radius > 0.0 {
                losses.push(Loss::VolumeShape);
            }
            Vec3::new(2.0 * radius, 2.0 * radius, height)
        }
        EmitterShape::Sphere { radius } => {
            if radius > 0.0 {
                losses.push(Loss::VolumeShape);
            }
            Vec3::splat(2.0 * radius)
        }
    };
    let birth_rate = emission
        .mean_rate()
        .filter(|r| *r > 0.0)
        .ok_or(Refusal::NoBirthRate)?;
    if system.pool == 0 {
        return Err(Refusal::EmptyPool);
    }
    let (lo, hi) = emission
        .rate_keys
        .iter()
        .fold((f32::MAX, f32::MIN), |a, k| (a.0.min(k.1), a.1.max(k.1)));
    if emission.rate_keys.len() > 1 && hi - lo > 1e-3 {
        losses.push(Loss::RateCurve);
    }

    // Кадры: родитель системы, сама система, узел эмиттера.
    let parent = parent_of(source, system.index);
    let parent_world = parent
        .and_then(|p| world.get(&p))
        .map_or(Affine3A::IDENTITY, |p| p.1);
    let system_world = parent_world * av_affine(&system.av);
    let frame_of = |object: crate::read::BlockRef| {
        object
            .index()
            .and_then(|i| world.get(&i))
            .map_or(system_world, |p| p.1)
    };
    let emitter_frame =
        parent_world.inverse() * mesh_frame.unwrap_or_else(|| frame_of(emitter.object));
    // У эмиттера-меша направление задаёт тип скорости: вдоль оси — склонение и угол этой оси,
    // случайно — во все стороны, по нормалям — как записано у эмиттера (ось +Z его кадра).
    let (declination, declination_variation, planar_angle, planar_angle_variation) =
        match (emitter.shape, emitter.velocity_type) {
            (EmitterShape::Mesh, 2) => {
                let axis = Vec3::from_array(emitter.emission_axis).normalize_or_zero();
                (
                    axis.z.clamp(-1.0, 1.0).acos(),
                    emitter.declination_variation,
                    axis.y.atan2(axis.x),
                    emitter.planar_angle_variation,
                )
            }
            (EmitterShape::Mesh, 1) => (0.0, std::f32::consts::PI, 0.0, std::f32::consts::TAU),
            _ => (
                emitter.declination,
                emitter.declination_variation,
                emitter.planar_angle,
                emitter.planar_angle_variation,
            ),
        };

    // Темп выше, чем пул успевает освобождать, ничего не добавляет: больше пула частиц не
    // будет. Срез держит число в разумных пределах (у ванили до 1 500, у Oblivion встречается
    // 10 миллионов).
    let shortest_life =
        (emitter.life_span - emitter.life_span_variation.abs() / 2.0).max(MIN_WINDOW);
    let birth_rate = birth_rate.min(f32::from(system.pool) / shortest_life);

    let mut plan = Plan {
        name: crate::node_name(&system.av.net.name),
        system_frame: av_affine(&system.av),
        emitter_frame,
        world_space: system.world_space,
        pool: system.pool,
        speed: emitter.speed,
        speed_variation: emitter.speed_variation,
        declination,
        declination_variation,
        planar_angle,
        planar_angle_variation,
        color: Vec4::from_array(emitter.color),
        size: emitter.radius,
        lifespan: emitter.life_span,
        lifespan_variation: emitter.life_span_variation,
        volume,
        birth_rate,
        start: emission.start,
        stop: emission.stop.max(emission.start + MIN_WINDOW),
        once: emission.once,
        grow: None,
        gravity: Vec::new(),
        rotation: None,
        color_keys: Vec::new(),
        spawn: None,
        losses,
    };

    let mut spawn_on_death = false;
    let mut spawn = None;
    for m in system.modifiers.iter().filter(|m| m.active) {
        match &m.kind {
            ModifierKind::GrowFade { grow, fade } => plan.grow = Some((*grow, *fade)),
            ModifierKind::Gravity {
                object,
                axis,
                decay,
                strength,
                force_type,
                turbulence,
            } => {
                if *turbulence != 0.0 {
                    plan.losses.push(Loss::Turbulence);
                }
                // Ось и точка тяготения — в кадре своего объекта; у Morrowind — в кадре частиц.
                let to_system = system_world.inverse() * frame_of(*object);
                plan.gravity.push(Gravity {
                    strength: *strength / GRAVITY_MAGIC,
                    decay: *decay,
                    point: *force_type == 1,
                    position: Vec3::from(to_system.translation),
                    direction: Vec3::from(
                        to_system.transform_vector3a(Vec3::from_array(*axis).into()),
                    )
                    .normalize_or_zero(),
                });
            }
            ModifierKind::Rotation {
                speed,
                random_axis,
                axis,
            } => plan.rotation = Some((*speed, *random_axis, Vec3::from_array(*axis))),
            ModifierKind::Color { keys } => {
                plan.color_keys = keys
                    .iter()
                    .map(|(t, c)| (*t, Vec4::from_array(*c)))
                    .collect();
            }
            ModifierKind::AgeDeath {
                spawn_on_death: yes,
            } => spawn_on_death |= *yes,
            ModifierKind::Spawn {
                generations,
                percentage,
                max,
                speed_chaos,
                direction_chaos,
                ..
            } => {
                spawn = Some((
                    *generations,
                    *percentage,
                    (*max).max(1),
                    *speed_chaos,
                    *direction_chaos,
                ));
            }
            ModifierKind::Housekeeping => {}
            ModifierKind::Other(name) => plan.losses.push(Loss::Modifier(name)),
        }
    }
    if spawn_on_death {
        plan.spawn = spawn;
    }
    plan.losses.sort();
    plan.losses.dedup();
    Ok(plan)
}

/// Радиус ограничивающей сферы системы в её кадре: дальше частица не улетит.
pub fn reach(plan: &Plan) -> f32 {
    let speed = plan.speed + plan.speed_variation.abs() / 2.0;
    let life = plan.lifespan + plan.lifespan_variation.abs() / 2.0;
    speed.abs() * life.max(0.0) + plan.volume.length() / 2.0 + plan.size.abs()
}

/// Флаги узла частиц у ванили: 0x20 — играть сразу, 0x80 — частицы в кадре узла (имена битов —
/// по OpenMW); 0x0A — как у прочих узлов. У костра `light_fire.nif` — 0xAA.
pub const PARTICLE_NODE_FLAGS: u16 = 0x00AA;
/// То же без «в кадре узла»: частицы остаются в мире, когда эмиттер уходит.
pub const PARTICLE_NODE_WORLD_FLAGS: u16 = 0x002A;
/// Флаги узла-эмиттера (костёр, 123 из 577 ванильных систем).
pub const EMITTER_NODE_FLAGS: u16 = 0x00A2;
/// Флаги геометрии частиц (501 из 577).
pub const PARTICLES_FLAGS: u16 = 0x0002;
/// Флаги контроллера: активен, цикл — петля (501 из 577).
pub const CONTROLLER_LOOP: u16 = 0x0008;
/// Активен, цикл зажат: эффект проигрывается один раз (72 из 577).
pub const CONTROLLER_ONCE: u16 = 0x000C;
/// `NiZBufferProperty` узла частиц: проверять глубину, не писать (у всех 577).
pub const Z_TEST_ONLY: u16 = 0x0001;
/// Метка узла для оптимизатора сцены движка: «не сливать» (у 513 из 577 узлов частиц).
pub const KEEP_NODE: &str = "sgoKeep";

/// `NiAVObject` с кадром `frame`: поворот без масштаба, масштаб — длина первой оси.
fn av_object(
    name: String,
    frame: &Affine3A,
    flags: u16,
    properties: Vec<NiLink<NiProperty>>,
    extra_data: NiLink<NiExtraData>,
) -> NiAVObject {
    let scale = frame.matrix3.x_axis.length();
    let scale = if scale > 0.0 { scale } else { 1.0 };
    NiAVObject {
        base: NiObjectNET {
            name,
            extra_data,
            ..NiObjectNET::default()
        },
        flags,
        translation: Vec3::from(frame.translation),
        // В файле матрица лежит по строкам — транспонированной к `glam` (см. `skin::affine`).
        rotation: (Mat3::from(frame.matrix3) * (1.0 / scale)).transpose(),
        scale,
        properties,
        ..NiAVObject::default()
    }
}

fn keep_mark(stream: &mut NiStream) -> NiLink<NiExtraData> {
    stream
        .insert(NiStringExtraData {
            base: NiExtraData {
                // Длина строки и её `u32`-счётчик — так пишет ваниль (11 у `sgoKeep`).
                bytes_remaining: u32::try_from(KEEP_NODE.len() + 4).expect("короткая строка"),
                ..NiExtraData::default()
            },
            value: KEEP_NODE.to_owned(),
        })
        .cast()
}

/// Строит эмиттер Morrowind по плану. `properties` — уже сконвертированные свойства системы
/// (текстура, альфа, материал). Возвращает двух детей для узла-родителя: узел-эмиттер и узел
/// частиц.
pub fn build(
    stream: &mut NiStream,
    plan: &Plan,
    properties: Vec<NiLink<NiProperty>>,
) -> [NiLink<NiAVObject>; 2] {
    let pool = usize::from(plan.pool);

    // Узел-эмиттер: пустой, в его кадре рождаются частицы.
    let mark = keep_mark(stream);
    let emitter = stream.insert(NiNode {
        base: av_object(
            format!("{} Emitter", plan.name),
            &plan.emitter_frame,
            EMITTER_NODE_FLAGS,
            Vec::new(),
            mark,
        ),
        ..NiNode::default()
    });

    // Данные: пул вершин и размеров; цвета — только если цвет меняется с возрастом.
    let geometry = NiGeometryData {
        vertices: vec![Vec3::ZERO; pool],
        bound: NiBound {
            center: Vec3::ZERO,
            radius: reach(plan),
        },
        vertex_colors: if plan.color_keys.is_empty() {
            Vec::new()
        } else {
            vec![plan.color; pool]
        },
        ..NiGeometryData::default()
    };
    let particles_data = NiParticlesData {
        base: geometry,
        num_particles: plan.pool,
        particle_radius: plan.size,
        num_active: 0,
        sizes: vec![1.0; pool],
    };
    let particles_av = av_object(
        plan.name.clone(),
        &Affine3A::IDENTITY,
        PARTICLES_FLAGS,
        properties,
        NiLink::null(),
    );
    // Вращающиеся частицы — другой тип геометрии с кватернионами в данных.
    let particles: NiLink<NiAVObject> = if plan.rotation.is_some() {
        let data = stream.insert(NiRotatingParticlesData {
            base: particles_data,
            rotations: vec![Quat::IDENTITY; pool],
        });
        stream
            .insert(NiRotatingParticles {
                base: NiParticles {
                    base: NiGeometry {
                        base: particles_av,
                        geometry_data: data.cast(),
                        ..NiGeometry::default()
                    },
                },
            })
            .cast()
    } else {
        let data = stream.insert(NiAutoNormalParticlesData {
            base: particles_data,
        });
        stream
            .insert(NiAutoNormalParticles {
                base: NiParticles {
                    base: NiGeometry {
                        base: particles_av,
                        geometry_data: data.cast(),
                        ..NiGeometry::default()
                    },
                },
            })
            .cast()
    };

    let (spawn_generations, spawn_percentage, spawn_multiplier, speed_chaos, direction_chaos) =
        plan.spawn.unwrap_or((0, 1.0, 1, 0.0, 0.0));
    let controller = stream.insert(NiParticleSystemController {
        base: NiTimeController {
            flags: if plan.once {
                CONTROLLER_ONCE
            } else {
                CONTROLLER_LOOP
            },
            frequency: 1.0,
            phase: 0.0,
            start_time: plan.start,
            stop_time: plan.stop,
            target: particles.cast(),
            ..NiTimeController::default()
        },
        speed: plan.speed,
        speed_variation: plan.speed_variation,
        declination_angle: plan.declination,
        declination_variation: plan.declination_variation,
        planar_angle: plan.planar_angle,
        planar_angle_variation: plan.planar_angle_variation,
        initial_normal: Vec3::X,
        initial_color: plan.color,
        initial_size: plan.size,
        emit_start_time: plan.start,
        emit_stop_time: plan.stop,
        reset_particle_system: false,
        birth_rate: plan.birth_rate,
        lifespan: plan.lifespan,
        lifespan_variation: plan.lifespan_variation,
        use_birth_rate: true,
        spawn_on_death: plan.spawn.is_some(),
        emitter_width: plan.volume.x,
        emitter_height: plan.volume.y,
        emitter_depth: plan.volume.z,
        emitter: emitter.cast(),
        spawn_generations,
        spawn_percentage,
        spawn_multiplier,
        spawned_speed_chaos: speed_chaos,
        spawned_direction_chaos: direction_chaos,
        particles: vec![NiPerParticleData::default(); pool],
        num_active_particles: 0,
        ..NiParticleSystemController::default()
    });
    if let Some(av) = stream.get_mut(particles) {
        av.controller = controller.cast();
    }

    // Воздействия — цепочкой с конца: тяготение → вращение → цвет → рост и затухание, как в
    // ванильных цепочках.
    let mut next: NiLink<NiParticleModifier> = NiLink::null();
    let base = |next: NiLink<NiParticleModifier>| NiParticleModifier {
        next,
        controller,
        ..NiParticleModifier::default()
    };
    if let Some((grow, fade)) = plan.grow {
        next = stream
            .insert(NiParticleGrowFade {
                base: base(next),
                grow_time: grow,
                fade_time: fade,
            })
            .cast();
    }
    if !plan.color_keys.is_empty() {
        let color_data = stream.insert(NiColorData {
            keys: NiColorKey::LinKey(
                plan.color_keys
                    .iter()
                    .map(|&(time, value)| NiLinColKey { time, value })
                    .collect(),
            ),
            ..NiColorData::default()
        });
        next = stream
            .insert(NiParticleColorModifier {
                base: base(next),
                color_data,
            })
            .cast();
    }
    if let Some((speed, random_axis, axis)) = plan.rotation {
        next = stream
            .insert(NiParticleRotation {
                base: base(next),
                random_initial_axis: random_axis,
                initial_axis: axis,
                rotation_speed: speed,
            })
            .cast();
    }
    for g in plan.gravity.iter().rev() {
        next = stream
            .insert(NiGravity {
                base: base(next),
                decay: g.decay,
                strength: g.strength,
                force_type: if g.point {
                    ForceType::Spherical
                } else {
                    ForceType::Planar
                },
                position: g.position,
                direction: g.direction,
            })
            .cast();
    }
    if let Some(c) = stream.get_mut(controller) {
        c.particle_modifier = next;
    }

    // Узел частиц: глубина проверяется, но не пишется — частицы не закрывают друг друга.
    let z_buffer = stream.insert(NiZBufferProperty {
        base: NiProperty {
            flags: Z_TEST_ONLY,
            ..NiProperty::default()
        },
    });
    let mark = keep_mark(stream);
    let node = stream.insert(NiBSParticleNode {
        base: NiNode {
            base: av_object(
                plan.name.clone(),
                &plan.system_frame,
                if plan.world_space {
                    PARTICLE_NODE_WORLD_FLAGS
                } else {
                    PARTICLE_NODE_FLAGS
                },
                vec![z_buffer.cast()],
                mark,
            ),
            children: vec![particles],
            ..NiNode::default()
        },
    });
    [emitter.cast(), node.cast()]
}

#[cfg(test)]
mod tests;
