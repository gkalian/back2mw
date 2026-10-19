use std::f32::consts::PI;

use super::*;
use crate::read::common::{AvObject, BlockRef, ObjectNet};
use crate::read::header::NifHeader;
use crate::read::psys::{Emission, Emitter, Modifier};
use crate::read::{Ignored, NiNode as SrcNode, NiTriStrips, NiTriStripsData};

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
/// Поворот на +90° вокруг Z, девять чисел в порядке файла (по строкам).
const TURN_Z: [f32; 9] = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

fn av(name: &str, translation: [f32; 3], rotation: [f32; 9]) -> AvObject {
    AvObject {
        net: ObjectNet {
            name: name.to_owned(),
            extra_data: Vec::new(),
            controller: BlockRef::NONE,
        },
        flags: 0,
        translation,
        rotation,
        scale: 1.0,
        properties: Vec::new(),
        collision_object: BlockRef::NONE,
    }
}

fn node(name: &str, translation: [f32; 3], rotation: [f32; 9], children: &[u32]) -> Block {
    Block::Node(SrcNode {
        av: av(name, translation, rotation),
        children: children.iter().map(|&i| BlockRef(Some(i))).collect(),
        effects: Vec::new(),
    })
}

fn particle_block() -> Block {
    Block::Ignored(Ignored {
        type_name: "NiParticleSystem",
    })
}

fn nif(blocks: Vec<Block>) -> SourceNif {
    SourceNif {
        header: NifHeader {
            line: String::new(),
            version: 0x1400_0004,
            user_version: 11,
            user_version_2: 11,
            export_info: Default::default(),
            block_types: Vec::new(),
            block_type_index: Vec::new(),
            groups: Vec::new(),
            body_offset: 0,
        },
        blocks,
        roots: vec![BlockRef(Some(0))],
    }
}

/// Сцена факела: корень → «Torch» (сдвинут) → узел-эмиттер и система частиц.
fn torch() -> SourceNif {
    nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1]),
        node("Torch", [10.0, 0.0, 50.0], TURN_Z, &[2, 3]),
        node("FireEmitter", [0.0, 4.0, 20.0], IDENTITY, &[]),
        particle_block(),
    ])
}

fn emitter(shape: EmitterShape, object: Option<u32>) -> Emitter {
    Emitter {
        name: "NiPSysBoxEmitter:0".to_owned(),
        active: true,
        speed: 20.0,
        speed_variation: 10.0,
        declination: 0.1,
        declination_variation: 0.28,
        planar_angle: 0.2,
        planar_angle_variation: PI,
        color: [1.0, 0.5, 0.25, 1.0],
        radius: 8.0,
        life_span: 1.5,
        life_span_variation: 0.5,
        object: BlockRef(object),
        shape,
        meshes: Vec::new(),
        velocity_type: 0,
        emission_axis: [0.0; 3],
    }
}

fn emission(rate: f32) -> Emission {
    Emission {
        flags: 0x0008,
        frequency: 1.0,
        phase: 0.0,
        start: 0.0,
        stop: 0.95,
        rate: Some(rate),
        rate_keys: Vec::new(),
        sequence: None,
        once: false,
        triggered: false,
    }
}

fn modifier(kind: ModifierKind) -> Modifier {
    Modifier {
        name: "m".to_owned(),
        active: true,
        kind,
    }
}

fn system(shape: EmitterShape, modifiers: Vec<Modifier>) -> ParticleSystem {
    ParticleSystem {
        index: 3,
        av: av("Fire", [1.0, 2.0, 3.0], IDENTITY),
        world_space: true,
        pool: 20,
        mesh_particles: false,
        emitter: Some(emitter(shape, Some(2))),
        emission: Some(emission(13.0)),
        modifiers,
    }
}

const POINT: EmitterShape = EmitterShape::Box {
    width: 0.0,
    height: 0.0,
    depth: 0.0,
};

fn close(a: Vec3, b: [f32; 3]) -> bool {
    (a - Vec3::from_array(b)).length() < 1e-3
}

