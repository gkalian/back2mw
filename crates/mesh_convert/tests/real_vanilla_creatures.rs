//! T11.5a: перепись ванильных существ Morrowind — как устроены меш и анимация существа в
//! `Morrowind.bsa` (`meshes\r\`): файл с лентой, пара `x<имя>.nif` + `x<имя>.kf`, метки групп,
//! дорожка движения на `Bip01`. По ней пишется своя модель существа Oblivion: наш файл должен
//! быть устроен так же, как ванильный.
//!
//! Это измерение, синтетики нет; числа переносятся в `docs/ru/formats/tes3.md`, «Модель и
//! анимация существа».
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p mesh_convert --test real_vanilla_creatures -- --ignored --nocapture
//! ```

mod common;

use std::collections::{BTreeMap, BTreeSet};

use tes3::bsa::Archive;
use tes3::esp::{Creature, Plugin};
use tes3::nif::{
    NiAVObject, NiExtraData, NiFloatKey, NiKey, NiKeyframeController, NiKeyframeData, NiLink,
    NiObject, NiObjectNET, NiPosKey, NiRotKey, NiStream, NiTextKey, NiTimeController, NiType,
};

fn name_of(object: &NiType) -> String {
    String::from_utf8_lossy(object.type_name()).into_owned()
}

fn type_at(stream: &NiStream, key: NiKey) -> String {
    stream
        .objects
        .get(key)
        .map_or_else(|| "—".to_owned(), name_of)
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn print_map(title: &str, map: &BTreeMap<String, usize>, limit: usize) {
    println!("{title}:");
    let mut rows: Vec<(&String, &usize)> = map.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (k, n) in rows.into_iter().take(limit) {
        println!("  {n:5}  {k}");
    }
}

/// Дети узла сцены — у любого наследника `NiNode`.
fn children_of(object: &NiType) -> Option<&Vec<NiLink<NiAVObject>>> {
    match object {
        NiType::NiNode(n) => Some(&n.children),
        NiType::NiBSParticleNode(n) => Some(&n.base.children),
        NiType::RootCollisionNode(n) => Some(&n.base.children),
        NiType::NiBillboardNode(n) => Some(&n.base.children),
        NiType::NiBSAnimationNode(n) => Some(&n.base.children),
        NiType::NiSwitchNode(n) => Some(&n.base.children),
        _ => None,
    }
}

fn net(stream: &NiStream, key: NiKey) -> Option<&NiObjectNET> {
    stream.get_as::<NiObject, NiObjectNET>(NiLink::new(key))
}

fn av(stream: &NiStream, key: NiKey) -> Option<&NiAVObject> {
    stream.get_as::<NiObject, NiAVObject>(NiLink::new(key))
}

/// Вид ключей: тип интерполяции и число.
fn rot_kind(keys: &NiRotKey) -> (&'static str, usize) {
    match keys {
        NiRotKey::LinKey(k) => ("лин", k.len()),
        NiRotKey::BezKey(k) => ("безье", k.len()),
        NiRotKey::TCBKey(k) => ("TCB", k.len()),
        NiRotKey::EulerKey(_) => ("эйлер", 1),
    }
}

fn pos_points(keys: &NiPosKey) -> (&'static str, Vec<(f32, [f32; 3])>) {
    match keys {
        NiPosKey::LinKey(k) => (
            "лин",
            k.iter().map(|k| (k.time, k.value.to_array())).collect(),
        ),
        NiPosKey::BezKey(k) => (
            "безье",
            k.iter().map(|k| (k.time, k.value.to_array())).collect(),
        ),
        NiPosKey::TCBKey(k) => (
            "TCB",
            k.iter().map(|k| (k.time, k.value.to_array())).collect(),
        ),
    }
}

fn float_kind(keys: &NiFloatKey) -> (&'static str, usize) {
    match keys {
        NiFloatKey::LinKey(k) => ("лин", k.len()),
        NiFloatKey::BezKey(k) => ("безье", k.len()),
        NiFloatKey::TCBKey(k) => ("TCB", k.len()),
    }
}

/// Сдвиг в момент `t`: линейно между соседними ключами (для оценки скорости этого хватает).
fn pos_at(points: &[(f32, [f32; 3])], t: f32) -> Option<[f32; 3]> {
    let first = points.first()?;
    if t <= first.0 {
        return Some(first.1);
    }
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.0 {
            let span = b.0 - a.0;
            let f = if span > 0.0 { (t - a.0) / span } else { 0.0 };
            return Some([0, 1, 2].map(|i| a.1[i] + (b.1[i] - a.1[i]) * f));
        }
    }
    points.last().map(|p| p.1)
}

