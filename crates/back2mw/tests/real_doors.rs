//! T2.6a: перепись дверей `Oblivion.esm` — что двигает последовательность `Open` их мешей, где
//! у створки петля относительно начала координат меша, стоит ли в том же меше рама, сколько
//! дверей служат телепортом; T2.6b — куда сдвигаются ссылки на двери с перенесённой петлёй.
//! Требует `BACK2MW_GAME_DATA` с `Oblivion.esm` и архивами мешей.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p back2mw --test real_doors -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/nif-oblivion.md`, «Двери: что двигает `Open`».

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;

use mesh_convert::door::{DoorFate, DoorKind, DoorMotion, door_motion, fate};
use mesh_convert::read::motion::sequences;
use mesh_convert::read::parse_with_offsets;
use tes4_read::{BsaIndex, Fields, FromFields, Placement, Plugin};
use transform::exterior::{ExteriorSource, reference_offset};

/// Вид двери для отчёта.
fn class(motion: Option<&DoorMotion>) -> &'static str {
    match fate(motion) {
        DoorFate::NoOpen => "без Open",
        DoorFate::Still => "Open ничего не двигает",
        DoorFate::Rehinged => "петля смещена, рамы нет",
        DoorFate::Framed => "петля + рама в меше",
        DoorFate::AtOrigin => "петля в начале координат",
        DoorFate::Leaves => "несколько створок",
        DoorFate::Sliding => "сдвиг (решётка)",
        DoorFate::Tilting => "наклон (люк)",
    }
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn census_of_door_motion() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let mut archives = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        archives.push((r, idx));
    }

    // Двери: FormID → (EDID, модель).
    let mut doors: BTreeMap<u32, (String, String)> = BTreeMap::new();
    let mut without_model = 0usize;
    for r in plugin.records_of(*b"DOOR") {
        let fields = Fields::parse(&r.data, "DOOR").expect("DOOR");
        let edid = fields.opt_zstring(*b"EDID").unwrap_or_default();
        match fields.opt_zstring(*b"MODL").filter(|m| !m.is_empty()) {
            Some(model) => {
                doors.insert(r.header.form_id, (edid, model.to_ascii_lowercase()));
            }
            None => without_model += 1,
        }
    }

    // Меш → движение. Ошибка разбора и отсутствие в архиве — отдельно.
    let models: BTreeSet<&str> = doors.values().map(|(_, m)| m.as_str()).collect();
    let mut motions: BTreeMap<&str, Option<DoorMotion>> = BTreeMap::new();
    let (mut missing, mut unreadable) = (Vec::new(), Vec::new());
    let mut versions: BTreeMap<u32, usize> = BTreeMap::new();
    for &model in &models {
        let path = format!("meshes\\{model}");
        let Some((reader, entry, idx)) = archives
            .iter_mut()
            .find_map(|(r, i)| i.find(&path).cloned().map(|e| (r, e, &*i)))
        else {
            missing.push(model);
            continue;
        };
        let data = idx.extract(reader, &entry).expect("extract");
        match parse_with_offsets(&data) {
            Ok((source, offsets)) => {
                *versions.entry(source.header.version).or_default() += 1;
                let seq = sequences(&data, &source, &offsets)
                    .unwrap_or_else(|e| panic!("{model}: анимация не читается: {e}"));
                motions.insert(model, door_motion(&source, &seq));
            }
            Err(e) => unreadable.push(format!("{model}: {e}")),
        }
    }

    // Ссылки на двери во всём плагине: телепорт или нет.
    let mut refs: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    for r in plugin.records_of(*b"REFR") {
        let p = Placement::from_record(r).expect("REFR");
        if doors.contains_key(&p.base) {
            let e = refs.entry(p.base).or_default();
            if p.teleport.is_some() {
                e.0 += 1;
            } else {
                e.1 += 1;
            }
        }
    }
    // То же в `SEWorld` с интерьерами, достижимыми дверями, — то, что переносится сейчас.
    let world = ExteriorSource::read(&plugin, "SEWorld", Some)
        .expect("SEWorld")
        .expect("SEWorld есть в Oblivion.esm");
    let mut world_refs: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    for p in world
        .placements
        .iter()
        .chain(world.interiors.iter().flat_map(|c| &c.placements))
    {
        if doors.contains_key(&p.base) && !p.initially_disabled {
            let e = world_refs.entry(p.base).or_default();
            if p.teleport.is_some() {
                e.0 += 1;
            } else {
                e.1 += 1;
            }
        }
    }

    println!(
        "DOOR {}: без модели {without_model}, мешей {}, нет в архивах {}, не читаются {}; версии {:?}",
        doors.len() + without_model,
        models.len(),
        missing.len(),
        unreadable.len(),
        versions
            .iter()
            .map(|(v, n)| format!("{v:#010x}: {n}"))
            .collect::<Vec<_>>()
    );
    for line in &unreadable {
        println!("  не читается: {line}");
    }

    #[derive(Default)]
    struct Row {
        meshes: usize,
        records: usize,
        /// Ссылок во всём плагине: телепортов, обычных.
        refs: (usize, usize),
        world: (usize, usize),
        examples: Vec<String>,
    }
    let mut rows: BTreeMap<&str, Row> = BTreeMap::new();
    for (&model, motion) in &motions {
        let row = rows.entry(class(motion.as_ref())).or_default();
        row.meshes += 1;
        if row.examples.len() < 4 {
            row.examples.push(model.to_owned());
        }
    }
    for (form_id, (_, model)) in &doors {
        let Some(motion) = motions.get(model.as_str()) else {
            continue;
        };
        let row = rows.entry(class(motion.as_ref())).or_default();
        row.records += 1;
        let (t, n) = refs.get(form_id).copied().unwrap_or_default();
        row.refs = (row.refs.0 + t, row.refs.1 + n);
        let (t, n) = world_refs.get(form_id).copied().unwrap_or_default();
        row.world = (row.world.0 + t, row.world.1 + n);
    }
    println!(
        "вид | мешей | записей DOOR | ссылок: телепорт / обычные | в SEWorld: телепорт / обычные | примеры"
    );
    for (name, row) in &rows {
        println!(
            "{name} | {} | {} | {} / {} | {} / {} | {}",
            row.meshes,
            row.records,
            row.refs.0,
            row.refs.1,
            row.world.0,
            row.world.1,
            row.examples.join(", ")
        );
    }

    // Створки на петле: направление, угол, смещение петли, поза файла.
    let hinged: Vec<(&str, &DoorMotion)> = motions
        .iter()
        .filter_map(|(m, d)| Some((*m, d.as_ref()?)))
        .filter(|(_, d)| d.kind() == DoorKind::Hinged)
        .collect();
    let turns: Vec<f32> = hinged
        .iter()
        .map(|(_, d)| d.leaves[0].turn_about_z().to_degrees())
        .collect();
    let clockwise = turns.iter().filter(|a| **a < 0.0).count();
    let offsets: Vec<f32> = hinged
        .iter()
        .map(|(_, d)| d.leaves[0].pivot.truncate().length())
        .collect();
    let fold = |v: &[f32]| {
        v.iter().fold((f32::MAX, f32::MIN), |(lo, hi), x| {
            (lo.min(x.abs()), hi.max(x.abs()))
        })
    };
    let durations: Vec<f32> = motions
        .values()
        .flatten()
        .filter(|d| d.kind() != DoorKind::Still)
        .map(|d| d.duration)
        .collect();
    let ajar = motions
        .values()
        .flatten()
        .flat_map(|d| &d.leaves)
        .filter(|l| l.pose_gap > 0.02)
        .count();
    println!(
        "на петле {}: по часовой (−Z, как Morrowind) {clockwise}, против {}; угол {:?}°; смещение петли {:?}; \
         длительность Open {:?} с; створок, чья поза файла не равна первому ключу Open: {ajar}",
        hinged.len(),
        hinged.len() - clockwise,
        fold(&turns),
        fold(&offsets),
        fold(&durations),
    );
    for (model, d) in hinged.iter().take(12) {
        let l = &d.leaves[0];
        println!(
            "  {model}: узел {:?}, петля ({:.1}, {:.1}, {:.1}), поворот {:.1}°, форм {} + {} вне",
            l.node,
            l.pivot.x,
            l.pivot.y,
            l.pivot.z,
            l.turn_about_z().to_degrees(),
            d.moving_shapes,
            d.fixed_shapes
        );
    }
    for (model, d) in motions.iter().filter_map(|(m, d)| Some((m, d.as_ref()?))) {
        if matches!(
            d.kind(),
            DoorKind::Leaves | DoorKind::Sliding | DoorKind::Tilting
        ) {
            let leaves: Vec<String> = d
                .leaves
                .iter()
                .map(|l| {
                    format!(
                        "{} ось ({:.1},{:.1},{:.1}) {:.0}° сдвиг ({:.0},{:.0},{:.0})",
                        l.node,
                        l.axis.x,
                        l.axis.y,
                        l.axis.z,
                        l.angle.to_degrees(),
                        l.slide.x,
                        l.slide.y,
                        l.slide.z
                    )
                })
                .collect();
            println!("  {:?} {model}: {}", d.kind(), leaves.join("; "));
        }
    }

    // T2.6b: ссылки `SEWorld` на двери с перенесённой петлёй — куда уезжает ссылка. И сколько
    // ссылок на двери вообще наклонено (углы X, Y не нулевые): для них порядок осей важен.
    let (mut tilted, mut door_refs) = (0usize, 0usize);
    for p in world
        .placements
        .iter()
        .chain(world.interiors.iter().flat_map(|c| &c.placements))
    {
        let Some((edid, model)) = doors.get(&p.base) else {
            continue;
        };
        door_refs += 1;
        tilted += usize::from(p.rotation[0].abs() > 1e-3 || p.rotation[1].abs() > 1e-3);
        let Some(hinge) = motions
            .get(model.as_str())
            .and_then(|m| m.as_ref()?.hinge())
        else {
            continue;
        };
        let shift = reference_offset(p.rotation, p.scale.unwrap_or(1.0), hinge.to_array());
        println!(
            "  петля перенесена: ссылка {:08X} на {edid} ({model}): позиция {:?}, углы {:?}, масштаб {:?}; \
             петля в меше {:?} → сдвиг ссылки {:?}",
            p.form_id, p.position, p.rotation, p.scale, hinge, shift
        );
    }
    println!(
        "ссылок на двери в SEWorld {door_refs}, из них наклонённых (углы X или Y ≠ 0) {tilted}"
    );

    assert_eq!(doors.len() + without_model, 501, "DOOR Oblivion.esm");
    let count = |f: DoorFate| motions.values().filter(|m| fate(m.as_ref()) == f).count();
    assert_eq!(
        [
            count(DoorFate::NoOpen),
            count(DoorFate::AtOrigin),
            count(DoorFate::Rehinged),
            count(DoorFate::Framed),
            count(DoorFate::Leaves),
            count(DoorFate::Sliding),
            count(DoorFate::Tilting),
            count(DoorFate::Still),
        ],
        [158, 30, 13, 9, 22, 12, 10, 0],
        "мешей по видам"
    );
    let _ = <Placement as FromFields>::NAME;
}