/// Числа эмиттера переносятся как есть; кадр эмиттера — его узел относительно родителя системы.
#[test]
fn emitter_numbers_are_carried_over_and_the_frame_is_relative_to_the_parent() {
    let plan = plan(&torch(), &system(POINT, Vec::new())).unwrap();
    assert_eq!(plan.name, "Fire");
    assert_eq!(
        (plan.speed, plan.speed_variation, plan.size, plan.pool),
        (20.0, 10.0, 8.0, 20)
    );
    assert_eq!((plan.declination, plan.declination_variation), (0.1, 0.28));
    assert_eq!((plan.planar_angle, plan.planar_angle_variation), (0.2, PI));
    assert_eq!((plan.lifespan, plan.lifespan_variation), (1.5, 0.5));
    assert_eq!(plan.color, Vec4::new(1.0, 0.5, 0.25, 1.0));
    assert_eq!((plan.birth_rate, plan.start, plan.stop), (13.0, 0.0, 0.95));
    assert!(plan.world_space && !plan.once);
    assert_eq!(plan.volume, Vec3::ZERO);
    assert!(plan.losses.is_empty(), "{:?}", plan.losses);
    // Узел-эмиттер — сосед системы под «Torch»: его кадр относительно «Torch» — его же сдвиг.
    assert!(close(
        plan.emitter_frame.translation.into(),
        [0.0, 4.0, 20.0]
    ));
    assert!(close(plan.system_frame.translation.into(), [1.0, 2.0, 3.0]));
}

/// Эмиттер без своего узла рождает частицы в кадре самой системы.
#[test]
fn an_emitter_without_an_object_sits_in_the_frame_of_the_system() {
    let mut s = system(POINT, Vec::new());
    s.emitter = Some(emitter(POINT, None));
    let plan = plan(&torch(), &s).unwrap();
    assert!(close(
        plan.emitter_frame.translation.into(),
        [1.0, 2.0, 3.0]
    ));
}

#[test]
fn cylinder_and_sphere_become_the_circumscribed_box_with_a_loss() {
    let cylinder = plan(
        &torch(),
        &system(
            EmitterShape::Cylinder {
                radius: 5.0,
                height: 30.0,
            },
            Vec::new(),
        ),
    )
    .unwrap();
    assert_eq!(cylinder.volume, Vec3::new(10.0, 10.0, 30.0));
    assert_eq!(cylinder.losses, [Loss::VolumeShape]);
    let sphere = plan(
        &torch(),
        &system(EmitterShape::Sphere { radius: 4.0 }, Vec::new()),
    )
    .unwrap();
    assert_eq!(sphere.volume, Vec3::splat(8.0));
    // Коробка — коробка; сфера нулевого радиуса — точка, потери нет.
    let boxed = plan(
        &torch(),
        &system(
            EmitterShape::Box {
                width: 1.0,
                height: 2.0,
                depth: 3.0,
            },
            Vec::new(),
        ),
    )
    .unwrap();
    assert_eq!(boxed.volume, Vec3::new(1.0, 2.0, 3.0));
    assert!(boxed.losses.is_empty());
    let dot = plan(
        &torch(),
        &system(EmitterShape::Sphere { radius: 0.0 }, Vec::new()),
    )
    .unwrap();
    assert!(dot.losses.is_empty());
}