/// Всё, что перепись берёт из одного файла.
#[derive(Default)]
struct Scan {
    /// Тип и имя корней.
    roots: Vec<String>,
    nodes: usize,
    shapes_skinned: usize,
    shapes_rigid: usize,
    /// Имена корней скиннинга.
    skin_roots: BTreeSet<String>,
    /// Контроллеры ключей: имя цели → данные.
    tracks: Vec<(String, NiKeyframeData)>,
    /// Флаги, частота, фаза, начало, конец каждого контроллера ключей.
    controller_fields: Vec<(u16, f32, f32, f32, f32)>,
    /// Цель контроллера пуста (у `.kf` — так и должно быть).
    targets_null: usize,
    /// Текстовые ключи и имя объекта, на котором они висят.
    text_keys: Vec<(String, Vec<NiTextKey>)>,
    /// Прочие контроллеры (не ключевые) по типам.
    other_controllers: BTreeMap<String, usize>,
    /// Типы узлов сцены.
    node_types: BTreeMap<String, usize>,
    has_bip01: bool,
    /// Родитель `Bip01`: «корень» или имя.
    bip01_parent: String,
    /// Поза `Bip01` в файле: сдвиг и единичен ли поворот.
    bip01_pose: Option<([f32; 3], bool)>,
    has_bounding_box: bool,
    /// Узел `Bounding Box`: флаги, сдвиг узла и его объём, как есть.
    bounding_box: Option<String>,
    /// Строки `NiStringExtraData` в цепочке корня (у `.kf` — имена целей контроллеров).
    root_strings: Vec<String>,
    /// Порядок цепочки extra data корня по типам.
    root_extra_chain: Vec<String>,
    /// Длина цепочки контроллеров корня.
    root_controllers: usize,
}

