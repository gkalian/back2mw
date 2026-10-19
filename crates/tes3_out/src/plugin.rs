use std::io;
use std::path::Path;

use tes3::esp::{FileType, Header, Plugin, TES3Object};

use crate::model::{self, Object};
use crate::records;
use crate::world;

/// Предел длины строковых полей `MODL`, `ITEX`, `FNAM`: 31 знак плюс нуль.
///
/// Для `MODL` предел показал сам Construction Set: длиннее он обрезает с предупреждением
/// `Chunk size … too big in chunk MODL_ID … Max size is 32` (2026-09-24). В ванили максимум по
/// всем трём полям — 31 знак (`docs/ru/formats/tes3.md`). Для `ITEX` и `FNAM` предел CS не
/// измерялся; 31 — нижняя граница, которую ваниль заведомо держит.
pub const MAX_FIELD_LEN: usize = 31;

/// Собирает плагин Morrowind.
///
/// Мастером остаётся `Morrowind.esm`: мы только добавляем записи и ни одной
/// ванильной не переопределяем.
///
/// # Кодировка
///
/// Все строки в плагине TES3 — **Windows-1252**, и кириллицы в них не бывает.
/// Библиотека отказывает на записи, а не портит данные молча, поэтому любой
/// текст, который мы туда кладём (автор, описание, ID, пути), обязан быть
/// представим в cp1252. Проверено тестом [`tests::rejects_text_outside_cp1252`].
/// На фазе 1 это касается нашей собственной метаинформации; когда пойдёт
/// настоящий контент, непредставимые символы придётся обрабатывать осознанно.
#[derive(Debug, Clone)]
pub struct PluginBuilder {
    header: Header,
    objects: Vec<TES3Object>,
}

impl PluginBuilder {
    pub fn new(author: &str, description: &str) -> Self {
        // FixedString принимает String, а не &str: это фиксированные буферы
        // 32 и 256 байт, обрезка до размера — забота библиотеки.
        let header = Header {
            author: author.to_owned().into(),
            description: description.to_owned().into(),
            ..Header::default()
        };
        Self {
            header,
            objects: Vec::new(),
        }
    }

    /// Мастер-файл, на который опирается плагин. Размер нужен движку для
    /// проверки, что подложили тот же файл.
    #[must_use]
    pub fn with_master(mut self, name: &str, size: u64) -> Self {
        self.header.masters.push((name.to_owned(), size));
        self
    }

    /// Пометить файл мастером (`HEDR` тип 1). Нужен, когда источник — `.esm`: у Morrowind
    /// расширение и тип в заголовке должны совпадать, иначе CS и лаунчер видят не то.
    #[must_use]
    pub fn as_master(mut self) -> Self {
        self.header.file_type = FileType::Esm;
        self
    }

    pub fn add(&mut self, object: impl Into<Object>) {
        self.objects.push(records::to_tes3(object.into()));
    }

    pub fn add_static(&mut self, id: impl Into<String>, mesh: impl Into<String>) {
        self.add(model::Static {
            id: id.into(),
            mesh: mesh.into(),
        });
    }

    pub fn add_leveled_item(&mut self, list: model::LeveledItem) {
        self.objects.push(records::leveled_item(list));
    }

    /// Список существ (T11.4). Кладётся после существ и NPC и раньше ячеек, как в
    /// `Morrowind.esm`.
    pub fn add_leveled_creature(&mut self, list: model::LeveledCreature) {
        self.objects.push(records::leveled_creature(list));
    }

    /// Глобальная переменная (T11.1). `Long`, который `f32` не держит точно, — ошибка при записи
    /// (проверка `tes3`), поэтому значения готовит `transform`.
    pub fn add_global(&mut self, global: model::Global) {
        self.objects.push(records::global(global));
    }

    pub fn add_enchantment(&mut self, enchantment: model::Enchantment) {
        self.objects.push(records::enchantment(enchantment));
    }

    /// Звук. Кладётся раньше объектов, которые на него ссылаются, — как в `Morrowind.esm`.
    pub fn add_sound(&mut self, sound: model::Sound) {
        self.objects.push(records::sound(sound));
    }

    /// Звук существа (T11.6). Кладётся после `SOUN`, на который ссылается. Вид вне 0…7 не пишется.
    pub fn add_sound_gen(&mut self, gen_: model::SoundGen) {
        self.objects.extend(records::sound_gen(gen_));
    }

