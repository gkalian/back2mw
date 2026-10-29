//! World objects (T2.2): `ACTI` → `ACTI`, `DOOR` → `DOOR`, `CONT` → `CONT`, `LIGH` → `LIGH`, `FLOR`
//! → an organic `CONT`, `FURN` → `ACTI` (Morrowind has no furniture). `GRAS` stays a `STAT` — the
//! shared core does this.
//!
//! Rules and measurements — `docs/en/formats/tes4.md`, "World objects" (2026-09-25):
//!
//! * An Oblivion container's `DATA` is the box's own weight, while Morrowind's `CNDT` is the
//!   capacity. They cannot be carried over into each other: the capacity is 200, the mode and
//!   median of the 769 ordinary containers of `Morrowind.esm`. Plants get 0, like vanilla flora.
//! * The "respawns" flag is 0x02 in both.
//! * Oblivion light flags 0x1…0x8 and 0x20…0x100 mean the same as in Morrowind. Oblivion's 0x10 is
//!   undocumented (in Morrowind it is "fire"), 0x200/0x400 is a spot light, which Morrowind does
//!   not have: these three bits are cleared.
//! * The contents of containers and plants are source `FormID`s: the shared core turns them into
//!   IDs once the whole plugin is read (items can come after the container in the file).

use tes3_out::{Activator, Container, Door, Light, MAX_FIELD_LEN, Object};
use tes4_read::{ContainerItem, WorldData, WorldSource};

use crate::weapons::display_name;

/// Capacity of an ordinary container: the mode and median of `CNDT` over the 769 inorganic
/// containers of `Morrowind.esm` (2026-09-25).
pub const CONTAINER_CAPACITY: f32 = 200.0;

/// Light flags that mean the same in Oblivion and Morrowind.
const SHARED_LIGHT_FLAGS: u32 = 0x1 | 0x2 | 0x4 | 0x8 | 0x20 | 0x40 | 0x80 | 0x100;

/// The "respawns" flag of an Oblivion `CONT`.
const CONTAINER_RESPAWNS: u8 = 0x02;

/// A Morrowind world object and what comes with it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertedWorld {
    pub object: Object,
    /// Notes about losses.
    pub notes: Vec<String>,
    /// Container contents: the `FormID` as in the source file and the count. The core issues the
    /// IDs.
    pub contents: Vec<ContainerItem>,
}

/// Contents of the set's containers and plants (absolute `FormID`s of `CONT`/`FLOR`) and everything
/// reachable from them through the plugin's leveled lists `LVLI`. With `--worldspace` they have to be
/// converted even though they do not stand in the world, like NPC inventory
/// (`crate::npcs::inventory_of`): without them the chests and the lists they use would be emptied
/// (`SEWorld`: 702 of 1,398 lists, 2026-09-27).
pub fn contents_of(
    plugin: &tes4_read::Plugin,
    map: impl Fn(u32) -> Option<u32>,
    wanted: &std::collections::BTreeSet<u32>,
) -> Result<std::collections::BTreeSet<u32>, crate::npcs::ActorError> {
    let mut start = Vec::new();
    for r in plugin
        .records_of(*b"CONT")
        .chain(plugin.records_of(*b"FLOR"))
    {
        if !map(r.header.form_id).is_some_and(|id| wanted.contains(&id)) {
            continue;
        }
        let source = WorldSource::from_record(r)
            .map_err(|e| crate::npcs::ActorError::Read(format!("{:08X}: {e}", r.header.form_id)))?;
        match &source.data {
            WorldData::Container { items, .. } => {
                start.extend(items.iter().filter_map(|i| map(i.form_id)));
            }
            WorldData::Flora {
                ingredient: Some(i),
            } => start.extend(map(*i)),
            _ => {}
        }
    }
    crate::npcs::reachable_items(plugin, &map, start)
}

/// A world object from an Oblivion record; `id`, `mesh`, `icon` are already issued (`mesh` is empty
/// for a light without a model). Sounds are set by the shared core once the whole plugin is read
/// (T10.1).
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
            panic!("container");
        };
        assert_eq!(
            cont.capacity, CONTAINER_CAPACITY,
            "the box's weight is not its capacity"
        );
        assert!(cont.respawns && !cont.organic);
        assert!(cont.items.is_empty(), "the core issues the IDs");
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
            panic!("container");
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
            panic!("light");
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