fn scan(stream: &NiStream) -> Scan {
    let mut s = Scan::default();
    let mut parent_of: BTreeMap<NiKey, NiKey> = BTreeMap::new();
    for (key, object) in &stream.objects {
        for c in children_of(object).into_iter().flatten() {
            parent_of.insert(c.key, key);
        }
    }
    for root in &stream.roots {
        let name = net(stream, root.key).map_or("", |n| n.name.as_str());
        s.roots
            .push(format!("{} «{name}»", type_at(stream, root.key)));
        if let Some(n) = net(stream, root.key) {
            let mut extra: NiLink<NiExtraData> = n.extra_data;
            while let Some(object) = stream.objects.get(extra.key) {
                s.root_extra_chain.push(name_of(object));
                if let NiType::NiStringExtraData(v) = object {
                    s.root_strings.push(v.value.clone());
                }
                let Some(e) = stream.get_as::<NiExtraData, NiExtraData>(extra) else {
                    break;
                };
                extra = e.next;
                if s.root_extra_chain.len() > 4096 {
                    break;
                }
            }
            let mut ctl: NiLink<NiTimeController> = n.controller;
            while let Some(t) = stream.get(ctl) {
                s.root_controllers += 1;
                ctl = t.next;
                if s.root_controllers > 4096 {
                    break;
                }
            }
        }
    }
    // Контроллеры корня `.kf` идут в том же порядке, что строки с именами целей.
    let kf_names: Vec<String> = s.root_strings.clone();
    let mut kf_order: Vec<NiKey> = Vec::new();
    if let Some(root) = stream.roots.first()
        && matches!(
            stream.objects.get(root.key),
            Some(NiType::NiSequenceStreamHelper(_))
        )
        && let Some(n) = net(stream, root.key)
    {
        let mut ctl: NiLink<NiTimeController> = n.controller;
        while let Some(t) = stream.get(ctl) {
            kf_order.push(ctl.key);
            ctl = t.next;
            if kf_order.len() > 4096 {
                break;
            }
        }
    }

    for (key, object) in &stream.objects {
        if children_of(object).is_some() {
            s.nodes += 1;
            bump(&mut s.node_types, name_of(object));
        }
        if let Some(a) = av(stream, key) {
            if a.base.name.eq_ignore_ascii_case("Bip01") {
                s.has_bip01 = true;
                s.bip01_parent = match parent_of.get(&key) {
                    None => "нет (сам корень)".to_owned(),
                    Some(p) if stream.roots.iter().any(|r| r.key == *p) => "корень".to_owned(),
                    Some(p) => net(stream, *p).map_or_else(String::new, |n| n.name.clone()),
                };
                let identity = a
                    .rotation
                    .abs_diff_eq(tes3::nif::glam::Mat3::IDENTITY, 1e-3);
                s.bip01_pose = Some((a.translation.to_array(), identity));
            }
            if a.base.name.eq_ignore_ascii_case("Bounding Box") {
                s.has_bounding_box = true;
                s.bounding_box = Some(format!(
                    "флаги {:#06x}, T={:?}, {}",
                    a.flags,
                    a.translation.to_array(),
                    match a.bounding_volume.as_ref().map(|v| &v.bound_data) {
                        Some(tes3::nif::BoundData::NiBoxBV(b)) => format!(
                            "коробка: центр {:?}, оси единичные {}, размеры {:?}",
                            b.center.to_array(),
                            b.axis.abs_diff_eq(tes3::nif::glam::Mat3::IDENTITY, 1e-3),
                            b.extents.to_array()
                        ),
                        Some(other) => format!("{other:?}").chars().take(60).collect(),
                        None => "объёма нет".to_owned(),
                    }
                ));
            }
        }
        match object {
            NiType::NiTriShape(shape) => {
                if let Some(skin) = stream.get(shape.base.base.skin_instance) {
                    s.shapes_skinned += 1;
                    s.skin_roots.insert(
                        net(stream, skin.root.key).map_or_else(String::new, |n| n.name.clone()),
                    );
                } else {
                    s.shapes_rigid += 1;
                }
            }
            NiType::NiKeyframeController(c) => {
                let c: &NiKeyframeController = c;
                s.controller_fields.push((
                    c.base.flags,
                    c.base.frequency,
                    c.base.phase,
                    c.base.start_time,
                    c.base.stop_time,
                ));
                let target = if c.base.target.is_null() {
                    s.targets_null += 1;
                    kf_order
                        .iter()
                        .position(|k| *k == key)
                        // Первая строка цепочки `.kf` — текстовые ключи, а не имя: имена — только
                        // `NiStringExtraData`, по порядку.
                        .and_then(|i| kf_names.get(i).cloned())
                        .unwrap_or_default()
                } else {
                    net(stream, c.base.target.key).map_or_else(String::new, |n| n.name.clone())
                };
                if let Some(data) = stream.get(c.data) {
                    s.tracks.push((target, data.clone()));
                }
            }
            NiType::NiTextKeyExtraData(t) => {
                // Хозяин — объект, в чьей цепочке extra data стоит блок.
                let mut holder = String::new();
                for (other, _) in &stream.objects {
                    let Some(n) = net(stream, other) else {
                        continue;
                    };
                    let mut extra: NiLink<NiExtraData> = n.extra_data;
                    let mut hops = 0;
                    while !extra.is_null() && hops < 4096 {
                        if extra.key == key {
                            holder.clone_from(&n.name);
                        }
                        let Some(e) = stream.get_as::<NiExtraData, NiExtraData>(extra) else {
                            break;
                        };
                        extra = e.next;
                        hops += 1;
                    }
                }
                s.text_keys.push((holder, t.keys.clone()));
            }
            other => {
                let kind = name_of(other);
                if kind.ends_with("Controller") {
                    bump(&mut s.other_controllers, kind);
                }
            }
        }
    }
    s
}

/// Группа и метка текстового ключа: `"Idle: Loop Start"` → (`idle`, `loop start`). В одном ключе
/// бывает несколько строк.
fn split_text_key(value: &str) -> Vec<(String, String)> {
    value
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|line| match line.split_once(':') {
            Some((group, mark)) => (
                group.trim().to_ascii_lowercase(),
                mark.trim().to_ascii_lowercase(),
            ),
            None => (String::new(), line.to_ascii_lowercase()),
        })
        .collect()
}

