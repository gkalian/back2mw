//! T9.2a: скелет Morrowind и устройство ванильной брони со скиннингом.
//!
//! Одноразовое измерение, числа — в `docs/ru/formats/tes3.md` («Скиннинг»). Синтетики нет.
//!
//! ```text
//! $env:BACK2MW_GAME_DATA = 'C:\path\to\back2mw-data\tes'
//! cargo test -p mesh_convert --test real_skinning -- --ignored --nocapture
//! ```

mod common;

use tes3::bsa::Archive;
use tes3::nif::{NiNode, NiSkinData, NiSkinInstance, NiStream, NiTriShape};

fn load(archive: &Archive, path: &str) -> NiStream {
    let file = archive
        .get(path)
        .unwrap_or_else(|| panic!("нет {path} в архиве"));
    NiStream::from_bytes(file.as_bytes()).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn type_name(object: &tes3::nif::NiType) -> String {
    let debug = format!("{object:?}");
    debug.split('(').next().unwrap_or("").to_owned()
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn morrowind_skeleton_and_a_vanilla_skinned_cuirass() {
    let archive = Archive::from_path(common::game_file("Morrowind.bsa")).expect("Morrowind.bsa");

    for skeleton in ["meshes\\base_anim.nif", "meshes\\base_anim_female.nif"] {
        let stream = load(&archive, skeleton);
        let world = stream.world_transforms();
        let mut bones: Vec<(String, [f32; 3])> = stream
            .objects_of_type_with_link::<NiNode>()
            .filter(|(_, n)| n.base.base.name.starts_with("Bip01"))
            .filter_map(|(link, n)| {
                let t = world.get(&link.key)?;
                Some((n.base.base.name.clone(), t.translation.into()))
            })
            .collect();
        bones.sort_by(|a, b| a.0.cmp(&b.0));
        println!("=== {skeleton}: костей Bip01 {} ===", bones.len());
        for (name, p) in &bones {
            println!("  {name:24} ({:8.2}, {:8.2}, {:8.2})", p[0], p[1], p[2]);
        }
    }

    for body in [
        "meshes\\b\\b_n_imperial_m_skins.nif",
        "meshes\\b\\b_n_imperial_f_skins.nif",
    ] {
        let stream = load(&archive, body);
        let world = stream.world_transforms();
        let skinned = stream
            .objects_of_type::<NiTriShape>()
            .filter(|s| !s.base.base.skin_instance.is_null())
            .count();
        let mut bones: Vec<(String, [f32; 3])> = stream
            .objects_of_type_with_link::<NiNode>()
            .filter(|(_, n)| n.base.base.name.starts_with("Bip01"))
            .filter_map(|(link, n)| {
                let t = world.get(&link.key)?;
                Some((n.base.base.name.clone(), t.translation.into()))
            })
            .collect();
        bones.sort_by(|a, b| a.0.cmp(&b.0));
        println!(
            "=== {body}: форм со скиннингом {skinned}, узлов Bip01 {} ===",
            bones.len()
        );
        for (name, p) in bones.iter().filter(|(n, _)| {
            [
                "Pelvis",
                "Head",
                "L UpperArm",
                "L Forearm",
                "L Hand",
                "L Calf",
                "L Foot",
            ]
            .iter()
            .any(|k| n.ends_with(k))
        }) {
            println!("  {name:24} ({:8.2}, {:8.2}, {:8.2})", p[0], p[1], p[2]);
        }
    }

    // Полная T-поза: все ли кости base_anim есть в кирасе и совпадают ли они с телом.
    let positions = |path: &str| -> std::collections::BTreeMap<String, tes3::nif::glam::Vec3A> {
        let stream = load(&archive, path);
        let world = stream.world_transforms();
        stream
            .objects_of_type_with_link::<NiNode>()
            .filter_map(|(link, n)| {
                Some((n.base.base.name.clone(), world.get(&link.key)?.translation))
            })
            .collect()
    };
    let cuirass = positions("meshes\\a\\a_art_cuirass_lords_c.nif");
    let body = positions("meshes\\b\\b_n_imperial_m_skins.nif");
    let anim = positions("meshes\\base_anim.nif");
    let missing: Vec<&String> = anim
        .keys()
        .filter(|n| n.starts_with("Bip01") && !cuirass.contains_key(*n))
        .collect();
    let differ: Vec<(String, f32)> = body
        .iter()
        .filter(|(n, _)| n.starts_with("Bip01"))
        .filter_map(|(n, p)| Some((n.clone(), cuirass.get(n)?.distance(*p))))
        .filter(|(_, d)| *d > 0.01)
        .collect();
    println!(
        "=== кираса как скелет: нет костей base_anim {missing:?}; расходятся с телом больше 0.01: {differ:?} ==="
    );
    assert!(missing.is_empty());
    assert!(
        differ.iter().all(|(n, _)| n.contains("Finger")),
        "поза кирасы не совпала с телом вне пальцев"
    );

    let path = "meshes\\a\\a_art_cuirass_lords_c.nif";
    let stream = load(&archive, path);
    println!("=== {path} ===");
    for (_, object) in stream.objects.iter() {
        let name = <&tes3::nif::NiObjectNET>::try_from(object)
            .map(|n| n.name.clone())
            .unwrap_or_default();
        println!("  {:24} {name}", type_name(object));
    }
    let world = stream.world_transforms();
    for shape in stream.objects_of_type::<NiTriShape>() {
        let skin_link = shape.base.base.skin_instance;
        let Some(skin) = stream.get::<NiSkinInstance>(skin_link) else {
            println!("  {} — без скиннинга", shape.base.base.base.base.name);
            continue;
        };
        let root = stream
            .get_as::<_, tes3::nif::NiObjectNET>(skin.root)
            .map(|n| n.name.clone());
        let data = stream.get::<NiSkinData>(skin.data).expect("NiSkinData");
        // Соглашение: skin · корень⁻¹ · кость · обратная_поза должно быть единицей в позе привязки.
        let root_world = world.get(&skin.root.key).copied().expect("корень");
        let shape_link = stream
            .objects_of_type_with_link::<NiTriShape>()
            .find(|(_, s)| std::ptr::eq(*s, shape))
            .map(|(l, _)| l)
            .expect("форма");
        println!(
            "  корень Bip01 в мире: {:?}; форма в мире: {:?}",
            root_world.translation,
            world.get(&shape_link.key).map(|t| t.translation)
        );
        let worst = skin
            .bones
            .iter()
            .zip(&data.bone_data)
            .filter_map(|(b, bd)| {
                let m =
                    data.transform() * root_world.inverse() * *world.get(&b.key)? * bd.transform();
                Some(
                    (m.matrix3 - tes3::nif::glam::Mat3A::IDENTITY)
                        .to_cols_array()
                        .iter()
                        .map(|v| v.abs())
                        .fold(m.translation.length(), f32::max),
                )
            })
            .fold(0.0f32, f32::max);
        println!("  отклонение произведения от единицы: {worst:.5}");
        let shape_world = *world.get(&shape_link.key).expect("форма");
        let rel_shape = root_world.inverse() * shape_world;
        println!(
            "  корень⁻¹·форма: сдвиг {:?} поворот {:?}",
            rel_shape.translation, rel_shape.matrix3
        );
        for (b, bd) in skin.bones.iter().zip(&data.bone_data) {
            let m = shape_world.inverse() * *world.get(&b.key).expect("кость") * bd.transform();
            println!(
                "    форма⁻¹·кость·обратная_поза: сдвиг {:.4?} диагональ ({:.4}, {:.4}, {:.4})",
                m.translation, m.matrix3.x_axis.x, m.matrix3.y_axis.y, m.matrix3.z_axis.z
            );
        }
        if let Some((b, bd)) = skin.bones.iter().zip(&data.bone_data).next() {
            let rel = root_world.inverse() * *world.get(&b.key).expect("кость");
            let inv = bd.transform().inverse();
            println!(
                "  первая кость: корень⁻¹·кость сдвиг {:?} поворот {:?}\n               обратная_поза⁻¹ сдвиг {:?} поворот {:?}",
                rel.translation, rel.matrix3, inv.translation, inv.matrix3
            );
        }
        println!(
            "  форма {}: корень скелета {root:?}, костей {}, общий сдвиг {:?} масштаб {}",
            shape.base.base.base.base.name,
            skin.bones.len(),
            data.translation,
            data.scale
        );
        for (bone, bd) in skin.bones.iter().zip(&data.bone_data) {
            let name = stream
                .get_as::<_, tes3::nif::NiObjectNET>(*bone)
                .map(|n| n.name.clone())
                .unwrap_or_default();
            let in_file = world.get(&bone.key).map(|t| t.translation);
            println!(
                "    {name:22} весов {:4}, сдвиг обратной позы {:?}, кость в файле: {in_file:?}",
                bd.vertex_weights.len(),
                bd.translation
            );
        }
    }
}
