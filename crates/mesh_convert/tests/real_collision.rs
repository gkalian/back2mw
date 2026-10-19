//! T1.7: коллизия на реальных мешах `STAT` и проверка масштаба.
//!
//! Множитель `HAVOK_UNITS_TO_GAME` = 7 взят из референса; здесь он проверяется данными.
//! Для каждого меша с `RootCollisionNode` сравниваются габариты коллизии и видимой
//! геометрии (обе — в выходном потоке, в игровых юнитах): при верном множителе отношение
//! около 1 по каждой оси. Отдельно печатается, сколько мешей совпало бы **без**
//! множителя, — чтобы было видно, что критерий различает 1 и 7.
//!
//! Что проверяет `niftest` на выходных файлах с коллизией — `real_convert.rs`.
//!
//! Запуск (PowerShell):
//! ```text
//! $env:BACK2MW_GAME_DATA = 'C:\path\to\back2mw-data\tes'
//! cargo test -p mesh_convert --test real_collision -- --ignored --nocapture
//! ```

mod common;

use std::fs::{self, File};
use std::io::BufReader;

use mesh_convert::geometry::convert;
use mesh_convert::read::{parse, parse_header};
use tes3::nif::glam::{Affine3A, Vec3};
use tes3::nif::{NiNode, NiStream, NiTriShape, NiTriShapeData};
use tes4_read::{BsaIndex, FromFields, Plugin, Stat};

const READABLE: &[&str] = mesh_convert::read::SUPPORTED_TYPES;

/// Срез M1: 10 мешей из `docs/ru/formats/nif-oblivion.md` (пути в архиве, нижний регистр).
const SLICE: [&str; 10] = [
    "meshes/clutter/vinyard/vinyardbarrelcartstopper.nif",
    "meshes/architecture/castle/skingrad/skcastlefloor01.nif",
    "meshes/dungeons/fortruins/dungeon/decorubble/rfrubblehallschunk01.nif",
    "meshes/oblivion/caves/obcavedoorplug.nif",
    "meshes/rocks/jeralmountains/jmrock01.nif",
    "meshes/landscape/cypressknee01.nif",
    "meshes/dungeons/fortruins/dungeon/doorsportcullis/rfnageddoor03.nif",
    "meshes/dungeons/fortruins/exterior/castle/rfcastleroofbeams02.nif",
    "meshes/dungeons/misc/coffinlid01.nif",
    "meshes/architecture/castle/kvatch/brokengatehousing.nif",
];

#[derive(Clone, Copy)]
struct Aabb {
    min: Vec3,
    max: Vec3,
}

impl Aabb {
    fn empty() -> Self {
        Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        }
    }
    fn add(&mut self, v: Vec3) {
        self.min = self.min.min(v);
        self.max = self.max.max(v);
    }
    fn extent(&self) -> Vec3 {
        self.max - self.min
    }
    fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
}

/// Габарит видимой геометрии (всё, кроме `RootCollisionNode`) в системе корня.
/// Возвращает габарит и признак «на пути были повороты», от которых зависит трактовка
/// матрицы, ещё не проверенная на объекте с известным поворотом.
fn visible_aabb(stream: &NiStream) -> (Aabb, bool) {
    let mut aabb = Aabb::empty();
    let mut rotated = false;
    for root in stream.roots_of_type::<NiNode>() {
        walk(
            stream,
            root.base.transform(),
            &root.children,
            &mut aabb,
            &mut rotated,
        );
    }
    (aabb, rotated)
}

fn walk(
    stream: &NiStream,
    parent: Affine3A,
    children: &[tes3::nif::NiLink<tes3::nif::NiAVObject>],
    aabb: &mut Aabb,
    rotated: &mut bool,
) {
    for &child in children {
        if let Some(shape) = stream.get_as::<_, NiTriShape>(child) {
            let av = &shape.base.base.base;
            *rotated |= av.rotation != tes3::nif::glam::Mat3::IDENTITY;
            let world = parent * av.transform();
            let data: &NiTriShapeData = stream
                .get_as(shape.base.base.geometry_data)
                .expect("данные формы");
            for &v in &data.base.base.vertices {
                aabb.add(world.transform_point3(v));
            }
        } else if let Some(node) = stream.get_as::<_, NiNode>(child) {
            *rotated |= node.base.rotation != tes3::nif::glam::Mat3::IDENTITY;
            walk(
                stream,
                parent * node.base.transform(),
                &node.children,
                aabb,
                rotated,
            );
        }
        // `RootCollisionNode` — отдельный тип и сюда не попадает.
    }
}

