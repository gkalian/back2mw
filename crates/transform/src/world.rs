//! Объекты мира (T2.2): `ACTI` → `ACTI`, `DOOR` → `DOOR`, `CONT` → `CONT`, `LIGH` → `LIGH`,
//! `FLOR` → органический `CONT`, `FURN` → `ACTI` (мебели в Morrowind нет). `GRAS` остаётся
//! `STAT` — это делает общее ядро.
//!
//! Правила и измерения — `docs/ru/formats/tes4.md`, «Объекты мира» (2026-09-25):
//!
//! * `DATA` контейнера Oblivion — собственный вес ящика, а у Morrowind `CNDT` — вместимость. Их
//!   нельзя переносить друг в друга: вместимость — 200, мода и медиана 769 обычных контейнеров
//!   `Morrowind.esm`. У растений — 0, как у ванильной флоры.
//! * Флаг «пополняется» у обоих — 0x02.
//! * Флаги света Oblivion 0x1…0x8 и 0x20…0x100 значат то же, что у Morrowind. 0x10 у Oblivion не
//!   описан (у Morrowind это «огонь»), 0x200/0x400 — прожектор, которого у Morrowind нет: эти
//!   три бита снимаются.
//! * Содержимое контейнеров и растений — `FormID` источника: их переводит в ID общее ядро, когда
//!   весь плагин уже прочитан (предметы могут стоять в файле после контейнера).

use tes3_out::{Activator, Container, Door, Light, MAX_FIELD_LEN, Object};
use tes4_read::{ContainerItem, WorldData, WorldSource};

use crate::weapons::display_name;

/// Вместимость обычного контейнера: мода и медиана `CNDT` у 769 неорганических контейнеров
/// `Morrowind.esm` (2026-09-25).
pub const CONTAINER_CAPACITY: f32 = 200.0;

/// Флаги света, которые у Oblivion и Morrowind значат одно и то же.
const SHARED_LIGHT_FLAGS: u32 = 0x1 | 0x2 | 0x4 | 0x8 | 0x20 | 0x40 | 0x80 | 0x100;

/// Флаг «пополняется» у `CONT` Oblivion.
const CONTAINER_RESPAWNS: u8 = 0x02;

/// Объект мира Morrowind и что к нему прилагается.
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertedWorld {
    pub object: Object,
    /// Замечания о потерях.
    pub notes: Vec<String>,
    /// Содержимое контейнера: `FormID` как в файле источника и число. ID выдаёт ядро.
    pub contents: Vec<ContainerItem>,
}