/// Воздействия: рост и затухание, цвет, вращение — как есть; тяготение — в кадр системы и с
/// поправкой силы; порождение — только если включено у `AgeDeath`; без пары — в потери.
#[test]
fn modifiers_are_mapped_and_the_ones_without_a_counterpart_are_losses() {
    let modifiers = vec![
        modifier(ModifierKind::AgeDeath {
            spawn_on_death: true,
        }),
        modifier(ModifierKind::Spawn {
            generations: 2,
            percentage: 0.5,
            min: 1,
            max: 3,
            speed_chaos: 0.2,
            direction_chaos: 0.3,
        }),
        modifier(ModifierKind::GrowFade {
            grow: 0.15,
            fade: 1.35,
        }),
        modifier(ModifierKind::Color {
            keys: vec![(0.0, [1.0; 4]), (1.0, [1.0, 0.0, 0.0, 0.0])],
        }),
        modifier(ModifierKind::Rotation {
            speed: 2.0,
            random_axis: true,
            axis: [1.0, 0.0, 0.0],
        }),
        // Ось тяготения — в кадре узла «FireEmitter» (он под «Torch», как и система).
        modifier(ModifierKind::Gravity {
            object: BlockRef(Some(2)),
            axis: [0.0, 0.0, -2.0],
            decay: 0.5,
            strength: 16.0,
            force_type: 0,
            turbulence: 3.0,
        }),
        modifier(ModifierKind::Housekeeping),
        modifier(ModifierKind::Other("BSWindModifier")),
        // Выключенное воздействие не действует и потерей не считается.
        Modifier {
            name: "off".to_owned(),
            active: false,
            kind: ModifierKind::Other("NiPSysDragModifier"),
        },
    ];
    let plan = plan(&torch(), &system(POINT, modifiers)).unwrap();
    assert_eq!(plan.grow, Some((0.15, 1.35)));
    assert_eq!(plan.color_keys.len(), 2);
    assert_eq!(plan.rotation, Some((2.0, true, Vec3::X)));
    assert_eq!(plan.spawn, Some((2, 0.5, 3, 0.2, 0.3)));
    assert_eq!(plan.gravity.len(), 1);
    let g = plan.gravity[0];
    assert!((g.strength - 10.0).abs() < 1e-6, "16 / 1.6: {}", g.strength);
    assert!(!g.point && g.decay == 0.5);
    assert!(close(g.direction, [0.0, 0.0, -1.0]), "{:?}", g.direction);
    // Узел тяготения (0, 4, 20), система (1, 2, 3) — оба под «Torch»: разность.
    assert!(close(g.position, [-1.0, 2.0, 17.0]), "{:?}", g.position);
    assert_eq!(
        plan.losses,
        [Loss::Turbulence, Loss::Modifier("BSWindModifier")]
    );

    // Контроль: без флага у `AgeDeath` порождения нет.
    let no_spawn = super::plan(
        &torch(),
        &system(
            POINT,
            vec![modifier(ModifierKind::Spawn {
                generations: 2,
                percentage: 0.5,
                min: 1,
                max: 3,
                speed_chaos: 0.2,
                direction_chaos: 0.3,
            })],
        ),
    )
    .unwrap();
    assert_eq!(no_spawn.spawn, None);
}

#[test]
fn what_morrowind_cannot_start_or_draw_is_refused() {
    let mut meshes = system(POINT, Vec::new());
    meshes.mesh_particles = true;
    assert_eq!(plan(&torch(), &meshes), Err(Refusal::MeshParticles));

    let mut bare = system(POINT, Vec::new());
    bare.emitter = None;
    assert_eq!(plan(&torch(), &bare), Err(Refusal::NoEmitter));

    let mut silent = system(POINT, Vec::new());
    silent.emission = None;
    assert_eq!(plan(&torch(), &silent), Err(Refusal::NoBirthRate));
    silent.emission = Some(Emission {
        rate: None,
        ..emission(0.0)
    });
    assert_eq!(plan(&torch(), &silent), Err(Refusal::NoBirthRate));
    silent.emission = Some(emission(-30.0));
    assert_eq!(plan(&torch(), &silent), Err(Refusal::NoBirthRate));

    // Ловушка: темп задаёт последовательность действия — в покое частиц нет.
    let mut trap = system(POINT, Vec::new());
    trap.emission = Some(Emission {
        sequence: Some("Forward".to_owned()),
        triggered: true,
        ..emission(40.0)
    });
    assert_eq!(plan(&torch(), &trap), Err(Refusal::Triggered));

    let mut empty = system(POINT, Vec::new());
    empty.pool = 0;
    assert_eq!(plan(&torch(), &empty), Err(Refusal::EmptyPool));

    // Эмиттер-меш без геометрии.
    assert_eq!(
        plan(&torch(), &system(EmitterShape::Mesh, Vec::new())),
        Err(Refusal::MeshEmitter)
    );
}