/// Время метки `group: mark` на ленте.
fn mark_time(keys: &[NiTextKey], group: &str, mark: &str) -> Option<f32> {
    keys.iter().find_map(|k| {
        split_text_key(&k.value)
            .iter()
            .any(|(g, m)| g == group && m == mark)
            .then_some(k.time)
    })
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
#[expect(
    clippy::too_many_lines,
    reason = "перепись: одна длинная печать, делить её на части — только мешать чтению"
)]
fn census_of_vanilla_creatures() {
    let archive = Archive::from_path(common::game_file("Morrowind.bsa")).expect("Morrowind.bsa");
    // Файлы `meshes\r\` и общая анимация гуманоида.
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for entry in archive.entries() {
        let Some(name) = entry.name() else { continue };
        let path = String::from_utf8_lossy(name.as_ref())
            .to_ascii_lowercase()
            .replace('/', "\\");
        if path.starts_with("meshes\\r\\") || path.starts_with("meshes\\xbase_anim") {
            files.insert(path, entry.as_bytes().to_vec());
        }
    }
    let in_r: Vec<&String> = files
        .keys()
        .filter(|p| p.starts_with("meshes\\r\\"))
        .collect();
    let stems = |ext: &str, x: bool| -> BTreeSet<String> {
        in_r.iter()
            .filter_map(|p| p.strip_prefix("meshes\\r\\"))
            .filter_map(|p| p.strip_suffix(ext))
            .filter(|p| p.starts_with('x') == x)
            .map(|p| p.strip_prefix('x').unwrap_or(p).to_owned())
            .collect()
    };
    // Имя без `x` у файлов-пар; у обычных — как есть. `xbase…` в `r\` нет, так что первая буква
    // `x` — признак пары (проверяется ниже: у каждого `x<имя>.nif` должен быть `<имя>.nif`).
    let x_nif = stems(".nif", true);
    let x_kf = stems(".kf", true);
    let plain_all: BTreeSet<String> = in_r
        .iter()
        .filter_map(|p| p.strip_prefix("meshes\\r\\"))
        .filter_map(|p| p.strip_suffix(".nif"))
        .map(str::to_owned)
        .collect();
    let plain: BTreeSet<String> = plain_all
        .iter()
        .filter(|p| !(p.starts_with('x') && plain_all.contains(&p[1..])))
        .cloned()
        .collect();
    println!(
        "meshes\\r\\: файлов {}, .nif {}, .kf {}",
        in_r.len(),
        in_r.iter().filter(|p| p.ends_with(".nif")).count(),
        in_r.iter().filter(|p| p.ends_with(".kf")).count(),
    );
    println!(
        "обычных <имя>.nif {}, x<имя>.nif {}, x<имя>.kf {}; пар nif+kf {}; пар без обычного файла {:?}; kf без x-nif {:?}",
        plain.len(),
        x_nif.iter().filter(|n| plain.contains(*n)).count(),
        x_kf.len(),
        x_kf.iter().filter(|n| x_nif.contains(*n)).count(),
        x_kf.iter()
            .filter(|n| !plain.contains(*n))
            .collect::<Vec<_>>(),
        x_kf.iter()
            .filter(|n| !x_nif.contains(*n))
            .collect::<Vec<_>>(),
    );

    // Записи существ: какие модели и есть ли у них пара.
    let mut plugin = Plugin::new();
    plugin
        .load_path_filtered(common::game_file("Morrowind.esm"), |tag| {
            matches!(&tag, b"CREA")
        })
        .expect("Morrowind.esm");
    let mut models: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut creatures = 0usize;
    for c in plugin.objects_of_type::<Creature>() {
        creatures += 1;
        let model = c.mesh.to_ascii_lowercase().replace('/', "\\");
        let row = models.entry(model).or_default();
        row.0 += 1;
        if c.creature_flags.bits() & 0x1 != 0 {
            row.1 += 1;
        }
    }
    let with_pair = models
        .keys()
        .filter(|m| {
            m.strip_prefix("r\\")
                .and_then(|m| m.strip_suffix(".nif"))
                .is_some_and(|m| x_kf.contains(m))
        })
        .count();
    println!(
        "Morrowind.esm: CREA {creatures}, моделей {}, с парой x-nif + x-kf {with_pair}, двуногих записей {}",
        models.len(),
        models.values().map(|r| r.1).sum::<usize>(),
    );
    for (model, (n, biped)) in &models {
        let stem = model
            .strip_prefix("r\\")
            .and_then(|m| m.strip_suffix(".nif"))
            .unwrap_or("");
        if !x_kf.contains(stem) || *biped > 0 {
            println!(
                "  {model}: записей {n}, двуногих {biped}, пара {}",
                x_kf.contains(stem)
            );
        }
    }

    let mut facts: BTreeMap<String, usize> = BTreeMap::new();
    let mut groups: BTreeMap<String, usize> = BTreeMap::new();
    let mut marks: BTreeMap<String, usize> = BTreeMap::new();
    let mut group_marks: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut key_kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut controller_flags: BTreeMap<String, usize> = BTreeMap::new();
    let mut other_controllers: BTreeMap<String, usize> = BTreeMap::new();
    let mut node_types: BTreeMap<String, usize> = BTreeMap::new();
    let mut tracked_only_in_kf: BTreeMap<String, usize> = BTreeMap::new();
    let mut table: Vec<String> = Vec::new();
    let mut boxes: Vec<String> = Vec::new();

    for stem in &plain {
        let load = |path: String| -> Option<NiStream> {
            let bytes = files.get(&path)?;
            let mut stream = NiStream::new();
            stream.load_bytes(bytes).ok()?;
            Some(stream)
        };
        let Some(base_stream) = load(format!("meshes\\r\\{stem}.nif")) else {
            bump(&mut facts, "обычный файл не читается");
            continue;
        };
        let base = scan(&base_stream);
        let x = load(format!("meshes\\r\\x{stem}.nif")).map(|s| scan(&s));
        let kf = load(format!("meshes\\r\\x{stem}.kf")).map(|s| scan(&s));

        // Обычный файл: что в нём.
        for (what, yes) in [
            ("обычный: есть контроллеры ключей", !base.tracks.is_empty()),
            ("обычный: есть текстовые ключи", !base.text_keys.is_empty()),
            ("обычный: есть Bip01", base.has_bip01),
            ("обычный: есть Bounding Box", base.has_bounding_box),
            ("обычный: есть кожа", base.shapes_skinned > 0),
            ("обычный: есть жёсткие формы", base.shapes_rigid > 0),
            (
                "обычный: есть пара x-nif + x-kf",
                x.is_some() && kf.is_some(),
            ),
            (
                "обычный: анимирован, пары нет",
                !base.tracks.is_empty() && kf.is_none(),
            ),
            (
                "обычный: без анимации и без пары",
                base.tracks.is_empty() && kf.is_none(),
            ),
        ] {
            if yes {
                bump(&mut facts, what);
            }
        }
        if let Some(b) = &base.bounding_box {
            bump(
                &mut facts,
                format!(
                    "Bounding Box: {}",
                    if b.contains("коробка") {
                        "коробка"
                    } else {
                        "не коробка"
                    }
                ),
            );
            bump(
                &mut facts,
                format!(
                    "Bounding Box: оси единичные {}",
                    b.contains("оси единичные true")
                ),
            );
            bump(
                &mut facts,
                format!("Bounding Box: {}", b.split(',').next().unwrap_or_default()),
            );
            boxes.push(format!("{stem}: {b}"));
        }
        for r in &base.roots {
            bump(
                &mut facts,
                format!(
                    "обычный: корень {}",
                    r.split(' ').next().unwrap_or_default()
                ),
            );
        }
        if base.has_bip01 {
            bump(
                &mut facts,
                format!("обычный: родитель Bip01 — {}", base.bip01_parent),
            );
        }
        for root in &base.skin_roots {
            bump(&mut facts, format!("обычный: корень кожи «{root}»"));
        }
        for (holder, _) in &base.text_keys {
            bump(
                &mut facts,
                format!("обычный: текстовые ключи висят на «{holder}»"),
            );
        }
        for (k, n) in &base.other_controllers {
            *other_controllers.entry(k.clone()).or_default() += n;
        }
        for (k, n) in &base.node_types {
            *node_types.entry(k.clone()).or_default() += n;
        }

        if let (Some(x), Some(kf)) = (&x, &kf) {
            for (what, yes) in [
                ("x-nif: есть контроллеры ключей", !x.tracks.is_empty()),
                ("x-nif: есть текстовые ключи", !x.text_keys.is_empty()),
                ("x-nif: есть кожа", x.shapes_skinned > 0),
                ("x-nif: есть жёсткие формы", x.shapes_rigid > 0),
                (
                    "x-nif: узлов столько же, сколько в обычном",
                    x.nodes == base.nodes,
                ),
                (
                    "x-nif: форм столько же, сколько в обычном",
                    x.shapes_skinned + x.shapes_rigid == base.shapes_skinned + base.shapes_rigid,
                ),
                (
                    "kf: корень — NiSequenceStreamHelper",
                    kf.roots.len() == 1 && kf.roots[0].starts_with("NiSequenceStreamHelper"),
                ),
                (
                    "kf: первая в цепочке extra data — NiTextKeyExtraData",
                    kf.root_extra_chain.first().map(String::as_str) == Some("NiTextKeyExtraData"),
                ),
                (
                    "kf: остальные в цепочке — только NiStringExtraData",
                    kf.root_extra_chain
                        .iter()
                        .skip(1)
                        .all(|t| t == "NiStringExtraData"),
                ),
                (
                    "kf: строк столько же, сколько контроллеров",
                    kf.root_strings.len() == kf.root_controllers,
                ),
                (
                    "kf: все контроллеры — в цепочке корня",
                    kf.tracks.len() == kf.root_controllers,
                ),
                (
                    "kf: цель у всех контроллеров пуста",
                    kf.targets_null == kf.tracks.len(),
                ),
                (
                    "kf: дорожек столько же, сколько в обычном файле",
                    kf.tracks.len() == base.tracks.len(),
                ),
                (
                    "kf: текстовые ключи те же, что в обычном файле",
                    kf.text_keys.first().map(|t| &t.1) == base.text_keys.first().map(|t| &t.1),
                ),
                (
                    "kf: есть дорожка Bip01",
                    kf.tracks
                        .iter()
                        .any(|(n, _)| n.eq_ignore_ascii_case("Bip01")),
                ),
            ] {
                bump(&mut facts, format!("{what}: {yes}"));
            }
            let base_names: BTreeSet<&str> = base.tracks.iter().map(|t| t.0.as_str()).collect();
            for (name, _) in &kf.tracks {
                if !base_names.contains(name.as_str()) {
                    bump(&mut tracked_only_in_kf, name.clone());
                }
            }
        }

        // Лента: из пары, если она есть, иначе из обычного файла.
        let tape = kf.as_ref().unwrap_or(&base);
        for (flags, frequency, phase, start, stop) in &tape.controller_fields {
            bump(
                &mut controller_flags,
                format!(
                    "флаги {flags:#06x}, частота {frequency}, фаза {phase}, начало {}",
                    if *start == 0.0 { "0" } else { "≠ 0" }
                ),
            );
            let _ = stop;
        }
        let keys: &[NiTextKey] = tape.text_keys.first().map_or(&[], |t| t.1.as_slice());
        let mut own_groups: BTreeSet<String> = BTreeSet::new();
        for k in keys {
            for (group, mark) in split_text_key(&k.value) {
                bump(&mut marks, mark.clone());
                group_marks
                    .entry(group.clone())
                    .or_default()
                    .insert(mark.clone());
                own_groups.insert(group);
            }
        }
        for g in &own_groups {
            bump(&mut groups, g.clone());
        }
        let tape_end = keys.iter().map(|k| k.time).fold(0.0f32, f32::max);
        let stop_max = tape
            .controller_fields
            .iter()
            .map(|c| c.4)
            .fold(0.0f32, f32::max);
        let (mut rot_keys, mut pos_keys, mut scale_keys) = (0usize, 0usize, 0usize);
        for (_, data) in &tape.tracks {
            let (rk, rn) = rot_kind(&data.rotations.keys);
            let (pk, points) = pos_points(&data.translations.keys);
            let (sk, sn) = float_kind(&data.scales.keys);
            if rn > 0 {
                bump(&mut key_kinds, format!("поворот: {rk}"));
            }
            if !points.is_empty() {
                bump(&mut key_kinds, format!("сдвиг: {pk}"));
            }
            if sn > 0 {
                bump(&mut key_kinds, format!("масштаб: {sk}"));
            }
            rot_keys += rn;
            pos_keys += points.len();
            scale_keys += sn;
        }

        // Дорожка движения: сдвиг `Bip01` за петлю ходьбы и бега.
        let bip = tape
            .tracks
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("Bip01"))
            .map(|(_, d)| pos_points(&d.translations.keys).1);
        let mut speeds = Vec::new();
        for group in ["walkforward", "runforward", "swimwalkforward"] {
            let (Some(a), Some(b), Some(points)) = (
                mark_time(keys, group, "loop start").or_else(|| mark_time(keys, group, "start")),
                mark_time(keys, group, "loop stop").or_else(|| mark_time(keys, group, "stop")),
                bip.as_ref(),
            ) else {
                continue;
            };
            let (Some(pa), Some(pb)) = (pos_at(points, a), pos_at(points, b)) else {
                continue;
            };
            let d = [0, 1, 2].map(|i| pb[i] - pa[i]);
            let duration = b - a;
            speeds.push(format!(
                "{group}: Δ=({:.1}, {:.1}, {:.1}) за {duration:.2} с → {:.0} ед/с",
                d[0],
                d[1],
                d[2],
                (d[0] * d[0] + d[1] * d[1]).sqrt() / duration.max(1e-3),
            ));
            let axis = if d[1].abs() >= d[0].abs() { "Y" } else { "X" };
            let sign = if (if axis == "Y" { d[1] } else { d[0] }) >= 0.0 {
                "+"
            } else {
                "−"
            };
            bump(&mut facts, format!("{group}: Bip01 уходит по {sign}{axis}"));
        }
        let bip_turn = tape
            .tracks
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("Bip01"))
            .and_then(|(_, d)| match &d.rotations.keys {
                NiRotKey::LinKey(k) => {
                    Some(k.iter().map(|k| (k.time, k.value)).collect::<Vec<_>>())
                }
                NiRotKey::TCBKey(k) => {
                    Some(k.iter().map(|k| (k.time, k.value)).collect::<Vec<_>>())
                }
                NiRotKey::BezKey(k) => {
                    Some(k.iter().map(|k| (k.time, k.value)).collect::<Vec<_>>())
                }
                NiRotKey::EulerKey(_) => None,
            });
        for group in ["turnleft", "turnright"] {
            let (Some(a), Some(b), Some(keys)) = (
                mark_time(keys, group, "loop start"),
                mark_time(keys, group, "loop stop"),
                bip_turn.as_ref(),
            ) else {
                continue;
            };
            let at = |t: f32| {
                keys.iter()
                    .min_by(|x, y| (x.0 - t).abs().total_cmp(&(y.0 - t).abs()))
                    .map(|k| k.1)
            };
            if let (Some(qa), Some(qb)) = (at(a), at(b)) {
                let degrees = qa.angle_between(qb).to_degrees();
                bump(
                    &mut facts,
                    format!(
                        "{group}: Bip01 за петлю поворачивается на {}",
                        if degrees < 5.0 {
                            "< 5°".to_owned()
                        } else {
                            format!("{:.0}°", (degrees / 15.0).round() * 15.0)
                        }
                    ),
                );
            }
        }
        if !tape.tracks.is_empty() {
            bump(
                &mut facts,
                format!(
                    "лента: у Bip01 есть ключи сдвига: {}",
                    bip.as_ref().is_some_and(|p| p.len() > 1)
                ),
            );
            bump(
                &mut facts,
                format!(
                    "лента: конец ключей = последняя метка (±0.05): {}",
                    (tape_end - stop_max).abs() < 0.05
                ),
            );
        }
        table.push(format!(
            "{stem}: пара {}, узлов {}, кожа {} / жёстких {}, дорожек {}, ключей пов/сдв/масш {rot_keys}/{pos_keys}/{scale_keys}, \
             лента {tape_end:.2} с (stop {stop_max:.2}), меток {}, групп {}; Bip01: родитель «{}», поза {:?}; {}",
            kf.is_some(),
            base.nodes,
            base.shapes_skinned,
            base.shapes_rigid,
            tape.tracks.len(),
            keys.len(),
            own_groups.len(),
            base.bip01_parent,
            base.bip01_pose,
            speeds.join("; "),
        ));
    }

    println!("существа ({}):", table.len());
    for row in &table {
        println!("  {row}");
    }
    println!("Bounding Box:");
    for row in &boxes {
        println!("  {row}");
    }
    print_map("факты", &facts, 200);
    print_map("поля контроллеров ключей", &controller_flags, 40);
    print_map("виды ключей (дорожек)", &key_kinds, 40);
    print_map(
        "прочие контроллеры в обычных файлах",
        &other_controllers,
        40,
    );
    print_map("типы узлов в обычных файлах", &node_types, 40);
    print_map("группы (в скольких существах)", &groups, 200);
    print_map("метки (всего)", &marks, 200);
    print_map(
        "дорожки, которые есть в kf, но не в обычном файле",
        &tracked_only_in_kf,
        40,
    );
    println!("метки по группам:");
    for (group, set) in &group_marks {
        println!(
            "  {group}: {}",
            set.iter().cloned().collect::<Vec<_>>().join(" | ")
        );
    }

    // Общая анимация гуманоида: чем она отличается от пары существа.
    for path in files.keys().filter(|p| p.starts_with("meshes\\xbase_anim")) {
        let mut stream = NiStream::new();
        if stream.load_bytes(&files[path]).is_err() {
            println!("{path}: не читается");
            continue;
        }
        let s = scan(&stream);
        println!(
            "{path}: корни {:?}, узлов {}, форм {}, дорожек {}, строк {}, меток {}",
            s.roots,
            s.nodes,
            s.shapes_skinned + s.shapes_rigid,
            s.tracks.len(),
            s.root_strings.len(),
            s.text_keys.first().map_or(0, |t| t.1.len()),
        );
    }
    assert!(!table.is_empty(), "в Morrowind.bsa есть существа");
}