/// Объект мира из записи Oblivion; `id`, `mesh`, `icon` уже выданы (`mesh` пуст у света без
/// модели). Звуки ставит общее ядро, когда весь плагин прочитан (T10.1).
pub fn convert_world(source: &WorldSource, id: &str, mesh: &str, icon: &str) -> ConvertedWorld {
    let mut notes = Vec::new();
    let name = match source.name.as_deref() {
        Some(full) => {
            let (name, cut) = display_name(full, MAX_FIELD_LEN);
            if cut {
                notes.push(lang::tr!(
                    "имя {full:?} обрезано до {name:?}",
                    "name {full:?} truncated to {name:?}"
                ));
            }
            name
        }
        None => String::new(),
    };
    let (id, mesh) = (id.to_owned(), mesh.to_owned());
    let mut contents = Vec::new();
    let object = match &source.data {
        WorldData::Activator | WorldData::Furniture => Object::Activator(Activator {
            id,
            name,
            mesh,
            script: String::new(),
        }),
        WorldData::Door { .. } => Object::Door(Door {
            id,
            name,
            mesh,
            open_sound: String::new(),
            close_sound: String::new(),
        }),
        WorldData::Container { flags, items, .. } => {
            contents.clone_from(items);
            Object::Container(Container {
                id,
                name,
                mesh,
                capacity: CONTAINER_CAPACITY,
                organic: false,
                respawns: flags & CONTAINER_RESPAWNS != 0,
                items: Vec::new(),
            })
        }
        WorldData::Flora { ingredient } => {
            match ingredient {
                Some(form_id) => contents.push(ContainerItem {
                    form_id: *form_id,
                    count: 1,
                }),
                None => notes.push(
                    lang::tr(
                        "растение ничего не даёт: нет ингредиента",
                        "the plant yields nothing: no ingredient",
                    )
                    .to_owned(),
                ),
            }
            Object::Container(Container {
                id,
                name,
                mesh,
                capacity: 0.0,
                organic: true,
                respawns: true,
                items: Vec::new(),
            })
        }
        WorldData::Light(l) => Object::Light(Light {
            id,
            name,
            mesh,
            icon: icon.to_owned(),
            sound: String::new(),
            weight: l.weight,
            value: l.value,
            time: l.time,
            radius: l.radius,
            color: l.color,
            flags: l.flags & SHARED_LIGHT_FLAGS,
        }),
    };
    ConvertedWorld {
        object,
        notes,
        contents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tes4_read::LightData;

    fn source(data: WorldData) -> WorldSource {
        WorldSource {
            editor_id: "Thing".to_owned(),
            name: Some("Thing".to_owned()),
            model: Some("Thing.nif".to_owned()),
            icon: None,
            script: None,
            sound: None,
            close_sound: None,
            data,
        }
    }

    #[test]
    fn furniture_becomes_an_activator_and_doors_stay_doors() {
        let c = convert_world(&source(WorldData::Furniture), "ob_Chair", "m.nif", "");
        assert!(matches!(c.object, Object::Activator(ref a) if a.name == "Thing"));
        let c = convert_world(
            &source(WorldData::Door { flags: 1 }),
            "ob_Door",
            "m.nif",
            "",
        );
        assert!(matches!(c.object, Object::Door(_)));
        assert!(c.notes.is_empty());
    }

    #[test]
    fn a_container_gets_vanilla_capacity_and_keeps_respawn_and_contents() {
        let items = vec![ContainerItem {
            form_id: 0x10,
            count: 2,
        }];
        let c = convert_world(
            &source(WorldData::Container {
                flags: CONTAINER_RESPAWNS,
                weight: 15.0,
                items: items.clone(),
            }),
            "ob_Chest",
            "m.nif",
            "",
        );
        let Object::Container(cont) = &c.object else {
            panic!("контейнер");
        };
        assert_eq!(
            cont.capacity, CONTAINER_CAPACITY,
            "вес ящика — не вместимость"
        );
        assert!(cont.respawns && !cont.organic);
        assert!(cont.items.is_empty(), "ID выдаёт ядро");
        assert_eq!(c.contents, items);
    }

    #[test]
    fn flora_is_an_organic_respawning_container_with_its_ingredient() {
        let c = convert_world(
            &source(WorldData::Flora {
                ingredient: Some(0x42),
            }),
            "ob_Plant",
            "m.nif",
            "",
        );
        let Object::Container(cont) = &c.object else {
            panic!("контейнер");
        };
        assert!(cont.organic && cont.respawns);
        assert_eq!(cont.capacity, 0.0);
        assert_eq!(
            c.contents,
            [ContainerItem {
                form_id: 0x42,
                count: 1
            }]
        );
        let empty = convert_world(
            &source(WorldData::Flora { ingredient: None }),
            "ob_Plant",
            "m.nif",
            "",
        );
        assert_eq!(empty.notes.len(), 1);
    }

    #[test]
    fn light_keeps_shared_flags_only() {
        let c = convert_world(
            &source(WorldData::Light(LightData {
                time: -1,
                radius: 300,
                color: [1, 2, 3, 0],
                flags: 0x1 | 0x8 | 0x10 | 0x20 | 0x200 | 0x400,
                value: 5,
                weight: 1.5,
            })),
            "ob_Light",
            "",
            "ob\\torch_0123456789.dds",
        );
        let Object::Light(l) = &c.object else {
            panic!("свет");
        };
        assert_eq!(l.flags, 0x1 | 0x8 | 0x20);
        assert_eq!((l.time, l.radius, l.value), (-1, 300, 5));
        assert!(l.mesh.is_empty());
        assert_eq!(l.icon, "ob\\torch_0123456789.dds");
    }

    #[test]
    fn an_unnamed_record_stays_unnamed() {
        let mut s = source(WorldData::Activator);
        s.name = None;
        let c = convert_world(&s, "ob_Trigger", "m.nif", "");
        assert!(matches!(c.object, Object::Activator(ref a) if a.name.is_empty()));
    }
}