/// Темп: кривая даёт средний с потерей; темп, который пул не успевает освобождать, срезается.
#[test]
fn birth_rate_is_the_mean_of_the_curve_and_never_outruns_the_pool() {
    let mut curved = system(POINT, Vec::new());
    curved.emission = Some(Emission {
        rate: None,
        rate_keys: vec![(0.0, 0.0), (1.0, 12.0), (3.0, 12.0)],
        stop: 3.0,
        ..emission(0.0)
    });
    let p = plan(&torch(), &curved).unwrap();
    // Площадь под кривой: 6 + 24, за 3 секунды.
    assert!((p.birth_rate - 10.0).abs() < 1e-4, "{}", p.birth_rate);
    assert_eq!(p.losses, [Loss::RateCurve]);
    assert_eq!(p.stop, 3.0);

    // Пул 20, самая короткая жизнь 1.5 − 0.25: больше 16 частиц в секунду не разместить.
    let mut flood = system(POINT, Vec::new());
    flood.emission = Some(emission(9_986_438.0));
    let p = plan(&torch(), &flood).unwrap();
    assert!((p.birth_rate - 16.0).abs() < 1e-4, "{}", p.birth_rate);

    // Окно нулевой длины растягивается до минимального.
    let mut instant = system(POINT, Vec::new());
    instant.emission = Some(Emission {
        start: 2.0,
        stop: 2.0,
        ..emission(5.0)
    });
    let p = plan(&torch(), &instant).unwrap();
    assert!((p.stop - (2.0 + MIN_WINDOW)).abs() < 1e-6);
}

/// Эмиттер-меш: коробка вокруг меша-источника в его кадре; ось эмиссии — в склонение и угол.
#[test]
fn a_mesh_emitter_becomes_a_box_around_its_mesh() {
    let data = NiTriStripsData {
        group_id: 0,
        num_vertices: 3,
        keep_flags: 0,
        compress_flags: 0,
        vertices: vec![[-4.0, -2.0, 0.0], [6.0, 2.0, 0.0], [0.0, 0.0, 8.0]],
        data_flags: 0,
        normals: Vec::new(),
        tangents: Vec::new(),
        bitangents: Vec::new(),
        bound_center: [0.0; 3],
        bound_radius: 0.0,
        colors: Vec::new(),
        uv_sets: Vec::new(),
        consistency_flags: 0,
        additional_data: BlockRef::NONE,
        num_triangles: 0,
        strips: Vec::new(),
        triangles: Vec::new(),
    };
    let source = nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1]),
        node("Torch", [10.0, 0.0, 50.0], IDENTITY, &[2, 3]),
        Block::TriStrips(NiTriStrips {
            av: av("FlameSource", [0.0, 0.0, 30.0], IDENTITY),
            data: BlockRef(Some(4)),
            skin_instance: BlockRef::NONE,
            shader_name: None,
            shader_extra_data: None,
        }),
        particle_block(),
        Block::TriStripsData(Box::new(data)),
    ]);
    let mut s = system(EmitterShape::Mesh, Vec::new());
    let mut e = emitter(EmitterShape::Mesh, None);
    e.meshes = vec![BlockRef(Some(2))];
    e.velocity_type = 2;
    e.emission_axis = [0.0, 3.0, 0.0];
    s.emitter = Some(e);
    let p = plan(&source, &s).unwrap();
    assert_eq!(p.volume, Vec3::new(10.0, 4.0, 8.0));
    // Центр коробки (1, 0, 4) в кадре меша, меш — на (0, 0, 30) под «Torch».
    assert!(
        close(p.emitter_frame.translation.into(), [1.0, 0.0, 34.0]),
        "{:?}",
        p.emitter_frame.translation
    );
    assert_eq!(p.losses, [Loss::MeshSurface]);
    // Ось +Y: склонение 90° от +Z, угол вокруг Z — 90°.
    assert!((p.declination - PI / 2.0).abs() < 1e-5 && (p.planar_angle - PI / 2.0).abs() < 1e-5);

    // Случайная скорость — во все стороны.
    let mut random = s.clone();
    random.emitter.as_mut().unwrap().velocity_type = 1;
    let p = plan(&source, &random).unwrap();
    assert_eq!(
        (p.declination_variation, p.planar_angle_variation),
        (PI, 2.0 * PI)
    );
}