fn collision_aabb(stream: &NiStream) -> Aabb {
    let mut aabb = Aabb::empty();
    for node in stream.objects_of_type::<tes3::nif::RootCollisionNode>() {
        for &child in &node.base.children {
            let shape: &NiTriShape = stream.get_as(child).expect("форма коллизии");
            let data: &NiTriShapeData = stream
                .get_as(shape.base.base.geometry_data)
                .expect("данные формы коллизии");
            for &v in &data.base.base.vertices {
                aabb.add(v);
            }
        }
    }
    aabb
}

fn bucket(r: f32) -> usize {
    match r {
        r if r < 0.5 => 0,
        r if r < 0.9 => 1,
        r if r <= 1.1 => 2,
        r if r <= 2.0 => 3,
        _ => 4,
    }
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn collision_extent_matches_visible_extent_with_the_havok_multiplier() {
    let esm = common::game_file("Oblivion.esm");
    let bytes = fs::read(&esm).unwrap_or_else(|e| panic!("не прочитать {esm:?}: {e}"));
    let plugin = Plugin::parse(&bytes).unwrap_or_else(|e| panic!("Plugin::parse: {e}"));
    let mut reader = BufReader::new(
        File::open(common::game_file("Oblivion - Meshes.bsa")).expect("открыть BSA"),
    );
    let index = BsaIndex::read_from(&mut reader).unwrap_or_else(|e| panic!("BsaIndex: {e}"));

    let mut paths = std::collections::BTreeSet::new();
    for record in plugin.records_of(*b"STAT") {
        if record.data.is_empty() {
            continue;
        }
        let stat = Stat::from_record(record).expect("STAT");
        if let Some(entry) = stat.model.and_then(|m| index.find(&format!("meshes\\{m}"))) {
            paths.insert(entry.path.clone());
        }
    }

    let (mut meshes, mut with_collision, mut skipped_meshes) = (0usize, 0usize, 0usize);
    let (mut shapes, mut triangles, mut skipped_objects) = (0usize, 0usize, 0usize);
    let (mut no_collision_at_all, mut rotated_meshes) = (0usize, 0usize);
    // Гистограмма отношения по осям (x, y, z): <0.5, 0.5–0.9, 0.9–1.1, 1.1–2, >2.
    let mut histogram = [[0usize; 5]; 3];
    let (mut all_axes_match, mut all_axes_match_without_scale) = (0usize, 0usize);
    let (mut compared, mut compared_no_rotation, mut match_no_rotation) = (0usize, 0usize, 0usize);
    let mut ratios: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut failures: Vec<String> = Vec::new();
    let mut skinned = 0usize;
    let (mut slice_seen, mut slice_with_collision) = (0usize, 0usize);
    let (mut bodies_t, mut bodies_t_moved) = (0usize, 0usize);
    // [только обычные тела, только тела T]: (сравнено, совпало как есть, совпало с перевёрнутым флагом)
    let mut experiment = [(0usize, 0usize, 0usize, 0usize, 0usize); 2];
    let (mut bodies_plain, mut bodies_plain_moved) = (0usize, 0usize);

    for path in &paths {
        let entry = index.find(path).expect("путь из индекса");
        let nif = index.extract(&mut reader, entry).expect("извлечь");
        let Ok(header) = parse_header(&nif) else {
            continue;
        };
        if !header
            .block_type_index
            .iter()
            .all(|&i| READABLE.contains(&header.block_types[usize::from(i)].as_str()))
        {
            continue;
        }
        let source = match parse(&nif) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{path}: чтение источника: {e}"));
                continue;
            }
        };
        for b in &source.blocks {
            if let mesh_convert::read::Block::RigidBody(body) = b {
                let moved = body.translation[..3].iter().any(|&t| t.abs() > 1e-6)
                    || body.rotation != [0.0, 0.0, 0.0, 1.0];
                if body.translated {
                    bodies_t += 1;
                    bodies_t_moved += usize::from(moved);
                } else {
                    bodies_plain += 1;
                    bodies_plain_moved += usize::from(moved);
                }
            }
        }
        // Скиннинговые меши у STAT — не статика (см. real_convert).
        if source
            .blocks
            .iter()
            .any(|b| matches!(b, mesh_convert::read::Block::SkinInstance(_)))
        {
            skinned += 1;
            continue;
        }
        let converted = match convert(&source) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{path}: конвертация: {e}"));
                continue;
            }
        };
        meshes += 1;
        let s = converted.stats;
        if SLICE.contains(&path.replace('\\', "/").as_str()) {
            slice_seen += 1;
            slice_with_collision += usize::from(s.collision_shapes > 0);
        }
        shapes += s.collision_shapes;
        triangles += s.collision_triangles;
        skipped_objects += s.collision_skipped;
        if s.collision_skipped > 0 {
            skipped_meshes += 1;
        }
        if s.collision_shapes == 0 {
            no_collision_at_all += 1;
            continue;
        }
        with_collision += 1;

        let (visible, rotated) = visible_aabb(&converted.stream);
        let collision = collision_aabb(&converted.stream);
        if rotated {
            rotated_meshes += 1;
        }
        if visible.is_empty() || collision.is_empty() {
            continue;
        }
        let (ve, ce) = (visible.extent(), collision.extent());
        let mut all_match = true;
        let mut all_match_raw = true;
        let mut usable = true;
        for axis in 0..3 {
            if ve[axis] < 1e-3 {
                usable = false;
                continue;
            }
            let ratio = ce[axis] / ve[axis];
            histogram[axis][bucket(ratio)] += 1;
            ratios[axis].push(ratio);
            all_match &= (0.9..=1.1).contains(&ratio);
            // Без множителя коллизия была бы в семь раз меньше.
            all_match_raw &= (0.9..=1.1).contains(&(ratio / 7.0));
        }
        // Опыт на решение «применять сдвиг и поворот только у bhkRigidBodyT»: те же меши,
        // но у всех тел флаг перевёрнут. Если решение верно, у перевёрнутых совпадение
        // габаритов ухудшится.
        let has_t = source
            .blocks
            .iter()
            .any(|b| matches!(b, mesh_convert::read::Block::RigidBody(r) if r.translated));
        let has_plain = source
            .blocks
            .iter()
            .any(|b| matches!(b, mesh_convert::read::Block::RigidBody(r) if !r.translated));
        if usable && (has_t != has_plain) {
            let mut flipped = source.clone();
            for b in &mut flipped.blocks {
                if let mesh_convert::read::Block::RigidBody(r) = b {
                    r.translated = !r.translated;
                }
            }
            if let Ok(alt) = convert(&flipped) {
                let (alt_visible, _) = visible_aabb(&alt.stream);
                let alt_collision = collision_aabb(&alt.stream);
                if !alt_visible.is_empty() && !alt_collision.is_empty() {
                    let (ae, ve2) = (alt_collision.extent(), alt_visible.extent());
                    let alt_match =
                        (0..3).all(|a| ve2[a] >= 1e-3 && (0.9..=1.1).contains(&(ae[a] / ve2[a])));
                    let slot = usize::from(has_t);
                    experiment[slot].0 += 1;
                    experiment[slot].1 += usize::from(all_match);
                    experiment[slot].2 += usize::from(alt_match);
                    experiment[slot].3 += usize::from(center_ok(&visible, &collision));
                    experiment[slot].4 += usize::from(center_ok(&alt_visible, &alt_collision));
                }
            }
        }
        if usable {
            compared += 1;
            all_axes_match += usize::from(all_match);
            all_axes_match_without_scale += usize::from(all_match_raw);
            if !rotated {
                compared_no_rotation += 1;
                match_no_rotation += usize::from(all_match);
            }
        }
    }

    println!("=== Коллизия STAT (набор M1 + альфа) ===");
    println!("  мешей сконвертировано:                 {meshes}");
    println!("  из них с RootCollisionNode:            {with_collision}");
    println!("  без коллизии у корня:                  {no_collision_at_all}");
    println!("  форм коллизии / треугольников:         {shapes} / {triangles}");
    println!(
        "  bhkCollisionObject не у корня (пропущено): {skipped_objects} в {skipped_meshes} мешах"
    );
    println!("  bhkRigidBodyT: {bodies_t}, из них со сдвигом или поворотом: {bodies_t_moved}");
    println!(
        "  bhkRigidBody:  {bodies_plain}, из них со сдвигом или поворотом: {bodies_plain_moved}"
    );
    println!(
        "  опыт с обратным флагом, меши только с обычными телами: сравнено {}, как есть совпало {}, со сдвигом совпало {}; центры: как есть {}, со сдвигом {}",
        experiment[0].0, experiment[0].1, experiment[0].2, experiment[0].3, experiment[0].4
    );
    println!(
        "  опыт с обратным флагом, меши только с телами T: сравнено {}, как есть совпало {}, без сдвига совпало {}; центры: как есть {}, без сдвига {}",
        experiment[1].0, experiment[1].1, experiment[1].2, experiment[1].3, experiment[1].4
    );
    println!(
        "  срез M1 (10 мешей): найдено {slice_seen}, с RootCollisionNode {slice_with_collision}"
    );
    println!("  мешей с поворотами на пути к видимой геометрии: {rotated_meshes}");
    println!("  сравнено (габарит видимой геометрии > 0 по всем осям): {compared}");
    println!("  по всем осям в пределах 10%: {all_axes_match}");
    println!("  то же, но без множителя ×7:  {all_axes_match_without_scale}");
    println!(
        "  без поворотов: сравнено {compared_no_rotation}, в пределах 10% {match_no_rotation}"
    );
    println!("  отношение габаритов коллизия/видимая геометрия по осям:");
    println!("      ось   <0.5   0.5–0.9   0.9–1.1   1.1–2   >2      медиана");
    for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
        let mut sorted = ratios[axis].clone();
        sorted.sort_by(f32::total_cmp);
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or(f32::NAN);
        let h = histogram[axis];
        println!(
            "      {name}   {:>6} {:>8} {:>9} {:>7} {:>5}   {median:.3}",
            h[0], h[1], h[2], h[3], h[4]
        );
    }
    println!("  скиннинговых (не статика, пропущены): {skinned}");
    for line in failures.iter().take(10) {
        println!("    {line}");
    }

    assert!(failures.is_empty(), "были отказы чтения или конвертации");
    assert!(with_collision > 0, "ни одного меша с коллизией");
    assert_eq!(slice_seen, SLICE.len(), "не все меши среза M1 найдены");
    assert_eq!(
        slice_with_collision, slice_seen,
        "у части мешей среза M1 нет RootCollisionNode"
    );
    assert!(
        all_axes_match * 100 >= compared * 50,
        "габарит коллизии не согласуется с видимым у большинства мешей: {all_axes_match} из {compared}"
    );
}

/// Центры габаритов совпадают: по каждой оси расхождение не больше 10% габарита видимой
/// геометрии плюс одна игровая единица. Габарит от сдвига не зависит, центр — зависит,
/// поэтому именно он проверяет сдвиг тела.
fn center_ok(visible: &Aabb, collision: &Aabb) -> bool {
    let (vc, cc) = (
        (visible.min + visible.max) * 0.5,
        (collision.min + collision.max) * 0.5,
    );
    let ve = visible.extent();
    (0..3).all(|a| (vc[a] - cc[a]).abs() <= 0.1 * ve[a] + 1.0)
}