    /// Скрипт по шаблону (T10.3). Кладётся после `SOUN` и раньше объектов, как в `Morrowind.esm`.
    /// Ошибка — ID или имя не ложится в шаблон (`crate::scripts`).
    pub fn add_script(&mut self, script: &crate::scripts::Script) -> io::Result<()> {
        self.objects.push(crate::scripts::script(script)?);
        Ok(())
    }

    pub fn add_spell(&mut self, spell: model::Spell) {
        self.objects.push(records::spell(spell));
    }

    pub fn add_land_texture(&mut self, texture: world::LandTexture) {
        self.objects.push(world::land_texture(texture));
    }

    pub fn add_region(&mut self, region: world::Region) {
        self.objects.push(world::region(region));
    }

    /// Ячейка со ссылками. Записи ложатся в порядке вызовов: `LTEX` и `REGN` — раньше ячеек,
    /// `LAND` — после, как в `Morrowind.esm`.
    pub fn add_cell(&mut self, cell: world::ExteriorCell) {
        self.objects.push(world::cell(cell));
    }

    /// Внутренняя ячейка со ссылками.
    pub fn add_interior(&mut self, cell: world::InteriorCell) {
        self.objects.push(world::interior_cell(cell));
    }

    pub fn add_npc(&mut self, npc: crate::actors::Npc) {
        self.objects.push(crate::actors::npc(npc));
    }

    pub fn add_creature(&mut self, creature: crate::actors::Creature) {
        self.objects.push(crate::actors::creature(creature));
    }

    pub fn add_faction(&mut self, faction: crate::actors::Faction) {
        self.objects.push(crate::actors::faction(faction));
    }

    pub fn add_class(&mut self, class: crate::actors::Class) {
        self.objects.push(crate::actors::class(class));
    }

    pub fn add_race(&mut self, race: crate::actors::Race) {
        self.objects.push(crate::actors::race(race));
    }

    /// Тема и её реплики (T10.6). Темы кладутся последними, как в `Morrowind.esm`. Ошибка — реплика
    /// не ложится в пределы формата (`crate::dialogue`).
    pub fn add_topic(&mut self, topic: crate::dialogue::Topic) -> io::Result<()> {
        self.objects.extend(crate::dialogue::topic(topic)?);
        Ok(())
    }

    pub fn add_land(&mut self, land: world::Land) {
        self.objects.push(world::land(land));
    }

    /// Сетка путей — после своей ячейки: OpenMW узнаёт интерьер по уже загруженной ячейке с тем же
    /// именем (`docs/ru/formats/tes3.md`, «Сетки путей»). В `Morrowind.esm` `PGRD` идёт сразу за своими
    /// `CELL`/`LAND`; ванильному движку, возможно, важно и это — не проверено.
    pub fn add_path_grid(&mut self, grid: world::PathGrid) {
        self.objects.push(world::path_grid(grid));
    }

    pub fn objects_len(&self) -> usize {
        self.objects.len()
    }

    /// `num_objects` в заголовке считается без самого заголовка.
    pub fn into_plugin(mut self) -> Plugin {
        self.header.num_objects =
            u32::try_from(self.objects.len()).expect("число записей не превышает u32");

        let mut objects = Vec::with_capacity(self.objects.len() + 1);
        objects.push(TES3Object::Header(self.header));
        objects.extend(self.objects);
        Plugin { objects }
    }

    /// Байты плагина. Поле длиннее [`MAX_FIELD_LEN`] — ошибка `InvalidData`, а не обрезка:
    /// обрезанный путь указывает на несуществующий файл.
    pub fn to_bytes(self) -> io::Result<Vec<u8>> {
        self.check_limits()?;
        self.into_plugin().save_bytes()
    }

    pub fn save(self, path: impl AsRef<Path>) -> io::Result<()> {
        self.check_limits()?;
        self.into_plugin().save_path(path)
    }