/// Дерево узлов одного меша двери и ключи его `Open` — посмотреть глазами, откуда взялась петля.
/// Меш — в `BACK2MW_DOOR` (путь относительно `Meshes\`), по умолчанию дверь Деменции.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn dump_one_door() {
    use mesh_convert::read::Block;
    let model = std::env::var("BACK2MW_DOOR").unwrap_or_else(|_| {
        r"architecture\city\dementia\dementiainteriors\demdooranim01.nif".to_owned()
    });
    let path = format!("meshes\\{}", model.to_ascii_lowercase());
    let mut data = None;
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        if let Some(entry) = idx.find(&path).cloned() {
            data = Some(idx.extract(&mut r, &entry).expect("extract"));
            break;
        }
    }
    let data = data.unwrap_or_else(|| panic!("{path}: нет в архивах"));
    let (source, offsets) = parse_with_offsets(&data).expect("parse");
    let mut stack: Vec<(usize, usize)> = source
        .roots
        .iter()
        .filter_map(|r| r.index())
        .map(|i| (i, 0))
        .collect();
    while let Some((index, depth)) = stack.pop() {
        let (kind, av, children) = match &source.blocks[index] {
            Block::Node(n) => ("узел", &n.av, n.children.as_slice()),
            Block::TriStrips(s) | Block::TriShape(s) => ("форма", &s.av, [].as_slice()),
            _ => continue,
        };
        println!(
            "{}{kind} {index} {:?}: сдвиг {:?}, масштаб {}, поворот {:?}",
            "  ".repeat(depth),
            av.net.name,
            av.translation,
            av.scale,
            av.rotation
        );
        for c in children.iter().rev().filter_map(|r| r.index()) {
            stack.push((c, depth + 1));
        }
    }
    let seq = sequences(&data, &source, &offsets).expect("sequences");
    for s in &seq {
        println!(
            "последовательность {:?}: {} … {} с",
            s.name, s.start, s.stop
        );
        for t in &s.tracks {
            println!("  {:?}: {:?}", t.node, t.keys);
        }
    }
    let motion = door_motion(&source, &seq);
    println!("движение: {motion:#?}");
    println!("участь: {:?}", fate(motion.as_ref()));
}
