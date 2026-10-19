//! Меши на теле (броня и одежда, T9.2c): скелет Morrowind и конвертация со скиннингом.
//!
//! Скелет берётся из установки Morrowind игрока (`tes3_out::vanilla`): иерархия и набор костей —
//! `base_anim.nif` (`base_anim_female.nif`), позы привязки — тело `b\b_n_imperial_*_skins.nif`,
//! а недостающие телу кости ног, ступней и пальцев — полная T-поза ванильной кирасы
//! `a\a_art_cuirass_lords_c.nif`. Её позы совпадают с телом во всех общих костях, кроме пальцев
//! (другая поза кисти) — тест `mesh_convert::real_skinning`. Почему не `base_anim` целиком: его
//! узлы стоят в позе анимации, а не привязки.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use mesh_convert::geometry::convert_with;
use mesh_convert::read::{Block, parse};
use mesh_convert::skin::{
    Side as MeshSide, TargetSkeleton, convert_skinned, drop_skin, keep_side, name_shapes,
};
use mesh_convert::textures::TextureRegistry;
use transform::worn::{BodyMesh, Side, bone_slots, is_body_skin_texture};

use crate::extract::extracted_path;
use crate::pipeline::MeshResult;

/// Файлы Morrowind, из которых строится скелет: (иерархия, тело) для мужчин и женщин.
pub const SKELETON_FILES: [(&str, &str); 2] = [
    (
        "meshes\\base_anim.nif",
        "meshes\\b\\b_n_imperial_m_skins.nif",
    ),
    (
        "meshes\\base_anim_female.nif",
        "meshes\\b\\b_n_imperial_f_skins.nif",
    ),
];
/// Полная T-поза: кости, которых нет у тела.
pub const FULL_POSE_FILE: &str = "meshes\\a\\a_art_cuirass_lords_c.nif";

/// Мужской и женский скелеты Morrowind.
#[derive(Debug, Clone)]
pub struct Skeletons {
    pub male: TargetSkeleton,
    pub female: TargetSkeleton,
}

/// Строит скелеты из `Data Files` Morrowind. Ошибка — текст для отчёта.
pub fn load(target_data: &Path) -> Result<Skeletons, String> {
    let read = |p: &str| tes3_out::vanilla::read_file(target_data, p).map_err(|e| e.to_string());
    let full = read(FULL_POSE_FILE)?;
    let build = |(anim, body): (&str, &str)| -> Result<TargetSkeleton, String> {
        let hierarchy = read(anim)?;
        let body = read(body)?;
        TargetSkeleton::from_bytes(&hierarchy, &[&body, &full]).map_err(|e| e.to_string())
    };
    Ok(Skeletons {
        male: build(SKELETON_FILES[0])?,
        female: build(SKELETON_FILES[1])?,
    })
}

/// Конвертирует меш на теле: скиннинговый — на скелет нужного пола и, если задана сторона,
/// только эту половину; жёсткий (шлем, щит) — как обычный меш.
pub fn convert(
    work_dir: &Path,
    body: &BodyMesh,
    skeletons: &Skeletons,
    registry: &mut TextureRegistry,
) -> Result<MeshResult, String> {
    let path =
        extracted_path(work_dir, &format!("meshes\\{}", body.model)).map_err(|e| e.to_string())?;
    let bytes = fs::read(&path)
        .map_err(|_| lang::tr("нет в архиве мешей", "not in the mesh archive").to_owned())?;
    let source = parse(&bytes).map_err(|e| lang::tr!("чтение: {e}", "read: {e}"))?;
    let skinned = source
        .blocks
        .iter()
        .any(|b| matches!(b, Block::SkinInstance(_)));
    let mut exposed_slots = BTreeSet::new();
    let mut converted = if skinned {
        let skeleton = if body.female {
            &skeletons.female
        } else {
            &skeletons.male
        };
        let mut c = convert_skinned(&source, registry, skeleton)
            .map_err(|e| lang::tr!("скиннинг: {e}", "skinning: {e}"))?;
        if let Some(side) = body.side {
            let side = match side {
                Side::Left => MeshSide::Left,
                Side::Right => MeshSide::Right,
            };
            if keep_side(&mut c.stream, side) == 0 {
                return Err(lang::tr(
                    "в этой половине меша не осталось ни одного треугольника",
                    "no triangles left in this half of the mesh",
                )
                .to_owned());
            }
        }
        // Кожа Oblivion — только на своём слоте части, остальное тело даёт Morrowind.
        let skin: BTreeSet<String> = c
            .textures
            .iter()
            .filter(|t| is_body_skin_texture(&t.source_path))
            .map(|t| t.target_name.to_ascii_lowercase())
            .collect();
        if !skin.is_empty() {
            let dropped = drop_skin(&mut c.stream, &skin, |bone| {
                bone_slots(bone).contains(&body.slot)
            });
            exposed_slots = dropped
                .iter()
                .flat_map(|b| bone_slots(b).iter().copied())
                .filter(|&s| s != body.slot)
                .collect();
        }
        name_shapes(&mut c.stream, body.node);
        c
    } else {
        convert_with(&source, registry)
            .map_err(|e| lang::tr!("конвертация: {e}", "conversion: {e}"))?
    };
    let out = converted
        .save_bytes()
        .map_err(|e| lang::tr!("запись NIF: {e}", "NIF write: {e}"))?;
    Ok(MeshResult {
        bytes: out,
        textures: converted.textures,
        generated: Vec::new(),
        exposed_slots,
    })
}