    fn check_limits(&self) -> io::Result<()> {
        for object in &self.objects {
            for (tag, id, value) in records::limited_fields(object) {
                // Считаем знаки, а не байты UTF-8: в файле cp1252, один знак — один байт.
                let len = value.chars().count();
                if len > MAX_FIELD_LEN {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        lang::tr!(
                            "{id}: {tag} длиной {len} больше предела {MAX_FIELD_LEN}: {value:?}",
                            "{id}: {tag} of length {len} exceeds the limit {MAX_FIELD_LEN}: {value:?}"
                        ),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArmorKind, WeaponKind};
    use tes3::esp::{
        ArmorType, AttributeId, AttributeId2, BipedObjectType, BodypartFlags, BodypartId,
        BodypartType, EffectId, EffectId2, SkillId, SkillId2, Static, WeaponFlags, WeaponType,
    };

    /// Броня и её часть тела: слот, `BODY`, тип, флаг «женская» доходят до файла.
    #[test]
    fn armor_with_its_body_part_reads_back() {
        let mut builder = PluginBuilder::new("back2mw", "");
        builder.add(model::BodyPart {
            id: "obf_0000abc".to_owned(),
            mesh: "ob\\cuirass_0123456789.nif".to_owned(),
            part: 3,
            kind: model::BodyPartKind::Armor,
            female: true,
            race: String::new(),
        });
        builder.add(model::Armor {
            id: "ob_IronCuirass_0000abc".to_owned(),
            name: "Iron Cuirass".to_owned(),
            mesh: "ob\\cuirass_gnd_0123456789.nif".to_owned(),
            icon: String::new(),
            enchantment: String::new(),
            kind: ArmorKind::Cuirass,
            weight: 30.0,
            value: 85,
            health: 284,
            enchant_capacity: 160,
            rating: 10,
            parts: vec![model::WornPart {
                slot: 3,
                male: String::new(),
                female: "obf_0000abc".to_owned(),
            }],
        });
        let bytes = builder.to_bytes().unwrap();
        let mut parsed = Plugin::new();
        parsed.load_bytes(&bytes).unwrap();
        let armor = parsed
            .objects
            .iter()
            .find_map(|o| match o {
                TES3Object::Armor(a) => Some(a),
                _ => None,
            })
            .expect("ARMO");
        assert_eq!(armor.data.armor_type, ArmorType::Cuirass);
        assert_eq!(armor.data.armor_rating, 10);
        assert_eq!(armor.biped_objects.len(), 1);
        assert_eq!(
            armor.biped_objects[0].biped_object_type,
            BipedObjectType::Chest
        );
        assert_eq!(armor.biped_objects[0].female_bodypart, "obf_0000abc");
        let body = parsed
            .objects
            .iter()
            .find_map(|o| match o {
                TES3Object::Bodypart(b) => Some(b),
                _ => None,
            })
            .expect("BODY");
        assert_eq!(body.data.part, BodypartId::Chest);
        assert_eq!(body.data.bodypart_type, BodypartType::Armor);
        assert!(body.data.flags.contains(BodypartFlags::FEMALE));
    }

    /// Эффекты (T2.1b): у зелья — `ENAM` целиком, у ингредиента — номер, навык и атрибут в `IRDT`.
    #[test]
    fn potion_and_ingredient_effects_read_back() {
        let fortify_blade = model::Effect {
            id: 83,
            skill: Some(5),
            attribute: None,
            range: 0,
            area: 0,
            duration: 30,
            magnitude: [5, 5],
        };
        let restore_str = model::Effect {
            id: 74,
            skill: None,
            attribute: Some(0),
            range: 0,
            area: 0,
            duration: 1,
            magnitude: [3, 3],
        };
        let item = |id: &str, kind| {
            Object::Item(model::Item {
                id: id.to_owned(),
                name: id.to_owned(),
                mesh: String::new(),
                icon: String::new(),
                weight: 0.5,
                value: 10,
                kind,
            })
        };
        let mut builder = PluginBuilder::new("back2mw", "");
        builder.add(item(
            "ob_Potion_0000001",
            model::ItemKind::Potion {
                effects: vec![fortify_blade, restore_str],
            },
        ));
        builder.add(item(
            "ob_Root_0000002",
            model::ItemKind::Ingredient {
                effects: vec![restore_str, fortify_blade],
            },
        ));
        let bytes = builder.to_bytes().unwrap();
        let mut parsed = Plugin::new();
        parsed.load_bytes(&bytes).unwrap();
        let mut seen = 0;
        for o in &parsed.objects {
            match o {
                TES3Object::Alchemy(a) => {
                    seen += 1;
                    assert_eq!(a.effects.len(), 2);
                    assert_eq!(a.effects[0].magic_effect, EffectId2::FortifySkill);
                    assert_eq!(a.effects[0].skill, SkillId2::LongBlade);
                    assert_eq!(a.effects[0].attribute, AttributeId2::None);
                    assert_eq!((a.effects[0].duration, a.effects[0].min_magnitude), (30, 5));
                    assert_eq!(a.effects[1].magic_effect, EffectId2::RestoreAttribute);
                    assert_eq!(a.effects[1].attribute, AttributeId2::Strength);
                }
                TES3Object::Ingredient(i) => {
                    seen += 1;
                    assert_eq!(
                        i.data.effects,
                        [
                            EffectId::RestoreAttribute,
                            EffectId::FortifySkill,
                            EffectId::None,
                            EffectId::None
                        ]
                    );
                    assert_eq!(i.data.attributes[0], AttributeId::Strength);
                    assert_eq!(i.data.skills[1], SkillId::LongBlade);
                    assert_eq!(i.data.skills[2], SkillId::None);
                }
                _ => {}
            }
        }
        assert_eq!(seen, 2);
    }

    /// Уровневый список (T2.5): флаги, шанс и строки доходят до файла. Флаги разные, чтобы
    /// перепутанные биты 0x1/0x2 не прошли.
    /// `GLOB`: тип и значение доходят до файла; `Long`, который `f32` не держит, не пишется.
    #[test]
    fn globals_read_back_and_imprecise_long_is_an_error() {
        let mut builder = PluginBuilder::new("back2mw", "");
        for (id, value) in [
            ("ob_SEKnightSpawnTime_1", model::GlobalValue::Short(4)),
            ("ob_SECrimeGold_2", model::GlobalValue::Long(16_777_216)),
            ("ob_CrimeForceJail_3", model::GlobalValue::Float(5000.5)),
        ] {
            builder.add_global(model::Global {
                id: id.to_owned(),
                value,
            });
        }
        let bytes = builder.to_bytes().unwrap();
        let mut parsed = Plugin::new();
        parsed.load_bytes(&bytes).unwrap();
        let globals: Vec<_> = parsed
            .objects
            .iter()
            .filter_map(|o| match o {
                TES3Object::GlobalVariable(g) => Some((g.id.as_str(), g.value)),
                _ => None,
            })
            .collect();
        assert_eq!(
            globals,
            [
                ("ob_SEKnightSpawnTime_1", tes3::esp::GlobalValue::Short(4)),
                ("ob_SECrimeGold_2", tes3::esp::GlobalValue::Long(16_777_216)),
                ("ob_CrimeForceJail_3", tes3::esp::GlobalValue::Float(5000.5)),
            ]
        );

        let mut builder = PluginBuilder::new("back2mw", "");
        builder.add_global(model::Global {
            id: "ob_Big_1".to_owned(),
            value: model::GlobalValue::Long(16_777_217),
        });
        assert!(builder.to_bytes().is_err(), "2^24 + 1 в f32 не помещается");
    }

    /// `CREA`: флаги по битам Morrowind, кровь, статы, три атаки и `CNAM` доходят до файла.
    #[test]
    fn creature_reads_back() {
        use tes3::esp::{Creature as EspCreature, CreatureFlags, CreatureType};
        let mut builder = PluginBuilder::new("back2mw", "");
        builder.add_creature(crate::actors::Creature {
            id: "ob_CreatureSkeleton1_a".to_owned(),
            name: "Skeleton".to_owned(),
            mesh: "r/Skeleton.NIF".to_owned(),
            sound: "skeleton".to_owned(),
            kind: 2,
            level: 3,
            attributes: [50, 30, 35, 20, 12, 60, 10, 25],
            health: 80,
            magicka: 10,
            fatigue: 165,
            soul: 30,
            combat: 40,
            magic: 5,
            stealth: 5,
            attack: (4, 10),
            gold: 0,
            biped: true,
            weapon_and_shield: true,
            swims: false,
            flies: false,
            walks: false,
            essential: true,
            respawn: true,
            blood_type: 1,
            scale: Some(1.2),
            inventory: vec![(1, "ob_Bone_b".to_owned())],
            spells: vec!["ob_Ability_c".to_owned()],
            ai: crate::actors::NpcAi {
                hello: 30,
                fight: 90,
                flee: 0,
                alarm: 0,
                services: 0,
                wander_distance: 512,
                wander_duration: 5,
                idles: [60, 20, 10, 10, 0, 0, 0, 0],
            },
        });
        let bytes = builder.to_bytes().unwrap();
        let mut parsed = Plugin::new();
        parsed.load_bytes(&bytes).unwrap();
        let c: Vec<&EspCreature> = parsed.objects_of_type::<EspCreature>().collect();
        assert_eq!(c.len(), 1);
        let c = c[0];
        assert_eq!(
            c.creature_flags,
            CreatureFlags::IS_BASE
                | CreatureFlags::BIPED
                | CreatureFlags::WEAPON_AND_SHIELD
                | CreatureFlags::ESSENTIAL
                | CreatureFlags::RESPAWN
        );
        assert_eq!(c.blood_type, 1);
        assert_eq!(
            (c.sound.as_str(), c.mesh.as_str()),
            ("skeleton", "r/Skeleton.NIF")
        );
        assert_eq!(c.data.creature_type, CreatureType::Undead);
        assert_eq!(
            (
                c.data.level,
                c.data.health,
                c.data.magicka,
                c.data.fatigue,
                c.data.soul
            ),
            (3, 80, 10, 165, 30)
        );
        assert_eq!((c.data.strength, c.data.luck), (50, 25));
        assert_eq!(
            [c.data.attack1, c.data.attack2, c.data.attack3],
            [(4, 10); 3]
        );
        assert_eq!(c.scale, Some(1.2));
        assert_eq!(c.spells, ["ob_Ability_c"]);
        assert_eq!(c.inventory.len(), 1);
    }

    #[test]
    fn leveled_item_reads_back() {
        let mut builder = PluginBuilder::new("back2mw", "");
        builder.add_leveled_item(model::LeveledItem {
            id: "ob_LL0Loot_a".to_owned(),
            all_levels: true,
            each_item: false,
            chance_none: 25,
            items: vec![("Gold_025".to_owned(), 1), ("ob_Sword_b".to_owned(), 12)],
        });
        let bytes = builder.to_bytes().unwrap();
        let mut parsed = Plugin::new();
        parsed.load_bytes(&bytes).unwrap();
        let lists: Vec<_> = parsed
            .objects
            .iter()
            .filter_map(|o| match o {
                TES3Object::LeveledItem(l) => Some(l),
                _ => None,
            })
            .collect();
        assert_eq!(lists.len(), 1);
        let l = lists[0];
        assert_eq!(l.id, "ob_LL0Loot_a");
        assert_eq!(
            l.leveled_item_flags.bits(),
            0x2,
            "0x2 — все уровни ≤ игрока"
        );
        assert_eq!(l.chance_none, 25);
        assert_eq!(
            l.items,
            [("Gold_025".to_owned(), 1), ("ob_Sword_b".to_owned(), 12)]
        );
    }

    #[test]
    fn leveled_creature_reads_back() {
        let mut builder = PluginBuilder::new("back2mw", "");
        builder.add_leveled_creature(model::LeveledCreature {
            id: "ob_LL1Rat_a".to_owned(),
            all_levels: true,
            chance_none: 10,
            creatures: vec![("ob_Rat_b".to_owned(), 1), ("ob_Bandit_c".to_owned(), 7)],
        });
        builder.add_leveled_creature(model::LeveledCreature {
            id: "ob_LL1Nearest_d".to_owned(),
            all_levels: false,
            chance_none: 0,
            creatures: vec![("ob_LL1Rat_a".to_owned(), 3)],
        });
        let bytes = builder.to_bytes().unwrap();
        let mut parsed = Plugin::new();
        parsed.load_bytes(&bytes).unwrap();
        let lists: Vec<_> = parsed
            .objects_of_type::<tes3::esp::LeveledCreature>()
            .collect();
        assert_eq!(lists.len(), 2);
        assert_eq!(lists[0].id, "ob_LL1Rat_a");
        assert_eq!(
            lists[0].leveled_creature_flags.bits(),
            0x1,
            "у LEVC «все уровни» — 0x1, а не 0x2, как у LEVI"
        );
        assert_eq!(lists[0].chance_none, 10);
        assert_eq!(
            lists[0].creatures,
            [("ob_Rat_b".to_owned(), 1), ("ob_Bandit_c".to_owned(), 7)]
        );
        assert_eq!(lists[1].leveled_creature_flags.bits(), 0);
        assert_eq!(lists[1].creatures, [("ob_LL1Rat_a".to_owned(), 3)]);
    }

    /// Объекты мира (T2.2): флаги контейнера, содержимое, свет без меша, звуки (T10.1) доходят до
    /// файла.
    #[test]
    fn world_objects_read_back() {
        let mut builder = PluginBuilder::new("back2mw", "");
        builder.add_sound(model::Sound {
            id: "ob_DoorOpen_0000009".to_owned(),
            file: "ob\\dooropen_0123456789.wav".to_owned(),
            volume: 200,
            min_range: 15,
            max_range: 40,
        });
        builder
            .add_script(&crate::Script::LoopSound {
                id: "obls_DoorOpen_0000009".to_owned(),
                sound: "ob_DoorOpen_0000009".to_owned(),
            })
            .unwrap();
        builder.add(Object::Activator(model::Activator {
            id: "ob_Chair_0000001".to_owned(),
            name: "Chair".to_owned(),
            mesh: "ob\\chair_0123456789.nif".to_owned(),
            script: "obls_DoorOpen_0000009".to_owned(),
        }));
        builder.add(Object::Door(model::Door {
            id: "ob_Door_0000002".to_owned(),
            name: "Door".to_owned(),
            mesh: "ob\\door_0123456789.nif".to_owned(),
            open_sound: "ob_DoorOpen_0000009".to_owned(),
            close_sound: String::new(),
        }));
        builder.add(Object::Container(model::Container {
            id: "ob_Plant_0000003".to_owned(),
            name: "Plant".to_owned(),
            mesh: "ob\\plant_0123456789.nif".to_owned(),
            capacity: 0.0,
            organic: true,
            respawns: true,
            items: vec![
                (1, "ob_Root_0000004".to_owned()),
                (-2, "Misc_SoulGem_Petty".to_owned()),
            ],
        }));
        builder.add(Object::Light(model::Light {
            id: "ob_Light_0000005".to_owned(),
            name: String::new(),
            mesh: String::new(),
            icon: String::new(),
            sound: "ob_DoorOpen_0000009".to_owned(),
            weight: 0.0,
            value: 0,
            time: -1,
            radius: 512,
            color: [255, 200, 100, 0],
            flags: 0x49,
        }));
        let bytes = builder.to_bytes().unwrap();
        let mut parsed = Plugin::new();
        parsed.load_bytes(&bytes).unwrap();
        let mut kinds = Vec::new();
        for o in &parsed.objects {
            match o {
                TES3Object::Activator(a) => {
                    assert_eq!(a.script, "obls_DoorOpen_0000009");
                    kinds.push(("ACTI", a.name.clone()));
                }
                TES3Object::Script(s) => {
                    assert_eq!(s.header.bytecode_length as usize, s.bytecode.len());
                    assert_eq!(s.bytecode.len(), 46 + 2 * "ob_DoorOpen_0000009".len());
                    assert!(s.text.contains("PlayLoopSound3DVP"), "{}", s.text);
                    kinds.push(("SCPT", s.id.clone()));
                }
                TES3Object::Door(d) => {
                    assert_eq!(d.open_sound, "ob_DoorOpen_0000009");
                    assert!(d.close_sound.is_empty());
                    kinds.push(("DOOR", d.name.clone()));
                }
                TES3Object::Sound(s) => {
                    assert_eq!(s.sound_path, "ob\\dooropen_0123456789.wav");
                    assert_eq!((s.data.volume, s.data.range), (200, (15, 40)));
                    kinds.push(("SOUN", s.id.clone()));
                }
                TES3Object::Container(c) => {
                    assert_eq!(
                        c.container_flags.bits(),
                        0x0B,
                        "органический, пополняется, 0x8"
                    );
                    let items: Vec<(i32, &str)> = c
                        .inventory
                        .iter()
                        .map(|(n, id)| (*n, id.0.as_str()))
                        .collect();
                    assert_eq!(items, [(1, "ob_Root_0000004"), (-2, "Misc_SoulGem_Petty")]);
                    kinds.push(("CONT", c.name.clone()));
                }
                TES3Object::Light(l) => {
                    assert!(l.mesh.is_empty());
                    assert_eq!((l.data.time, l.data.radius), (-1, 512));
                    assert_eq!(l.data.color, [255, 200, 100, 0]);
                    assert_eq!(l.data.flags.bits(), 0x49);
                    assert_eq!(l.sound, "ob_DoorOpen_0000009");
                    kinds.push(("LIGH", l.name.clone()));
                }
                _ => {}
            }
        }
        assert_eq!(
            kinds,
            [
                ("SOUN", "ob_DoorOpen_0000009".to_owned()),
                ("SCPT", "obls_DoorOpen_0000009".to_owned()),
                ("ACTI", "Chair".to_owned()),
                ("DOOR", "Door".to_owned()),
                ("CONT", "Plant".to_owned()),
                ("LIGH", String::new()),
            ]
        );
    }

    #[test]
    fn writes_a_plugin_that_reads_back() {
        let mut builder = PluginBuilder::new("back2mw", "converted from Oblivion")
            .with_master("Morrowind.esm", 0);
        builder.add_static("ob_Rock01_0000001", "ob\\Rock01.nif");
        builder.add_static("ob_Bowl01_0000002", "ob\\Bowl01.nif");

        let bytes = builder.to_bytes().unwrap();
        assert_eq!(&bytes[..4], b"TES3", "плагин начинается записью TES3");

        let mut parsed = Plugin::new();
        parsed.load_bytes(&bytes).unwrap();

        let statics: Vec<&Static> = parsed
            .objects
            .iter()
            .filter_map(|o| match o {
                TES3Object::Static(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(statics.len(), 2);
        assert_eq!(statics[0].id, "ob_Rock01_0000001");
        assert_eq!(statics[0].mesh, "ob\\Rock01.nif");
    }

    #[test]
    fn header_counts_objects_without_itself() {
        let mut builder = PluginBuilder::new("back2mw", "");
        builder.add_static("a", "a.nif");
        builder.add_static("b", "b.nif");
        let plugin = builder.into_plugin();

        let header = plugin
            .objects
            .iter()
            .find_map(|o| match o {
                TES3Object::Header(h) => Some(h),
                _ => None,
            })
            .expect("заголовок на месте");
        assert_eq!(header.num_objects, 2);
        assert_eq!(plugin.objects.len(), 3, "заголовок плюс две записи");
    }

    /// Документирует ограничение кодировки: строки TES3 — cp1252, и попытка
    /// записать то, что в неё не влезает, должна падать, а не портить файл.
    #[test]
    fn rejects_text_outside_cp1252() {
        let err = PluginBuilder::new("back2mw", "описание")
            .to_bytes()
            .expect_err("кириллица не представима в cp1252");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    fn iron_dagger() -> model::Weapon {
        model::Weapon {
            id: "ob_WeapIronDagger_0000abc".to_owned(),
            name: "Iron Dagger".to_owned(),
            mesh: "ob\\dagger_0123456789.nif".to_owned(),
            icon: "ob\\irondagger_0123456789.dds".to_owned(),
            enchantment: String::new(),
            stats: model::WeaponStats {
                kind: WeaponKind::ShortBladeOneHand,
                weight: 3.0,
                value: 6,
                health: 400,
                speed: 2.5,
                reach: 1.0,
                enchant_capacity: 40,
                chop: [3, 5],
                slash: [3, 5],
                thrust: [3, 5],
                ignores_resistance: true,
            },
        }
    }

    #[test]
    fn writes_a_weapon_that_reads_back_field_for_field() {
        let mut builder = PluginBuilder::new("back2mw", "");
        builder.add(iron_dagger());
        let bytes = builder.to_bytes().unwrap();

        let mut parsed = Plugin::new();
        parsed.load_bytes(&bytes).unwrap();
        let w = parsed
            .objects
            .iter()
            .find_map(|o| match o {
                TES3Object::Weapon(w) => Some(w),
                _ => None,
            })
            .expect("WEAP на месте");
        assert_eq!(w.id, "ob_WeapIronDagger_0000abc");
        assert_eq!(w.name, "Iron Dagger");
        assert_eq!(w.mesh, "ob\\dagger_0123456789.nif");
        assert_eq!(w.icon, "ob\\irondagger_0123456789.dds");
        assert_eq!(w.data.weapon_type, WeaponType::ShortBladeOneHand);
        assert_eq!(w.data.health, 400);
        assert_eq!(w.data.enchantment, 40);
        assert_eq!((w.data.thrust_min, w.data.thrust_max), (3, 5));
        assert!(
            w.data
                .flags
                .contains(WeaponFlags::IGNORES_NORMAL_WEAPON_RESISTANCE)
        );
    }

    /// Ровно предел проходит, на знак больше — отказ. Без первой половины тест прошёл бы и
    /// на проверке, которая отвергает всё.
    #[test]
    fn a_path_longer_than_the_cs_buffer_is_rejected_not_truncated() {
        let mut ok = PluginBuilder::new("back2mw", "");
        ok.add_static("a", "x".repeat(MAX_FIELD_LEN));
        ok.to_bytes().expect("31 знак влезает");

        let mut long = PluginBuilder::new("back2mw", "");
        long.add_static("a", "x".repeat(MAX_FIELD_LEN + 1));
        let err = long.to_bytes().expect_err("32 знака не влезают");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        let mut icon = PluginBuilder::new("back2mw", "");
        icon.add(model::Weapon {
            icon: "i".repeat(MAX_FIELD_LEN + 1),
            ..iron_dagger()
        });
        assert!(icon.to_bytes().is_err(), "ITEX проверяется тоже");
    }

    #[test]
    fn a_master_file_is_marked_in_the_header_and_a_plugin_is_not() {
        let header = |p: Plugin| {
            p.objects
                .into_iter()
                .find_map(|o| match o {
                    TES3Object::Header(h) => Some(h.file_type),
                    _ => None,
                })
                .expect("заголовок на месте")
        };
        let esm = PluginBuilder::new("back2mw", "")
            .as_master()
            .to_bytes()
            .unwrap();
        let esp = PluginBuilder::new("back2mw", "").to_bytes().unwrap();
        let (mut a, mut b) = (Plugin::new(), Plugin::new());
        a.load_bytes(&esm).unwrap();
        b.load_bytes(&esp).unwrap();
        assert_eq!(header(a), FileType::Esm);
        assert_eq!(header(b), FileType::Esp);
    }

    #[test]
    fn masters_are_recorded() {
        let builder = PluginBuilder::new("back2mw", "").with_master("Morrowind.esm", 79_764_287);
        let plugin = builder.into_plugin();
        let header = plugin
            .objects
            .iter()
            .find_map(|o| match o {
                TES3Object::Header(h) => Some(h),
                _ => None,
            })
            .expect("заголовок на месте");
        assert_eq!(header.masters, [("Morrowind.esm".to_owned(), 79_764_287)]);
    }
}

#[cfg(test)]
mod sound_gen_tests {
    use super::*;

    /// `SNDG` (T11.6) доходит до файла с видом, существом и звуком; вид вне 0…7 не пишется.
    #[test]
    fn sound_gens_read_back() {
        let mut builder = PluginBuilder::new("back2mw", "");
        for (id, kind) in [
            ("ob_Gnarl0005", 5u8),
            ("ob_Gnarl0000", 0),
            ("ob_Gnarl0009", 9),
        ] {
            builder.add_sound_gen(model::SoundGen {
                id: id.to_owned(),
                creature: "ob_Gnarl".to_owned(),
                sound: "ob_Roar_50".to_owned(),
                kind,
            });
        }
        let bytes = builder.to_bytes().unwrap();
        let mut parsed = Plugin::new();
        parsed.load_bytes(&bytes).unwrap();
        let got: Vec<(String, tes3::esp::SoundGenType)> = parsed
            .objects
            .iter()
            .filter_map(|o| match o {
                TES3Object::SoundGen(g) => {
                    assert_eq!(
                        (g.creature.as_str(), g.sound.as_str()),
                        ("ob_Gnarl", "ob_Roar_50")
                    );
                    Some((g.id.clone(), g.sound_gen_type))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            got,
            [
                ("ob_Gnarl0005".to_owned(), tes3::esp::SoundGenType::Roar),
                ("ob_Gnarl0000".to_owned(), tes3::esp::SoundGenType::LeftFoot),
            ]
        );
    }
}