fn full_plan() -> Plan {
    let modifiers = vec![
        modifier(ModifierKind::GrowFade {
            grow: 0.15,
            fade: 1.35,
        }),
        modifier(ModifierKind::Color {
            keys: vec![(0.0, [1.0; 4]), (1.0, [1.0, 0.0, 0.0, 0.0])],
        }),
        modifier(ModifierKind::Rotation {
            speed: 2.0,
            random_axis: false,
            axis: [0.0, 0.0, 1.0],
        }),
        modifier(ModifierKind::Gravity {
            object: BlockRef::NONE,
            axis: [0.0, 0.0, -1.0],
            decay: 0.0,
            strength: 16.0,
            force_type: 1,
            turbulence: 0.0,
        }),
    ];
    let mut s = system(
        EmitterShape::Box {
            width: 1.0,
            height: 2.0,
            depth: 3.0,
        },
        modifiers,
    );
    s.world_space = false;
    plan(&torch(), &s).unwrap()
}

/// Эмиттер Morrowind устроен как ванильный: узел-эмиттер и `NiBSParticleNode` с геометрией
/// частиц, контроллером и цепочкой воздействий; файл читается обратно.
#[test]
fn the_built_emitter_is_laid_out_like_a_vanilla_one_and_reads_back() {
    use tes3::nif::{NiAlphaProperty, NiMaterialProperty, NiTexturingProperty};

    let plan = full_plan();
    let mut stream = NiStream::new();
    let properties: Vec<NiLink<NiProperty>> = vec![
        stream.insert(NiTexturingProperty::default()).cast(),
        stream.insert(NiAlphaProperty::default()).cast(),
        stream.insert(NiMaterialProperty::default()).cast(),
    ];
    let pair = build(&mut stream, &plan, properties);
    let root = stream.insert(NiNode {
        children: pair.to_vec(),
        ..NiNode::default()
    });
    stream.roots.push(root.cast());
    let bytes = stream.save_bytes().unwrap();
    let back = NiStream::from_bytes(&bytes).unwrap();

    let emitter = back
        .objects_of_type::<NiNode>()
        .find(|n| n.base.base.name == "Fire Emitter")
        .expect("узел-эмиттер");
    assert_eq!(emitter.base.flags, EMITTER_NODE_FLAGS);
    assert!(emitter.children.is_empty());
    assert!(close(emitter.base.translation, [0.0, 4.0, 20.0]));

    let nodes: Vec<&NiBSParticleNode> = back.objects_of_type().collect();
    assert_eq!(nodes.len(), 1);
    let node = &nodes[0].base;
    assert_eq!(node.base.base.name, "Fire");
    assert_eq!(node.base.flags, PARTICLE_NODE_FLAGS, "частицы в кадре узла");
    assert!(close(node.base.translation, [1.0, 2.0, 3.0]));
    let z = back
        .get_as::<_, NiZBufferProperty>(node.base.properties[0])
        .expect("NiZBufferProperty на узле");
    assert_eq!(z.base.flags, Z_TEST_ONLY);
    let marks: Vec<&NiStringExtraData> = back.objects_of_type().collect();
    assert_eq!(marks.len(), 2, "sgoKeep на узле частиц и на узле-эмиттере");
    assert!(
        marks
            .iter()
            .all(|m| m.value == KEEP_NODE && m.base.bytes_remaining == 11)
    );

    // Вращение есть — геометрия вращающихся частиц.
    let particles: Vec<&NiRotatingParticles> = back.objects_of_type().collect();
    assert_eq!(particles.len(), 1);
    let geometry = &particles[0].base.base;
    assert_eq!(geometry.base.flags, PARTICLES_FLAGS);
    assert_eq!(geometry.base.properties.len(), 3);
    let data: Vec<&NiRotatingParticlesData> = back.objects_of_type().collect();
    let d = &data[0];
    assert_eq!(d.base.num_particles, 20);
    assert_eq!(d.base.base.vertices.len(), 20);
    assert_eq!(d.base.sizes, vec![1.0; 20]);
    assert_eq!(d.rotations.len(), 20);
    assert_eq!(d.base.particle_radius, 8.0);
    assert_eq!(d.base.base.vertex_colors.len(), 20, "есть цвет по возрасту");
    assert!(d.base.base.bound.radius > 20.0 * 1.5);

    let controllers: Vec<&NiParticleSystemController> = back.objects_of_type().collect();
    assert_eq!(controllers.len(), 1);
    let c = controllers[0];
    assert_eq!(c.base.flags, CONTROLLER_LOOP);
    assert_eq!((c.base.start_time, c.base.stop_time), (0.0, 0.95));
    assert_eq!((c.emit_start_time, c.emit_stop_time), (0.0, 0.95));
    assert_eq!((c.speed, c.speed_variation), (20.0, 10.0));
    assert_eq!((c.birth_rate, c.use_birth_rate), (13.0, true));
    assert_eq!((c.lifespan, c.lifespan_variation), (1.5, 0.5));
    assert_eq!(
        (c.emitter_width, c.emitter_height, c.emitter_depth),
        (1.0, 2.0, 3.0)
    );
    assert_eq!(c.initial_size, 8.0);
    assert_eq!(c.particles.len(), 20);
    assert!(!c.spawn_on_death);
    assert_eq!(
        back.get_as::<_, NiNode>(c.emitter)
            .map(|n| n.base.base.name.as_str()),
        Some("Fire Emitter")
    );
    assert!(
        back.get_as::<_, NiRotatingParticles>(c.base.target)
            .is_some()
    );
    assert!(
        back.get_as::<_, NiParticleSystemController>(geometry.base.base.controller)
            .is_some(),
        "геометрия ссылается на свой контроллер"
    );

    // Цепочка: тяготение → вращение → цвет → рост и затухание.
    let gravity = back
        .get_as::<_, NiGravity>(c.particle_modifier)
        .expect("NiGravity");
    assert_eq!(gravity.force_type, ForceType::Spherical);
    assert!((gravity.strength - 10.0).abs() < 1e-6);
    let rotation = back
        .get_as::<_, NiParticleRotation>(gravity.base.next)
        .expect("NiParticleRotation");
    assert_eq!(
        (rotation.rotation_speed, rotation.random_initial_axis),
        (2.0, false)
    );
    let color = back
        .get_as::<_, NiParticleColorModifier>(rotation.base.next)
        .expect("NiParticleColorModifier");
    let NiColorKey::LinKey(keys) = &back.get(color.color_data).unwrap().keys;
    assert_eq!(keys.len(), 2);
    let grow = back
        .get_as::<_, NiParticleGrowFade>(color.base.next)
        .expect("NiParticleGrowFade");
    assert_eq!((grow.grow_time, grow.fade_time), (0.15, 1.35));
    assert!(grow.base.next.is_null());
}