/// Текстовые ключи и дорожка `Bip01` одного существа — посмотреть глазами. Существо — в
/// `BACK2MW_CREATURE` (имя без `x` и расширения), по умолчанию `guar`.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn dump_one_vanilla_creature() {
    let stem = std::env::var("BACK2MW_CREATURE")
        .unwrap_or_else(|_| "guar".to_owned())
        .to_ascii_lowercase();
    let archive = Archive::from_path(common::game_file("Morrowind.bsa")).expect("Morrowind.bsa");
    let find = |wanted: String| -> Option<NiStream> {
        let entry = archive.entries().find(|e| {
            e.name()
                .is_some_and(|n| String::from_utf8_lossy(n.as_ref()).to_ascii_lowercase() == wanted)
        })?;
        let mut stream = NiStream::new();
        stream.load_bytes(entry.as_bytes()).ok()?;
        Some(stream)
    };
    for path in [
        format!("meshes\\r\\{stem}.nif"),
        format!("meshes\\r\\x{stem}.nif"),
        format!("meshes\\r\\x{stem}.kf"),
    ] {
        let Some(stream) = find(path.clone()) else {
            println!("==== {path}: нет");
            continue;
        };
        let s = scan(&stream);
        println!(
            "==== {path}: блоков {}, корни {:?}, цепочка extra корня {:?} …, контроллеров корня {}",
            stream.objects.len(),
            s.roots,
            &s.root_extra_chain[..s.root_extra_chain.len().min(4)],
            s.root_controllers,
        );
        // Дерево узлов с позой.
        fn walk(stream: &NiStream, key: NiKey, depth: usize) {
            let Some(a) = av(stream, key) else { return };
            let object = &stream.objects[key];
            let controllers: Vec<String> = {
                let mut out = Vec::new();
                let mut ctl: NiLink<NiTimeController> = a.base.controller;
                while let Some(t) = stream.get(ctl) {
                    out.push(type_at(stream, ctl.key));
                    ctl = t.next;
                    if out.len() > 8 {
                        break;
                    }
                }
                out
            };
            let skin = match object {
                NiType::NiTriShape(shape) if !shape.base.base.skin_instance.is_null() => " кожа",
                _ => "",
            };
            println!(
                "{}{} «{}» флаги {:#06x} T=({:.2}, {:.2}, {:.2}) R-единичный {} S={} {}{skin}",
                "  ".repeat(depth),
                name_of(object),
                a.base.name,
                a.flags,
                a.translation.x,
                a.translation.y,
                a.translation.z,
                a.rotation
                    .abs_diff_eq(tes3::nif::glam::Mat3::IDENTITY, 1e-3),
                a.scale,
                controllers.join("+"),
            );
            for c in children_of(object).into_iter().flatten() {
                walk(stream, c.key, depth + 1);
            }
        }
        for root in &stream.roots {
            walk(&stream, root.key, 1);
        }
        // Служебные поля, которые надо повторить при записи: счётчики байт у extra data и поля
        // контроллера ключей.
        let mut shown = [false; 3];
        for (_, object) in &stream.objects {
            match object {
                NiType::NiTextKeyExtraData(t) if !shown[0] => {
                    shown[0] = true;
                    println!(
                        "-- NiTextKeyExtraData: bytes_remaining {}, ключей {}",
                        t.base.bytes_remaining,
                        t.keys.len()
                    );
                }
                NiType::NiStringExtraData(t) if !shown[1] => {
                    shown[1] = true;
                    println!(
                        "-- NiStringExtraData: bytes_remaining {}, значение {:?}",
                        t.base.bytes_remaining, t.value
                    );
                }
                NiType::NiKeyframeController(c) if !shown[2] => {
                    shown[2] = true;
                    println!(
                        "-- NiKeyframeController: флаги {:#06x}, частота {}, фаза {}, начало {}, конец {}, цель пуста {}",
                        c.base.flags,
                        c.base.frequency,
                        c.base.phase,
                        c.base.start_time,
                        c.base.stop_time,
                        c.base.target.is_null()
                    );
                }
                _ => {}
            }
        }
        if let Some((holder, keys)) = s.text_keys.first() {
            println!("-- текстовые ключи на «{holder}» ({}):", keys.len());
            for k in keys {
                println!("  {:8.4}  {:?}", k.time, k.value);
            }
        }
        if let Some((_, data)) = s
            .tracks
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("Bip01"))
        {
            let (kind, points) = pos_points(&data.translations.keys);
            println!(
                "-- Bip01: поворот {:?}, сдвиг {kind} × {}, масштаб {:?}",
                rot_kind(&data.rotations.keys),
                points.len(),
                float_kind(&data.scales.keys),
            );
            for (t, p) in points.iter().take(60) {
                println!("  {t:8.4}  ({:9.3}, {:9.3}, {:9.3})", p[0], p[1], p[2]);
            }
        }
        println!(
            "-- дорожки ({}): {}",
            s.tracks.len(),
            s.tracks
                .iter()
                .map(|(n, d)| format!(
                    "{n} [{}/{}/{}]",
                    rot_kind(&d.rotations.keys).1,
                    pos_points(&d.translations.keys).1.len(),
                    float_kind(&d.scales.keys).1
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}