/// Без вращения и цвета — `NiAutoNormalParticles` без цветов вершин; частицы в мире — без
/// флага «в кадре узла»; эффект «один раз» — зажатый контроллер.
#[test]
fn a_plain_one_shot_world_space_system_takes_the_other_branches() {
    let mut s = system(POINT, Vec::new());
    s.emission = Some(Emission {
        once: true,
        ..emission(5.0)
    });
    let plan = plan(&torch(), &s).unwrap();
    let mut stream = NiStream::new();
    let pair = build(&mut stream, &plan, Vec::new());
    let root = stream.insert(NiNode {
        children: pair.to_vec(),
        ..NiNode::default()
    });
    stream.roots.push(root.cast());
    let back = NiStream::from_bytes(&stream.save_bytes().unwrap()).unwrap();
    assert_eq!(back.objects_of_type::<NiRotatingParticles>().count(), 0);
    assert_eq!(back.objects_of_type::<NiAutoNormalParticles>().count(), 1);
    let data: Vec<&NiAutoNormalParticlesData> = back.objects_of_type().collect();
    assert!(data[0].base.base.vertex_colors.is_empty());
    let node: Vec<&NiBSParticleNode> = back.objects_of_type().collect();
    assert_eq!(node[0].base.base.flags, PARTICLE_NODE_WORLD_FLAGS);
    let c: Vec<&NiParticleSystemController> = back.objects_of_type().collect();
    assert_eq!(c[0].base.flags, CONTROLLER_ONCE);
    assert!(c[0].particle_modifier.is_null());
}
