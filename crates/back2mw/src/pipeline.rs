//! Прогон `all`: выбранные плагины Oblivion → готовый мод в `output/`.
//!
//! Стадии `read`, `transform`, `meshes` и `write` идут одним проходом в памяти внутри
//! `all`; на диск ложатся только распакованное (`work/`) и результат (`output/`). Раздельный
//! запуск стадий с промежуточным результатом на диске требует формата промежуточных данных —
//! это отдельное решение, здесь его нет.
//!
//! Результат (раскладка — `package`):
//!
//! ```text
//! output/<имя плагина источника>             свой плагин Morrowind на каждый (T1.15)
//! output/Meshes/ob/<имя>_<хеш>.nif          короткий путь, transform::paths
//! output/Icons/ob/<имя>_<хеш>.dds           иконки предметов
//! output/BookArt/ob_<имя>_<хеш>.dds         картинки из текста книг
//! output/Textures/ob_<путь текстуры через _>.dds
//! output/back2mw-assets.tsv                 какой ассет какому плагину (для BSA)
//! ```
//!
//! **Плагины не сливаются.** `Mod1.esp` Oblivion даёт `Mod1.esp` Morrowind. Мастера плагина —
//! `Morrowind.esm` плюс те его мастера Oblivion, что конвертируются в этом же прогоне
//! (плагины обходятся в порядке загрузки, мастер пишется раньше зависимого, размер берётся с
//! диска). Переопределение записи мастера остаётся в переопределяющем плагине с тем же ID
//! (`transform::objects`), и Morrowind сливает их по ID, как ванильные.
//!
//! **Политика отказов.** Объект, чей меш не нашёлся в архиве или не конвертируется (ридер
//! знает пока не все типы блоков — T1.3w), в плагин не попадает: запись без меша была бы
//! битой. Такие записи перечисляются в отчёте. Если набор ограничен флагом `--only` и хоть
//! одна запрошенная запись не прошла — прогон завершается ошибкой. Без `--only` счётчик
//! пропущенных — информация, не отказ.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use mesh_convert::geometry::convert_with_particles;
use mesh_convert::read::parse_with_offsets;
use mesh_convert::textures::TextureRegistry;
use tes3_out::{
    Activator, Enchantment, LeveledCreature, LeveledItem, Object, PluginBuilder, Script, Sound,
    Spell,
};
use tes4_read::Plugin;
use transform::exterior::{
    BaseObject, ConvertedExterior, ExteriorSource, Offset, convert_exterior,
};
use transform::npcs::{ConvertedActors, convert_actors, inventory_of};
use transform::sounds::SoundOrigin;
use transform::{
    IdAllocator, ObjectOrigin, RecordKinds, book_art_path, convert_models_mapped, icon_path,
    mesh_path,
};

use crate::bodies;
use crate::config::Config;
use crate::extract::{self, ExtractReport, extracted_path};
use crate::package::{self, PluginAssets};
use crate::plugins::{self, PluginInfo};
use crate::validate;

pub const SOURCE_PLUGIN: &str = "Oblivion.esm";
pub const TARGET_MASTER: &str = "Morrowind.esm";

#[derive(Debug)]
pub enum PipelineError {
    /// Любая ошибка стадии с готовым текстом.
    Failed(String),
    /// `validate` нашёл брак: по строке на проблему.
    Validation(Vec<String>),
    /// `--only` запросил записи, которые не удалось довести до результата.
    Unresolved(Vec<String>),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(m) => f.write_str(m),
            Self::Validation(list) => {
                lang::trw!(
                    f,
                    "validate нашёл брак ({}):\n",
                    "validate found failures ({}):\n",
                    list.len()
                )?;
                write!(f, "  {}", list.join("\n  "))
            }
            Self::Unresolved(list) => {
                lang::trw!(
                    f,
                    "не удалось довести до результата ({}):\n",
                    "could not be converted ({}):\n",
                    list.len()
                )?;
                write!(f, "  {}", list.join("\n  "))
            }
        }
    }
}

impl std::error::Error for PipelineError {}

fn failed<E: fmt::Display>(context: &str) -> impl FnOnce(E) -> PipelineError + '_ {
    move |e| PipelineError::Failed(format!("{context}: {e}"))
}

/// Куда переместиться консолью Morrowind, чтобы оказаться в конвертированном мире: ячейка метки
/// начала Oblivion (`XMarkerHeading`) или, если метки нет, первая ячейка с объектами.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartHint {
    /// Мир-источник (`--worldspace`).
    pub world: String,
    pub cell: StartTarget,
    /// Положение метки в мире Morrowind, целыми; у запасной ячейки — `None`.
    pub position: Option<[i32; 3]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartTarget {
    /// Внешняя ячейка по сетке Morrowind.
    Exterior(i32, i32),
    /// Интерьер по имени.
    Interior(String),
}

impl StartHint {
    /// Строка журнала с командой консоли (`~` в игре) для плагина `plugin`.
    pub fn line(&self, plugin: &str) -> String {
        let source = if self.position.is_some() {
            lang::tr("метка начала Oblivion", "Oblivion start marker")
        } else {
            lang::tr(
                "метки начала нет, первая ячейка с объектами",
                "no start marker, the first cell with objects",
            )
        };
        let at = self
            .position
            .map(|[x, y, z]| format!(" ({x}, {y}, {z})"))
            .unwrap_or_default();
        match &self.cell {
            StartTarget::Exterior(x, y) => lang::tr!(
                "teleport: {plugin} ({}) — {source}, внешняя ячейка ({x}, {y}){at}; в консоли игры (~): coe {x} {y}",
                "teleport: {plugin} ({}) — {source}, exterior cell ({x}, {y}){at}; in the game console (~): coe {x} {y}",
                self.world
            ),
            StartTarget::Interior(name) => lang::tr!(
                "teleport: {plugin} ({}) — {source}, интерьер «{name}»{at}; в консоли игры (~): coc \"{name}\"",
                "teleport: {plugin} ({}) — {source}, interior \"{name}\"{at}; in the game console (~): coc \"{name}\"",
                self.world
            ),
        }
    }
}

/// Подсказка для консоли по конвертированному миру: метка начала, иначе первая ячейка с объектами.
fn start_hint(world: &str, converted: &ConvertedExterior) -> Option<StartHint> {
    use transform::exterior::StartCell;
    let rounded = |p: [f32; 3]| {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "подсказка: целые юниты мира"
        )]
        p.map(|v| v.round() as i32)
    };
    if let Some(start) = &converted.report.start {
        let cell = match &start.cell {
            StartCell::Exterior(x, y) => StartTarget::Exterior(*x, *y),
            StartCell::Interior(i) => {
                StartTarget::Interior(converted.interiors.get(*i)?.name.clone())
            }
        };
        return Some(StartHint {
            world: world.to_owned(),
            cell,
            position: Some(rounded(start.position)),
        });
    }
    let cell = converted
        .cells
        .iter()
        .find(|c| !c.references.is_empty())
        .map(|c| StartTarget::Exterior(c.grid.0, c.grid.1))
        .or_else(|| {
            converted
                .interiors
                .iter()
                .find(|c| !c.references.is_empty())
                .map(|c| StartTarget::Interior(c.name.clone()))
        })?;
    Some(StartHint {
        world: world.to_owned(),
        cell,
        position: None,
    })
}

/// Итог по одному плагину.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginOutcome {
    /// Куда переместиться консолью после установки (`--worldspace`).
    pub start: Option<StartHint>,
    /// Имя файла — одинаковое у источника и результата.
    pub name: String,
    pub statics: usize,
    pub weapons: usize,
    pub armor: usize,
    pub clothing: usize,
    /// Записи `BODY` (части тела брони и одежды).
    pub bodies: usize,
    /// Предметы: `MISC`, `BOOK`, `ALCH`, `INGR`, `APPA`.
    pub items: usize,
    /// Объекты мира: `ACTI`, `DOOR`, `CONT`, `LIGH` (T2.2).
    pub world: usize,
    /// Уровневые списки `LEVI` (T2.5).
    pub leveled: usize,
    /// Зачарования `ENCH` и заклинания `SPEL` (T9.4).
    pub enchantments: usize,
    pub spells: usize,
    /// Звуки `SOUN` (T10.1).
    pub sounds: usize,
    /// Глобальные переменные `GLOB` (T11.1).
    pub globals: usize,
    /// Существа `CREA` (T11.3).
    pub creatures: usize,
    /// Списки существ `LEVC` (T11.4).
    pub creature_lists: usize,
    /// Внешние ячейки и `LAND` (`--worldspace`).
    pub cells: usize,
    pub lands: usize,
    /// Внутренних ячеек.
    pub interiors: usize,
    /// NPC (`NPC_`, фаза 7).
    pub npcs: usize,
    /// Темы `DIAL` и реплики `INFO` (T10.6).
    pub topics: usize,
    pub infos: usize,
    /// Мастера плагина Morrowind, кроме `Morrowind.esm`.
    pub masters: Vec<String>,
}

/// Итог прогона.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Записанные плагины в порядке загрузки.
    pub plugins: Vec<PluginOutcome>,
    /// Объектов (`STAT` и `WEAP`) к конвертации, по всем плагинам.
    pub statics_seen: usize,
    /// Записей `STAT` во всех плагинах.
    pub statics_written: usize,
    /// Записей `WEAP` во всех плагинах.
    pub weapons_written: usize,
    /// Записей `ARMO`, `CLOT` и `BODY` во всех плагинах.
    pub armor_written: usize,
    pub clothing_written: usize,
    pub bodies_written: usize,
    pub items_written: usize,
    pub world_written: usize,
    /// Записей `LEVI` во всех плагинах.
    pub leveled_written: usize,
    /// Записей `GLOB` во всех плагинах (T11.1).
    pub globals_written: usize,
    /// Записей `CREA` во всех плагинах (T11.3).
    pub creatures_written: usize,
    /// Записей `LEVC` во всех плагинах (T11.4).
    pub creature_lists_written: usize,
    /// Записей `ENCH` и `SPEL` во всех плагинах.
    pub enchantments_written: usize,
    pub spells_written: usize,
    /// Записей `SOUN` во всех плагинах и их файлов (T10.1).
    pub sounds_written: usize,
    pub sound_files_copied: usize,
    /// Из них сведено из стерео в моно.
    pub sound_files_mixed: usize,
    /// Файлов голоса реплик (T10.8) — склеек mp3.
    pub voice_files_written: usize,
    /// Файлы звуков, которых нет ни свободными, ни в архивах, или пустой каталог.
    pub sound_files_missing: Vec<String>,
    /// Объекты, не попавшие в плагин, с причиной.
    pub statics_skipped: Vec<String>,
    /// Пустых слотов вещей снято: из их мешей убрана кожа Oblivion, видно тело Morrowind.
    pub worn_slots_unhidden: usize,
    pub meshes_written: usize,
    /// Системы частиц Oblivion в мешах прогона (T2.7): стали эмиттерами Morrowind, из них с
    /// потерями, и выброшенные (эффекты по событию, частицы-меши).
    pub particles_converted: usize,
    pub particles_lossy: usize,
    pub particles_dropped: usize,
    pub textures_copied: usize,
    /// Текстур, сделанных конвертером (кожа лиц, T8.8).
    pub textures_generated: usize,
    /// Текстуры, которых нет в архиве текстур (меш на них всё равно ссылается).
    pub textures_missing: Vec<String>,
    pub icons_copied: usize,
    /// Иконки, которых нет в архивах (оружие в плагине есть, но в инвентаре без картинки).
    pub icons_missing: Vec<String>,
    pub book_art_copied: usize,
    /// Картинки книг, которых нет в архивах (ссылка в тексте есть, картинки в книге не будет).
    pub book_art_missing: Vec<String>,
    /// Камни душ Oblivion, отображённые на ванильные камни Morrowind (записи не пишутся).
    pub soul_gems_mapped: usize,
    /// Цены, поднятые до Morrowind (оружие и броня), если просили.
    pub prices_raised: usize,
    /// Листов карты мира с описанием для мода MWSE-Lua (`--lua-map`).
    pub world_maps_written: usize,
    pub extract_meshes: ExtractReport,
    pub extract_textures: ExtractReport,
    /// `STAT` и `WEAP`, насчитанные `esmtool` по всем плагинам.
    pub plugin_stat_records: usize,
    pub plugin_weapon_records: usize,
    pub meshes_validated: usize,
    /// Плагины источника, которые удалось прочитать.
    pub plugins_read: usize,
    /// Замечания, не останавливающие прогон (плагин не читается, запись не представима и т. п.).
    pub problems: Vec<String>,
}

/// Записи одного плагина и их происхождение.
struct PluginWork<'a> {
    info: &'a PluginInfo,
    objects: Vec<Object>,
    origins: Vec<ObjectOrigin>,
    /// Уровневые списки (T2.5): у них нет меша, поэтому не в `objects`.
    leveled: Vec<LeveledItem>,
    /// Глобальные переменные (T11.1): свои записи; легшие на ванильные здесь не появляются.
    globals: Vec<tes3_out::Global>,
    /// Существа-доноры (T11.3): модель ванильного существа, числа Oblivion. С абсолютным
    /// `FormID` источника — по нему ставятся ссылки `ACRE` (T11.4).
    creatures: Vec<(u32, tes3_out::Creature)>,
    /// Свои модели существ без донора (T11.5e): из чего стадия мешей соберёт три файла.
    creature_models: Vec<transform::creatures::CreatureModel>,
    /// Списки существ (T11.4), тоже с `FormID`: в мире список стоит ссылкой.
    creature_lists: Vec<(u32, LeveledCreature)>,
    /// Зачарования и заклинания (T9.4): тоже без меша.
    enchantments: Vec<Enchantment>,
    spells: Vec<Spell>,
    /// Звуки (T10.1) и откуда брать их файлы (тот же порядок).
    sounds: Vec<Sound>,
    sound_origins: Vec<SoundOrigin>,
    /// Звуки существ со своей моделью (T11.6): `SNDG`; их `SOUN` — в `sounds`.
    sound_gens: Vec<tes3_out::SoundGen>,
    /// Все `SOUN` плагина по абсолютному `FormID` (ID и источник): для звуков регионов (T10.2).
    sound_sources: BTreeMap<u32, (String, tes4_read::Sound)>,
    /// Скрипты петель звука (T10.3): у активаторов со звуком и у маркеров размещённых звуков.
    scripts: Vec<Script>,
    /// Невидимые активаторы размещённых `SOUN` (T10.3): меш ванильный, поэтому не в `objects`
    /// (там у каждой записи меш источника).
    sound_markers: Vec<Activator>,
    /// Worldspace этого плагина, если прогон с `--worldspace`.
    world: Option<WorldWork>,
    /// NPC, стоящие в worldspace, и всё, на что они ссылаются (фаза 7).
    actors: Option<ConvertedActors>,
    /// Темы и приветствия для NPC (T10.6).
    topics: Vec<tes3_out::dialogue::Topic>,
    /// Файлы голоса реплик (T10.8): `SNAM` → mp3 ответов в архивах по порядку.
    voices: BTreeMap<String, Vec<String>>,
}

impl PluginWork<'_> {
    /// Имя плагина Morrowind: как у источника, а при переносе worldspace — `<EDID>.esp` (из
    /// `Oblivion.esm` иначе вышел бы `Oblivion.esm` с одним островом внутри).
    fn output_name(&self) -> String {
        match &self.world {
            Some(w) => format!("{}.esp", w.source.editor_id),
            None => self.info.name.clone(),
        }
    }
}

/// Worldspace к переносу и что про его размещения известно.
struct WorldWork {
    source: ExteriorSource,
    /// Тип записи каждого базового объекта размещений — для отчёта о непоставленном.
    base_kinds: HashMap<u32, String>,
    /// Базовые объекты, отображённые на ванильные записи (камни душ).
    vanilla: HashMap<u32, String>,
    converted: Option<ConvertedExterior>,
    /// Смещение, с которым мир перенесён: по нему лист карты ложится на мир (`--lua-map`).
    offset: Offset,
}

/// Внешние ячейки ванили (`Morrowind.esm`, `Tribunal.esm`, `Bloodmoon.esm`): x −29…23,
/// y −19…28 (2026-09-25, `tes3_out/tests/real_vanilla_grid.rs`).
pub const VANILLA_EXTERIOR: (i32, i32, i32, i32) = (-29, -19, 23, 28);

/// Зазор между ванилью и worldspace, ячеек.
pub const WORLD_GAP: i32 = 8;

/// Смещение по умолчанию: западный край worldspace — через [`WORLD_GAP`] ячеек восточнее ванили,
/// южный — на уровне южного края ванили. Сканера занятых ячеек модов ещё нет (фаза 5), поэтому
/// Tamriel Rebuilt и прочие соседи не учитываются.
pub fn default_offset(source: &ExteriorSource) -> Offset {
    let min_x = source.cells.iter().map(|c| c.grid.0.div_euclid(2)).min();
    let min_y = source.cells.iter().map(|c| c.grid.1.div_euclid(2)).min();
    Offset {
        dx: VANILLA_EXTERIOR.2 + 1 + WORLD_GAP - min_x.unwrap_or(0),
        dy: VANILLA_EXTERIOR.1 - min_y.unwrap_or(0),
    }
}

/// Распаковка из «свободных» файлов `Data` и архивов источника (`extract::extract_all`).
fn extract_sources(
    config: &Config,
    archives: &[PathBuf],
    wanted: &[String],
) -> Result<ExtractReport, extract::ExtractError> {
    extract::extract_all(&config.source_data, archives, wanted, &config.work_dir)
}

/// Результат конвертации одного меша.
pub(crate) struct MeshResult {
    pub(crate) bytes: Vec<u8>,
    pub(crate) textures: Vec<mesh_convert::textures::TextureCopy>,
    /// Текстуры, сделанные конвертером (кожа лица, T8.8): имя в `Textures\` → байты DDS. Их не
    /// распаковывают, а пишут как есть.
    pub(crate) generated: Vec<(String, Vec<u8>)>,
    /// Меш на теле: слоты `INDX`, где из меша убрана кожа Oblivion и должно быть видно тело
    /// Morrowind (`bodies::convert`). Вещь такие слоты не прячет.
    pub(crate) exposed_slots: BTreeSet<u8>,
}

/// Меш к записи: путь в плагине, по чему он опознаётся для проверки столкновений, результат.
struct MeshEntry {
    /// Путь относительно `Meshes\`.
    rel: String,
    /// Модель источника (с меткой половины и пола у мешей на теле).
    source: String,
    result: Result<MeshResult, String>,
}

/// Ключ меша записи: мировая модель — по пути источника, меш на теле — по пути результата (одна
/// модель источника даёт несколько: мужской, женский, половины). Дерево — тоже по пути
/// результата: у одной `.spt` бывают разные семена и листва. Дверь — тоже: её меш собирается
/// отдельно от меша той же модели у статики (T2.6b).
fn mesh_key(origin: &ObjectOrigin) -> String {
    match (&origin.body, &origin.tree, &origin.door) {
        (Some(body), _, _) => format!("@{}", body.mesh),
        (None, Some(tree), _) => format!("@{}", tree.mesh),
        (None, None, Some(door)) => format!("@{door}"),
        (None, None, None) => origin.model.clone(),
    }
}

/// Что стадия мешей узнала о двери (T2.6): её участь и, если петля перенесена, — на сколько
/// сдвинуто начало координат меша (на столько же сдвигаются ссылки).
#[derive(Debug, Clone, Copy, PartialEq)]
struct DoorInfo {
    fate: mesh_convert::door::DoorFate,
    pivot: Option<[f32; 3]>,
    /// Открывается против часовой стрелки: у Morrowind — в другую сторону.
    counter_clockwise: bool,
}

/// Убирает из контейнеров и уровневых списков предметы, чьи записи не попали ни в один плагин
/// (меш не сконвертировался): ссылка на несуществующий ID в `NPCO` или `INAM` — битая запись.
/// Опустевший список тоже не пишется, и ссылки на него уходят — в том числе из других плагинов
/// прогона, поэтому круги до неподвижной точки. Ванильные ID (камни душ, золото из
/// `Morrowind.esm`) — без нашего префикса, их не трогаем.
fn prune_contents(works: &mut [PluginWork<'_>], outcome: &mut Outcome) {
    let ours = |id: &str| {
        id.to_ascii_lowercase()
            .starts_with(transform::ids::DEFAULT_PREFIX)
    };
    let mut totals = vec![transform::leveled::Pruned::default(); works.len()];
    loop {
        let written: BTreeSet<String> = works
            .iter()
            .flat_map(|w| {
                w.objects
                    .iter()
                    .map(Object::id)
                    .chain(w.leveled.iter().map(|l| l.id.as_str()))
            })
            .map(str::to_ascii_lowercase)
            .collect();
        let mut dropped_lists = 0usize;
        for (work, total) in works.iter_mut().zip(&mut totals) {
            let pruned =
                transform::leveled::prune_references(&mut work.objects, &mut work.leveled, |id| {
                    ours(id) && !written.contains(&id.to_ascii_lowercase())
                });
            total.container_items += pruned.container_items;
            total.list_entries += pruned.list_entries;
            total.lists += pruned.lists;
            dropped_lists += pruned.lists;
        }
        if dropped_lists == 0 {
            break;
        }
    }
    // Инвентарь и заклинания NPC: те же правила, что у контейнеров. Круги не нужны — NPC ни на
    // что, что можно выбросить, не ссылаются.
    let written: BTreeSet<String> = works
        .iter()
        .flat_map(|w| {
            w.objects
                .iter()
                .map(Object::id)
                .chain(w.leveled.iter().map(|l| l.id.as_str()))
                .chain(w.spells.iter().map(|s| s.id.as_str()))
        })
        .map(str::to_ascii_lowercase)
        .collect();
    let gone = |id: &str| ours(id) && !written.contains(&id.to_ascii_lowercase());
    for work in works.iter_mut() {
        let Some(actors) = &mut work.actors else {
            continue;
        };
        let (mut items, mut spells) = (0usize, 0usize);
        for (_, npc) in &mut actors.npcs {
            let before = (npc.inventory.len(), npc.spells.len());
            npc.inventory.retain(|(_, id)| !gone(id));
            npc.spells.retain(|id| !gone(id));
            items += before.0 - npc.inventory.len();
            spells += before.1 - npc.spells.len();
        }
        for race in &mut actors.races {
            let before = race.spells.len();
            race.spells.retain(|id| !gone(id));
            spells += before - race.spells.len();
        }
        if items + spells > 0 {
            outcome.problems.push(lang::tr!(
                "{}: у NPC убрано предметов {items} и заклинаний {spells} — их записи не попали в плагины", "{}: {items} items and {spells} spells removed from NPCs — their records did not make it into the plugins",
                work.info.name
            ));
        }
    }
    // Существа (T11.3) — по тем же правилам, что NPC.
    for work in works.iter_mut() {
        let (mut items, mut spells) = (0usize, 0usize);
        for (_, c) in &mut work.creatures {
            let before = (c.inventory.len(), c.spells.len());
            c.inventory.retain(|(_, id)| !gone(id));
            c.spells.retain(|id| !gone(id));
            items += before.0 - c.inventory.len();
            spells += before.1 - c.spells.len();
        }
        if items + spells > 0 {
            outcome.problems.push(lang::tr!(
                "{}: у существ убрано предметов {items} и заклинаний {spells} — их записи не попали в плагины", "{}: {items} items and {spells} spells removed from creatures — their records did not make it into the plugins",
                work.info.name
            ));
        }
    }
    // Списки существ (T11.4): строка на существо, NPC или список, которых нет ни в одном плагине
    // прогона (чужой плагин не выбран, запись не получилась), уходит; опустевший список тоже —
    // и ссылки на него из других плагинов, поэтому круги.
    let mut list_totals = vec![transform::creature_lists::PrunedLists::default(); works.len()];
    loop {
        let written: BTreeSet<String> = works
            .iter()
            .flat_map(|w| {
                w.creatures
                    .iter()
                    .map(|(_, c)| c.id.as_str())
                    .chain(w.creature_lists.iter().map(|(_, l)| l.id.as_str()))
                    .chain(
                        w.actors
                            .iter()
                            .flat_map(|a| a.npcs.iter().map(|(_, n)| n.id.as_str())),
                    )
            })
            .map(str::to_ascii_lowercase)
            .collect();
        let mut dropped_lists = 0usize;
        for (work, total) in works.iter_mut().zip(&mut list_totals) {
            let pruned =
                transform::creature_lists::prune_creature_lists(&mut work.creature_lists, |id| {
                    ours(id) && !written.contains(&id.to_ascii_lowercase())
                });
            total.entries += pruned.entries;
            total.lists += pruned.lists;
            dropped_lists += pruned.lists;
        }
        if dropped_lists == 0 {
            break;
        }
    }
    for (work, total) in works.iter().zip(&list_totals) {
        if total.entries + total.lists > 0 {
            outcome.problems.push(lang::tr!(
                "{}: из списков существ убрано строк: {}, опустевших списков не записано: {} — их записи не попали в плагины", "{}: rows removed from creature lists: {}, emptied lists not written: {} — their records did not make it into the plugins",
                work.info.name, total.entries, total.lists
            ));
        }
    }
    for (work, total) in works.iter().zip(&totals) {
        if total.container_items > 0 {
            outcome.problems.push(lang::tr!(
                "{}: из контейнеров убрано предметов: {} — их записи не попали в плагины", "{}: items removed from containers: {} — their records did not make it into the plugins",
                work.info.name, total.container_items
            ));
        }
        if total.list_entries + total.lists > 0 {
            outcome.problems.push(lang::tr!(
                "{}: из уровневых списков убрано строк: {}, опустевших списков не записано: {} — их записи не попали в плагины", "{}: rows removed from leveled lists: {}, emptied lists not written: {} — their records did not make it into the plugins",
                work.info.name, total.list_entries, total.lists
            ));
        }
    }
}

/// ID частей тела, на которые ссылается вещь.
/// Снимает с вещей пустые «прячущие» слоты там, где из их мешей убрана кожа Oblivion: там должно
/// быть видно тело Morrowind (решение владельца 2026-09-30). Слот части тела вещи не трогается.
/// Возвращает число снятых слотов.
fn unhide_exposed(
    objects: &mut [Object],
    origins: &[ObjectOrigin],
    results: &BTreeMap<String, MeshEntry>,
) -> usize {
    let exposed: HashMap<String, BTreeSet<u8>> = objects
        .iter()
        .zip(origins)
        .filter_map(|(o, origin)| {
            let Object::BodyPart(b) = o else { return None };
            origin.body.as_ref()?;
            let slots = results
                .get(&mesh_key(origin))?
                .result
                .as_ref()
                .ok()?
                .exposed_slots
                .clone();
            Some((b.id.clone(), slots))
        })
        .collect();
    objects
        .iter_mut()
        .map(|object| match object {
            Object::Armor(a) => unhide_parts(&mut a.parts, &exposed),
            Object::Clothing(c) => unhide_parts(&mut c.parts, &exposed),
            _ => 0,
        })
        .sum()
}

/// Снимает пустые слоты вещи, открытые хоть одной её частью тела (`exposed`: ID `BODY` → слоты).
/// Части с телом не трогаются, даже если их слот открыт другой частью. Возвращает число снятых.
fn unhide_parts(
    parts: &mut Vec<tes3_out::WornPart>,
    exposed: &HashMap<String, BTreeSet<u8>>,
) -> usize {
    let open: BTreeSet<u8> = parts
        .iter()
        .flat_map(|p| [&p.male, &p.female])
        .filter_map(|id| exposed.get(id))
        .flatten()
        .copied()
        .collect();
    let before = parts.len();
    parts.retain(|p| !(p.male.is_empty() && p.female.is_empty() && open.contains(&p.slot)));
    before - parts.len()
}

fn referenced_bodies(object: &Object) -> Vec<&str> {
    let parts = match object {
        Object::Armor(a) => &a.parts,
        Object::Clothing(c) => &c.parts,
        _ => return Vec::new(),
    };
    parts
        .iter()
        .flat_map(|p| [p.male.as_str(), p.female.as_str()])
        .filter(|id| !id.is_empty())
        .collect()
}

/// `dir` + путь с `\`-разделителями (`ob\a\b.nif`).
fn join_backslash(dir: &Path, path: &str) -> PathBuf {
    let mut out = dir.to_owned();
    for segment in path.split('\\').filter(|s| !s.is_empty()) {
        out.push(segment);
    }
    out
}

/// Путь ассета относительно `Data Files` с `/`: `Meshes` + `ob\a.nif` → `Meshes/ob/a.nif`.
fn asset_relative(dir: &str, path: &str) -> String {
    format!("{dir}/{}", path.replace('\\', "/"))
}

/// Что конвертировать и как.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Плагины Oblivion (имена файлов в `source_data`); пусто — только `Oblivion.esm`.
    pub plugins: Vec<String>,
    /// `EDID` (без учёта регистра); пусто — все `STAT`.
    pub only: Vec<String>,
    /// Прогнать `esmtool` и `niftest`. Нужны пути к ним в конфигурации.
    pub validate: bool,
    /// Брать модели не только у `STAT`, но и у предметов, оружия, брони, мебели и т. д.
    /// (`transform::MODEL_RECORDS`): моды Oblivion почти не содержат `STAT`.
    pub all_models: bool,
    /// `EDID` worldspace: перенести его ячейки, ландшафт, регионы и размещения. Объекты тогда
    /// берутся только те, что в нём стоят, а плагин называется `<EDID>.esp`.
    pub worldspace: Option<String>,
    /// Смещение worldspace в ячейках Morrowind; `None` — по умолчанию
    /// ([`default_offset`]).
    pub offset: Option<(i32, i32)>,
    /// Подтянуть цены оружия и брони к Morrowind (`transform::prices`): только вверх.
    pub morrowind_prices: bool,
    /// Положить лист карты мира и мод MWSE-Lua, который его показывает (`crate::world_map`).
    /// Имеет смысл только с `worldspace`.
    pub lua_map: bool,
}

impl Request {
    /// Срез M1 / CLI: только `Oblivion.esm`, `validate` включён.
    pub fn oblivion(only: &[String]) -> Self {
        Self {
            plugins: Vec::new(),
            only: only.to_vec(),
            validate: true,
            all_models: false,
            worldspace: None,
            offset: None,
            morrowind_prices: false,
            lua_map: false,
        }
    }
}

/// Запускает прогон по `Oblivion.esm`. `only` — `EDID` (без учёта регистра); пусто — все `STAT`.
/// `log` получает строки прогресса.
pub fn run_all(
    config: &Config,
    only: &[String],
    log: &mut dyn FnMut(&str),
) -> Result<Outcome, PipelineError> {
    run(config, &Request::oblivion(only), log)
}

/// Файл начинается сигнатурой BSA Oblivion (`BSA\0`). Архивы Morrowind (magic `00 01 00 00`)
/// в `Data` Oblivion бывают — например, в наборе тестовых данных, где лежат обе игры, — и
/// читать их как источник нельзя.
fn is_oblivion_bsa(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok_and(|()| &magic == b"BSA\0")
}

/// Архивы источника в порядке приоритета: моды, потом дополнения, потом `Oblivion*`. Звуки
/// нужны с T10.1; архивы голосов — отдельно ([`voice_archives`]): в них 48 тысяч mp3, и обыскивать
/// их ради мешей и текстур незачем. Архивы не Oblivion (см. [`is_oblivion_bsa`]) пропускаются.
pub fn source_archives(source_data: &Path) -> Vec<PathBuf> {
    archives_of(source_data, false)
}

/// Архивы голосов источника (`*Voices*.bsa`, T10.8) в том же порядке приоритета.
pub fn voice_archives(source_data: &Path) -> Vec<PathBuf> {
    archives_of(source_data, true)
}

fn archives_of(source_data: &Path, voices: bool) -> Vec<PathBuf> {
    let mut list: Vec<(u8, String, PathBuf)> = Vec::new();
    if let Ok(entries) = fs::read_dir(source_data) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if !name.ends_with(".bsa") || name.contains("voices") != voices {
                continue;
            }
            if !is_oblivion_bsa(&e.path()) {
                continue;
            }
            let rank = if name.starts_with("oblivion") {
                2
            } else if name.starts_with("dlc") || name.starts_with("knights") {
                1
            } else {
                0
            };
            list.push((rank, name, e.path()));
        }
    }
    list.sort();
    list.into_iter().map(|(_, _, p)| p).collect()
}

/// Пути к `esmtool` и `niftest`, если нужна проверка. Узнаём об их отсутствии до долгой работы.
fn tools(config: &Config, request: &Request) -> Result<Option<(PathBuf, PathBuf)>, PipelineError> {
    if !request.validate {
        return Ok(None);
    }
    let need = |value: &Option<PathBuf>, tool, flag, env| {
        value.clone().ok_or_else(|| {
            PipelineError::Failed(
                validate::ValidateError::ToolNotConfigured {
                    tool,
                    key: tool,
                    flag,
                    env,
                }
                .to_string(),
            )
        })
    };
    Ok(Some((
        need(&config.esmtool, "esmtool", "--esmtool", "BACK2MW_ESMTOOL")?,
        need(&config.niftest, "niftest", "--niftest", "BACK2MW_NIFTEST")?,
    )))
}

/// Читает и преобразует выбранные плагины, по одному результату на плагин, в порядке загрузки.
fn read_plugins<'a>(
    config: &Config,
    request: &Request,
    installed: &'a [PluginInfo],
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> Result<(Vec<PluginWork<'a>>, IdAllocator), PipelineError> {
    let selected: Vec<&PluginInfo> = if request.plugins.is_empty() {
        installed
            .iter()
            .filter(|p| p.is_oblivion_master())
            .collect()
    } else {
        installed
            .iter()
            .filter(|p| {
                request
                    .plugins
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(&p.name))
            })
            .collect()
    };
    if selected.is_empty() {
        return Err(PipelineError::Failed(lang::tr!(
            "в {} нет ни одного из выбранных плагинов ({SOURCE_PLUGIN} обязателен для сборки)",
            "none of the selected plugins is in {} ({SOURCE_PLUGIN} is required for a build)",
            config.source_data.display()
        )));
    }

    let wanted_ids: BTreeSet<String> = request
        .only
        .iter()
        .map(|e| e.to_ascii_lowercase())
        .collect();
    // Один распределитель на весь прогон: переопределение в моде получает ID мастера.
    let mut ids = IdAllocator::default();
    let mut works = Vec::new();
    // `EDID` записей, отображённых на ванильные (камни душ): их нет в `origins`, но для `--only`
    // они найдены.
    let mut found: BTreeSet<String> = BTreeSet::new();
    // Части тела `Morrowind.esm` (головы, тела рас) — читаются один раз, если есть NPC.
    let mut vanilla_bodies: Option<Vec<tes3_out::vanilla::VanillaBodyPart>> = None;
    // Темы и глобальные переменные `Morrowind.esm` — тоже один раз, если есть диалоги.
    let mut vanilla_dialogue: Option<crate::dialogue::VanillaDialogue> = None;
    // Глобальные переменные `Morrowind.esm` (T11.1): ключ в нижнем регистре → ID как в мастере.
    let mut vanilla_globals: Option<HashMap<String, String>> = None;
    // Существа `Morrowind.esm` — доноры моделей (T11.3), тоже один раз.
    let mut vanilla_creatures: Option<Vec<tes3_out::vanilla::VanillaCreature>> = None;
    for info in selected {
        let path = config.source_data.join(&info.name);
        log(&format!("read: {}", path.display()));
        if let Some(e) = &info.error {
            outcome.problems.push(lang::tr!(
                "{}: заголовок не читается ({e}), плагин пропущен",
                "{}: the header cannot be read ({e}), plugin skipped",
                info.name
            ));
            continue;
        }
        let bytes = fs::read(&path).map_err(failed(&path.display().to_string()))?;
        let source = match Plugin::parse(&bytes) {
            Ok(p) => p,
            Err(e) => {
                outcome.problems.push(lang::tr!(
                    "{}: не разобрать ({e}), плагин пропущен",
                    "{}: cannot be parsed ({e}), plugin skipped",
                    info.name
                ));
                continue;
            }
        };
        let map = |file_form_id| plugins::absolute_form_id(installed, info, file_form_id);
        // С `--worldspace` из плагина, где он есть, переносятся только объекты, которые в нём
        // стоят. Плагин без него конвертируется как обычно: в окне рядом с `Oblivion.esm`
        // отмечены моды, и молча терять их из-за выбора мира нельзя.
        let mut world = None;
        let mut placed_edids: Option<BTreeSet<String>> = None;
        let exterior = match &request.worldspace {
            Some(name) => ExteriorSource::read(&source, name, map)
                .map_err(failed(&format!("read worldspace {name} ({})", info.name)))?,
            None => None,
        };
        let mut world_npcs: BTreeSet<u32> = BTreeSet::new();
        // Существа и списки существ мира (T11.4); без `--worldspace` — все, как прочие записи.
        let mut world_creatures: Option<BTreeSet<u32>> = None;
        let mut world_creature_lists: Option<BTreeSet<u32>> = None;
        if let Some(exterior) = exterior {
            let mut bases = exterior.base_objects();
            // Список существ стоит в мире ссылкой, а те, кого он порождает, — нет: существа и
            // NPC из его строк (и из вложенных списков) конвертируются тоже.
            let spawned = transform::creature_lists::reachable(&source, map, &bases)
                .map_err(failed(&format!("LVLC ({})", info.name)))?;
            bases.extend(spawned.actors.iter().copied());
            // NPC стоят в мире (`ACHR`), а их инвентарь — нет: предметы из него конвертируются
            // тоже, как ключи замков (T6.4). Так же — инвентарь существ.
            world_npcs = source
                .records_of(*b"NPC_")
                .filter_map(|r| map(r.header.form_id))
                .filter(|id| bases.contains(id))
                .collect();
            let creatures: BTreeSet<u32> = source
                .records_of(*b"CREA")
                .filter_map(|r| map(r.header.form_id))
                .filter(|id| bases.contains(id))
                .collect();
            bases.extend(
                inventory_of(&source, map, &world_npcs)
                    .map_err(failed(&format!("NPC ({})", info.name)))?,
            );
            bases.extend(
                transform::creatures::inventory_of(&source, map, &creatures)
                    .map_err(failed(&format!("CREA ({})", info.name)))?,
            );
            world_creatures = Some(creatures);
            world_creature_lists = Some(spawned.lists);
            let mut kinds = HashMap::new();
            let mut edids = BTreeSet::new();
            for r in source.records() {
                // Уровневые списки не стоят в мире, но их берут стоящие контейнеры: берём все,
                // а строки с непоставленными предметами отпадут при переводе в ID. Заклинания в
                // мире не стоят вовсе, а понадобятся NPC (фаза 7) — тоже берём все. Зачарования
                // фильтр не проходят: их пишут вещи, которые на них ссылаются.
                if matches!(&r.header.tag, b"LVLI" | b"SPEL") {
                    if let Some(edid) = tes4_read::Fields::parse(&r.data, "LVLI")
                        .ok()
                        .and_then(|f| f.opt_zstring(*b"EDID"))
                    {
                        edids.insert(edid.to_ascii_lowercase());
                    }
                    continue;
                }
                let Some(id) = map(r.header.form_id).filter(|id| bases.contains(id)) else {
                    continue;
                };
                kinds.insert(id, String::from_utf8_lossy(&r.header.tag).into_owned());
                if let Some(edid) = tes4_read::Fields::parse(&r.data, "base")
                    .ok()
                    .and_then(|f| f.opt_zstring(*b"EDID"))
                {
                    edids.insert(edid.to_ascii_lowercase());
                }
            }
            log(&lang::tr!(
                "read: worldspace {} — ячеек {}, размещений {}, базовых объектов {}",
                "read: worldspace {} — cells {}, references {}, base objects {}",
                exterior.editor_id,
                exterior.cells.len(),
                exterior.placements.len(),
                bases.len()
            ));
            placed_edids = Some(edids);
            world = Some(WorldWork {
                source: exterior,
                base_kinds: kinds,
                vanilla: HashMap::new(),
                converted: None,
                offset: Offset::default(),
            });
        }
        let converted = convert_models_mapped(
            &source,
            &mut ids,
            |editor_id| {
                let edid = editor_id.to_ascii_lowercase();
                (wanted_ids.is_empty() || wanted_ids.contains(&edid))
                    && placed_edids.as_ref().is_none_or(|set| set.contains(&edid))
            },
            |file_form_id| plugins::absolute_form_id(installed, info, file_form_id),
            false,
            if request.all_models {
                RecordKinds::AllWithModels
            } else {
                RecordKinds::Statics
            },
        )
        .map_err(failed(&format!("transform ({})", info.name)))?;
        outcome.plugins_read += 1;
        let r = &converted.report;
        outcome
            .problems
            .extend(r.problems.iter().map(|p| format!("{}: {p}", info.name)));
        if r.dropped_scripts + r.dropped_enchantments > 0 {
            outcome.problems.push(lang::tr!(
                "{}: записи перенесены без скриптов ({}) — их конвертации ещё нет; вещей без зачарования, которое не перенеслось: {}", "{}: records converted without their scripts ({}) — script conversion does not exist yet; items left without an enchantment that was not converted: {}",
                info.name, r.dropped_scripts, r.dropped_enchantments
            ));
        }
        if r.contents_unresolved > 0 {
            outcome.problems.push(lang::tr!(
                "{}: из контейнеров убрано предметов: {} — нет такой записи в выбранных плагинах или опустел уровневый список", "{}: items removed from containers: {} — no such record in the selected plugins, or the leveled list ended up empty",
                info.name, r.contents_unresolved
            ));
        }
        if r.leveled_entries_unresolved + r.leveled_empty > 0 {
            outcome.problems.push(lang::tr!(
                "{}: из уровневых списков убрано строк: {} — нет такой записи в выбранных плагинах; пустых списков не записано: {}", "{}: rows removed from leveled lists: {} — no such record in the selected plugins; empty lists not written: {}",
                info.name, r.leveled_entries_unresolved, r.leveled_empty
            ));
        }
        if r.unresolved_master > 0 {
            outcome.problems.push(lang::tr!(
                "{}: {} записей пропущено — не установлен нужный мастер",
                "{}: {} records skipped — the required master is not installed",
                info.name,
                r.unresolved_master
            ));
        }
        if r.sounds_dropped > 0 {
            outcome.problems.push(lang::tr!(
                "{}: звуков контейнеров не перенесено: {} — у Morrowind нет такого поля",
                "{}: container sounds not converted: {} — Morrowind has no such field",
                info.name,
                r.sounds_dropped
            ));
        }
        if r.activator_loops > 0 {
            log(&lang::tr!(
                "transform: {} — активаторов с петлёй звука {}, скриптов петель {}",
                "transform: {} — activators with a sound loop {}, loop scripts {}",
                info.name,
                r.activator_loops,
                converted.scripts.len()
            ));
        }
        outcome.soul_gems_mapped += r.soul_gems;
        outcome.statics_seen += converted.objects.len();
        found.extend(
            converted
                .vanilla
                .iter()
                .map(|v| v.editor_id.to_ascii_lowercase()),
        );
        if let Some(w) = &mut world {
            w.vanilla = converted
                .vanilla
                .iter()
                .map(|v| (v.form_id, v.id.to_owned()))
                .collect();
        }
        let globals = if request.all_models {
            read_globals(
                config,
                &source,
                map,
                &wanted_ids,
                &mut ids,
                &mut vanilla_globals,
                outcome,
                &info.name,
                log,
            )?
        } else {
            Vec::new()
        };
        let mut actors = if world_npcs.is_empty() {
            None
        } else {
            read_actors(
                config,
                &source,
                map,
                &world_npcs,
                &mut ids,
                world.as_ref().map(|w| &w.vanilla),
                &mut vanilla_bodies,
                outcome,
                &info.name,
            )?
        };
        if let Some(a) = &actors {
            let r = &a.report;
            log(&lang::tr!(
                "npc: {} — NPC {}, рас своих {} (частей тела {}), классов {}, фракций {}",
                "npc: {} — NPCs {}, own races {} (body parts {}), classes {}, factions {}",
                info.name,
                a.npcs.len(),
                a.races.len(),
                a.bodies.len(),
                a.classes.len(),
                a.factions.len()
            ));
            for (count, what) in [
                (
                    r.skipped,
                    lang::tr(
                        "NPC не перенесено — нет расы или класса",
                        "NPCs not converted — no race or class",
                    ),
                ),
                (
                    r.items_lost,
                    lang::tr(
                        "предметов из инвентаря NPC не нашлось в плагине",
                        "NPC inventory items not found in the plugin",
                    ),
                ),
                (
                    r.spells_lost,
                    lang::tr(
                        "заклинаний NPC не нашлось в плагине",
                        "NPC spells not found in the plugin",
                    ),
                ),
                (
                    r.without_head,
                    lang::tr(
                        "NPC без ванильной головы расы и пола",
                        "NPCs without a vanilla head for their race and sex",
                    ),
                ),
            ] {
                if count > 0 {
                    outcome
                        .problems
                        .push(format!("{}: {count} {what}", info.name));
                }
            }
            if !a.races_unlisted.is_empty() {
                outcome.problems.push(lang::tr!(
                    "{}: расы не из data/races.tsv стали своими с телом Imperial: {}", "{}: races missing from data/races.tsv became own races with the Imperial body: {}",
                    info.name,
                    a.races_unlisted.join(", ")
                ));
            }
        }
        let (creatures, creature_models, creature_sound_gens) = if request.all_models {
            read_creatures(
                config,
                &source,
                map,
                &wanted_ids,
                world_creatures.as_ref(),
                &mut ids,
                world.as_ref().map(|w| &w.vanilla),
                &mut vanilla_creatures,
                outcome,
                &info.name,
                log,
            )?
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };
        // `--only`: существо найдено, если его запись перенесена (в `origins` существ нет).
        if !wanted_ids.is_empty() {
            let converted: BTreeSet<u32> = creatures.iter().map(|(f, _)| *f).collect();
            found.extend(
                source
                    .records_of(*b"CREA")
                    .filter(|r| map(r.header.form_id).is_some_and(|id| converted.contains(&id)))
                    .filter_map(|r| {
                        tes4_read::Fields::parse(&r.data, "EDID")
                            .ok()
                            .and_then(|f| f.opt_zstring(*b"EDID"))
                            .map(|e| e.to_ascii_lowercase())
                    }),
            );
        }
        let creature_lists = if request.all_models {
            // Цели строк: существа и NPC этого плагина — те, что получились; записи других
            // плагинов прогона — по выданному ID (попали ли они в плагин, сверит
            // `prune_contents`).
            let own: HashMap<u32, String> = creatures
                .iter()
                .map(|(f, c)| (*f, c.id.clone()))
                .chain(
                    actors
                        .iter()
                        .flat_map(|a| a.npcs.iter().map(|(f, n)| (*f, n.id.clone()))),
                )
                .collect();
            read_creature_lists(
                &source,
                map,
                &wanted_ids,
                world_creature_lists.as_ref(),
                &mut ids,
                &own,
                info.load_index,
                outcome,
                &info.name,
                log,
            )?
        } else {
            Vec::new()
        };
        let mut scripts = converted.scripts;
        let mut topics = Vec::new();
        let mut voices = BTreeMap::new();
        let mut npc_scripts: BTreeMap<String, String> = BTreeMap::new();
        if let (Some(a), Some(w)) = (&actors, &world)
            && !a.npcs.is_empty()
        {
            // Метка переносимых NPC (решение 2 фазы 10): её `SCRI` ставит `transform::npcs`.
            scripts.push(Script::Locals {
                id: transform::dialogue::NPC_SCRIPT.to_owned(),
                shorts: vec![transform::dialogue::NPC_VARIABLE.to_owned()],
            });
            let owner = |form_id: u32| {
                let index = usize::try_from(form_id >> 24).ok()?;
                installed
                    .iter()
                    .find(|p| p.load_index == index)
                    .map(|p| p.name.clone())
            };
            let d = read_dialogue(
                config,
                &source,
                &map,
                &owner,
                w,
                a,
                &ids,
                &mut vanilla_dialogue,
                outcome,
                &info.name,
                log,
            );
            // Метки фракций: NPC фракции с меткой — свой скрипт с `ob_npc` и `obf_…`.
            scripts.extend(
                d.scripts
                    .into_iter()
                    .map(|(id, shorts)| Script::Locals { id, shorts }),
            );
            npc_scripts = d.npc_scripts;
            (topics, voices) = (d.topics, d.voices);
        }
        if let Some(a) = &mut actors {
            for (_, npc) in &mut a.npcs {
                if let Some(script) = npc_scripts.get(&npc.id) {
                    npc.script.clone_from(script);
                }
            }
        }
        // Звуки существ (T11.6): `SOUN` под них — те же записи, что у дверей и света.
        let mut sounds = converted.sounds;
        let mut sound_origins = converted.sound_origins;
        let creature_sounds = transform::sounds::creature_sounds(
            &creature_sound_gens,
            &|f| {
                creatures
                    .iter()
                    .find(|(form_id, _)| *form_id == f)
                    .map(|(_, c)| c.id.clone())
            },
            &converted.sound_sources,
            &|id| sounds.iter().any(|s| s.id.eq_ignore_ascii_case(id)),
        );
        outcome.problems.extend(
            creature_sounds
                .problems
                .iter()
                .map(|p| format!("{}: {p}", info.name)),
        );
        if creature_sounds.lost > 0 {
            outcome.problems.push(lang::tr!(
                "{}: у существ без звука осталось видов: {} — SOUN нет в этом плагине или путь не представим",
                "{}: creature sound kinds left silent: {} — the SOUN is not in this plugin or its path cannot be represented",
                info.name,
                creature_sounds.lost
            ));
        }
        for c in creature_sounds.sounds {
            sounds.push(c.sound);
            sound_origins.push(c.origin);
        }
        works.push(PluginWork {
            info,
            objects: converted.objects,
            origins: converted.origins,
            leveled: converted.leveled,
            globals,
            creatures,
            creature_models,
            creature_lists,
            enchantments: converted.enchantments,
            spells: converted.spells,
            sounds,
            sound_origins,
            sound_gens: creature_sounds.gens,
            sound_sources: converted.sound_sources,
            scripts,
            sound_markers: Vec::new(),
            world,
            actors,
            topics,
            voices,
        });
    }

    found.extend(
        works
            .iter()
            .flat_map(|w| &w.origins)
            .map(|o| o.editor_id.to_ascii_lowercase()),
    );
    let unknown: Vec<String> = request
        .only
        .iter()
        .filter(|e| !found.contains(&e.to_ascii_lowercase()))
        .map(|e| {
            lang::tr!(
                "{e}: нет такой записи с MODL",
                "{e}: no such record with a MODL"
            )
        })
        .collect();
    if !unknown.is_empty() {
        return Err(PipelineError::Unresolved(unknown));
    }
    Ok((works, ids))
}

/// Глобальные переменные плагина (T11.1). Ванильные переменные `Morrowind.esm` читаются один раз
/// на прогон; не прочитались — движковые переменные не переносятся, остальные пишутся своими
/// записями, с замечанием.
#[expect(clippy::too_many_arguments, reason = "одна стадия, всё её окружение")]
fn read_globals(
    config: &Config,
    source: &Plugin,
    map: impl Fn(u32) -> Option<u32>,
    wanted_ids: &BTreeSet<String>,
    ids: &mut IdAllocator,
    vanilla: &mut Option<HashMap<String, String>>,
    outcome: &mut Outcome,
    plugin: &str,
    log: &mut dyn FnMut(&str),
) -> Result<Vec<tes3_out::Global>, PipelineError> {
    let vanilla = vanilla.get_or_insert_with(|| {
        match tes3_out::vanilla::read_file(&config.target_data, TARGET_MASTER)
            .and_then(|bytes| tes3_out::vanilla::globals(&bytes))
        {
            Ok(list) => list
                .into_iter()
                .map(|(id, _)| (id.to_ascii_lowercase(), id))
                .collect(),
            Err(e) => {
                outcome.problems.push(lang::tr!(
                    "{plugin}: глобальные переменные {TARGET_MASTER} не прочитаны ({e}) — движковые переменные не перенесены", "{plugin}: the globals of {TARGET_MASTER} could not be read ({e}) — engine globals not converted"
                ));
                HashMap::new()
            }
        }
    });
    let converted = transform::globals::convert(
        source,
        ids,
        |edid| wanted_ids.is_empty() || wanted_ids.contains(&edid.to_ascii_lowercase()),
        map,
        vanilla,
        false,
    )
    .map_err(failed(&format!("GLOB ({plugin})")))?;
    let r = &converted.report;
    if r.seen > 0 {
        log(&lang::tr!(
            "transform: {plugin} — GLOB {}: своих {}, на ванильные {}, без пары {}",
            "transform: {plugin} — GLOB {}: own {}, mapped to vanilla {}, unmatched {}",
            r.seen,
            converted.globals.len(),
            r.to_vanilla,
            r.unsupported.len()
        ));
    }
    if !r.unsupported.is_empty() {
        outcome.problems.push(lang::tr!(
            "{plugin}: глобальные переменные без пары в {TARGET_MASTER} не перенесены: {}",
            "{plugin}: globals with no counterpart in {TARGET_MASTER} not converted: {}",
            r.unsupported.join(", ")
        ));
    }
    outcome
        .problems
        .extend(r.problems.iter().map(|p| format!("{plugin}: {p}")));
    Ok(converted.globals)
}

/// Существа плагина с `FormID` источника и свои модели, которые им нужны.
type CreaturesRead = (
    Vec<(u32, tes3_out::Creature)>,
    Vec<transform::creatures::CreatureModel>,
    Vec<transform::creatures::CreatureSoundGen>,
);

/// Существа плагина: на моделях доноров из `data/creatures.tsv` (T11.3) и, если донора нет, со
/// своей моделью из скелета и частей тела (T11.5e) — её описание возвращается вторым значением,
/// собирает модель стадия мешей. `--only` — только названные; `in_world` — только стоящие в мире
/// и порождаемые его списками (T11.4). Нет `Morrowind.esm` — существа не переносятся, с
/// замечанием.
#[expect(clippy::too_many_arguments, reason = "одна стадия, всё её окружение")]
fn read_creatures(
    config: &Config,
    source: &Plugin,
    map: impl Fn(u32) -> Option<u32> + Copy,
    wanted_ids: &BTreeSet<String>,
    in_world: Option<&BTreeSet<u32>>,
    ids: &mut IdAllocator,
    vanilla_items: Option<&HashMap<u32, String>>,
    vanilla: &mut Option<Vec<tes3_out::vanilla::VanillaCreature>>,
    outcome: &mut Outcome,
    plugin: &str,
    log: &mut dyn FnMut(&str),
) -> Result<CreaturesRead, PipelineError> {
    if vanilla.is_none() {
        match tes3_out::vanilla::read_file(&config.target_data, TARGET_MASTER)
            .and_then(|bytes| tes3_out::vanilla::creatures(&bytes))
        {
            Ok(list) => *vanilla = Some(list),
            Err(e) => {
                outcome.problems.push(lang::tr!(
                    "{plugin}: существа не перенесены — не прочитать {TARGET_MASTER} ({e})",
                    "{plugin}: creatures not converted — cannot read {TARGET_MASTER} ({e})"
                ));
                return Ok((Vec::new(), Vec::new(), Vec::new()));
            }
        }
    }
    let vanilla = vanilla.as_deref().expect("заполнено выше или вышли");
    let wanted = wanted_records(source, *b"CREA", map, wanted_ids, in_world);
    let known = ids.clone();
    let item = |f: u32| {
        vanilla_items
            .and_then(|v| v.get(&f).cloned())
            .or_else(|| known.id_for(f).map(str::to_owned))
    };
    let spell = |f: u32| known.id_for(f).map(str::to_owned);
    let converted = transform::creatures::convert_creatures(
        source,
        map,
        wanted.as_ref(),
        ids,
        vanilla,
        &item,
        &spell,
    )
    .map_err(failed(&format!("CREA ({plugin})")))?;
    let r = &converted.report;
    if r.seen > 0 {
        log(&lang::tr!(
            "transform: {plugin} — CREA {}: на донорах {}, со своей моделью {} (разных моделей {}, со звуками {}), не перенесено {}",
            "transform: {plugin} — CREA {}: on donors {}, with their own model {} (distinct models {}, with sounds {}), not converted {}",
            r.seen,
            r.converted - r.own_models,
            r.own_models,
            converted.models.len(),
            r.with_sounds,
            r.without_model.values().sum::<usize>()
        ));
    }
    if !r.without_model.is_empty() {
        outcome.problems.push(lang::tr!(
            "{plugin}: существа без донора в data/creatures.tsv и без частей тела не перенесены (папка модели — записей): {}", "{plugin}: creatures with neither a donor in data/creatures.tsv nor body parts not converted (model folder — records): {}",
            r.without_model
                .iter()
                .map(|(folder, n)| format!("{folder} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !r.donor_missing.is_empty() {
        outcome.problems.push(lang::tr!(
            "{plugin}: доноров из data/creatures.tsv нет в {TARGET_MASTER}: {}",
            "{plugin}: donors from data/creatures.tsv are missing in {TARGET_MASTER}: {}",
            r.donor_missing
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if r.items_lost + r.spells_lost > 0 {
        outcome.problems.push(lang::tr!(
            "{plugin}: у существ не нашлось предметов {} и заклинаний {} в плагине",
            "{plugin}: creature items ({}) and spells ({}) not found in the plugin",
            r.items_lost,
            r.spells_lost
        ));
    }
    if r.dropped_scripts > 0 {
        outcome.problems.push(lang::tr!(
            "{plugin}: существа перенесены без скриптов ({}) — их конвертации ещё нет", "{plugin}: creatures converted without their scripts ({}) — script conversion does not exist yet",
            r.dropped_scripts
        ));
    }
    Ok((converted.creatures, converted.models, converted.sound_gens))
}

/// Какие записи типа `tag` переносить: названные в `--only` и стоящие в мире (`in_world`).
/// `None` — фильтров нет, переносятся все.
fn wanted_records(
    source: &Plugin,
    tag: [u8; 4],
    map: impl Fn(u32) -> Option<u32>,
    wanted_ids: &BTreeSet<String>,
    in_world: Option<&BTreeSet<u32>>,
) -> Option<BTreeSet<u32>> {
    if wanted_ids.is_empty() {
        return in_world.cloned();
    }
    Some(
        source
            .records_of(tag)
            .filter(|r| {
                tes4_read::Fields::parse(&r.data, "EDID")
                    .ok()
                    .and_then(|f| f.opt_zstring(*b"EDID"))
                    .is_some_and(|e| wanted_ids.contains(&e.to_ascii_lowercase()))
            })
            .filter_map(|r| map(r.header.form_id))
            .filter(|id| in_world.is_none_or(|w| w.contains(id)))
            .collect(),
    )
}

/// Списки существ плагина (T11.4): `LVLC` → `LEVC`. `own` — ID перенесённых существ и NPC этого
/// плагина по абсолютному `FormID`; `load_index` — его место в порядке загрузки.
#[expect(clippy::too_many_arguments, reason = "одна стадия, всё её окружение")]
fn read_creature_lists(
    source: &Plugin,
    map: impl Fn(u32) -> Option<u32> + Copy,
    wanted_ids: &BTreeSet<String>,
    in_world: Option<&BTreeSet<u32>>,
    ids: &mut IdAllocator,
    own: &HashMap<u32, String>,
    load_index: usize,
    outcome: &mut Outcome,
    plugin: &str,
    log: &mut dyn FnMut(&str),
) -> Result<Vec<(u32, LeveledCreature)>, PipelineError> {
    let wanted = wanted_records(source, *b"LVLC", map, wanted_ids, in_world);
    let known = ids.clone();
    let actor = |f: u32| {
        own.get(&f).cloned().or_else(|| {
            let foreign = usize::try_from(f >> 24).is_ok_and(|i| i != load_index);
            foreign
                .then(|| known.id_for(f).map(str::to_owned))
                .flatten()
        })
    };
    let converted = transform::creature_lists::convert_creature_lists(
        source,
        map,
        wanted.as_ref(),
        ids,
        &actor,
    )
    .map_err(failed(&format!("LVLC ({plugin})")))?;
    let r = &converted.report;
    if r.seen > 0 {
        log(&lang::tr!(
            "transform: {plugin} — LVLC {}: LEVC {}, пустых не записано {}, строк без цели убрано {}",
            "transform: {plugin} — LVLC {}: LEVC {}, empty ones not written {}, rows without a target removed {}",
            r.seen,
            converted.lists.len(),
            r.empty,
            r.entries_unresolved
        ));
    }
    if r.entries_unresolved + r.empty > 0 {
        outcome.problems.push(lang::tr!(
            "{plugin}: из списков существ убрано строк: {} — существо не перенесено, NPC не из мира или запись из не выбранного плагина; пустых списков не записано: {}", "{plugin}: rows removed from creature lists: {} — a creature that is not converted, an NPC not from the world or a record from a plugin that is not selected; empty lists not written: {}",
            r.entries_unresolved, r.empty
        ));
    }
    if r.templates_dropped + r.scripts_dropped > 0 {
        outcome.problems.push(lang::tr!(
            "{plugin}: списки существ перенесены без шаблона существа ({}) и без скрипта ({}) — у LEVC таких полей нет", "{plugin}: creature lists converted without their creature template ({}) and script ({}) — LEVC has no such fields",
            r.templates_dropped, r.scripts_dropped
        ));
    }
    outcome
        .problems
        .extend(r.notes.iter().map(|n| format!("{plugin}: {n}")));
    Ok(converted.lists)
}

/// NPC плагина (фаза 7). Предметы и заклинания берутся по уже выданным ID: что не попадёт в
/// плагин (меш не сконвертировался, заклинание без эффектов), вычистит `prune_contents`. Нет
/// `Morrowind.esm` — NPC не переносятся, с замечанием.
#[expect(clippy::too_many_arguments, reason = "одна стадия, всё её окружение")]
fn read_actors(
    config: &Config,
    source: &Plugin,
    map: impl Fn(u32) -> Option<u32>,
    npcs: &BTreeSet<u32>,
    ids: &mut IdAllocator,
    vanilla_items: Option<&HashMap<u32, String>>,
    vanilla_bodies: &mut Option<Vec<tes3_out::vanilla::VanillaBodyPart>>,
    outcome: &mut Outcome,
    plugin: &str,
) -> Result<Option<ConvertedActors>, PipelineError> {
    if vanilla_bodies.is_none() {
        let master = tes3_out::vanilla::read_file(&config.target_data, TARGET_MASTER)
            .and_then(|bytes| tes3_out::vanilla::body_parts(&bytes));
        match master {
            Ok(list) => *vanilla_bodies = Some(list),
            Err(e) => {
                outcome.problems.push(lang::tr!(
                    "{plugin}: NPC не перенесены — не прочитать части тела {TARGET_MASTER} ({e})", "{plugin}: NPCs not converted — cannot read the body parts of {TARGET_MASTER} ({e})"
                ));
                return Ok(None);
            }
        }
    }
    let bodies = vanilla_bodies.as_deref().expect("заполнено выше или вышли");
    let known = ids.clone();
    let item = |f: u32| {
        vanilla_items
            .and_then(|v| v.get(&f).cloned())
            .or_else(|| known.id_for(f).map(str::to_owned))
    };
    let spell = |f: u32| known.id_for(f).map(str::to_owned);
    convert_actors(source, map, npcs, ids, bodies, &item, &spell)
        .map(Some)
        .map_err(failed(&format!("NPC ({plugin})")))
}

/// Темы и приветствия NPC плагина (T10.6) и файлы голоса их реплик (T10.8). Нет `Morrowind.esm`
/// или реплики не разобрать — без диалогов, с замечанием: NPC переносятся и без них.
#[expect(clippy::too_many_arguments, reason = "одна стадия, всё её окружение")]
fn read_dialogue(
    config: &Config,
    source: &Plugin,
    map: &dyn Fn(u32) -> Option<u32>,
    owner: &dyn Fn(u32) -> Option<String>,
    world: &WorldWork,
    actors: &ConvertedActors,
    ids: &IdAllocator,
    vanilla: &mut Option<crate::dialogue::VanillaDialogue>,
    outcome: &mut Outcome,
    plugin: &str,
    log: &mut dyn FnMut(&str),
) -> crate::dialogue::Dialogue {
    if vanilla.is_none() {
        let voice_archives = voice_archives(&config.source_data);
        match crate::dialogue::VanillaDialogue::read(
            &config.target_data,
            TARGET_MASTER,
            &voice_archives,
        ) {
            Ok(v) => {
                outcome.problems.extend(v.voice_problems.iter().cloned());
                *vanilla = Some(v);
            }
            Err(e) => {
                outcome.problems.push(lang::tr!(
                    "{plugin}: диалоги не перенесены — не прочитать темы {TARGET_MASTER} ({e})", "{plugin}: dialogue not converted — cannot read the topics of {TARGET_MASTER} ({e})"
                ));
                return crate::dialogue::Dialogue::default();
            }
        }
    }
    let vanilla = vanilla.as_ref().expect("заполнено выше или вышли");
    let d = match crate::dialogue::convert_plugin(
        source,
        map,
        owner,
        &world.source,
        actors,
        ids,
        &world.vanilla,
        vanilla,
    ) {
        Ok(d) => d,
        Err(e) => {
            outcome.problems.push(lang::tr!(
                "{plugin}: диалоги не перенесены ({e})",
                "{plugin}: dialogue not converted ({e})"
            ));
            return crate::dialogue::Dialogue::default();
        }
    };
    let kept: usize = d.selection.by_kind.values().map(|k| k.kept).sum();
    let r = &d.report;
    log(&lang::tr!(
        "dialogue: {plugin} — реплик отобрано {kept}, перенесено {} (голосовых и служебных {}); тем {} (дописано в ванильные {}), INFO {} (копий выбора {}, продолжений {}); тем только для выбора {}, их реплик не достигнуто {}",
        "dialogue: {plugin} — lines selected {kept}, converted {} (voice and service {}); topics {} (appended to vanilla {}), INFO {} (choice copies {}, continuations {}); choice-only topics {}, their lines never reached {}",
        r.source_infos,
        r.source_voice,
        r.topics,
        r.merged,
        r.infos,
        r.choice_copies,
        r.continuations,
        r.choice_only_topics,
        r.unreached
    ));
    if r.voiced > 0 {
        log(&lang::tr!(
            "dialogue: {plugin} — INFO с голосом {}, файлов голоса {}",
            "dialogue: {plugin} — voiced INFO {}, voice files {}",
            r.voiced,
            r.voice_files
        ));
    }
    if r.results_dropped > 0 {
        outcome.problems.push(lang::tr!(
            "{plugin}: {} реплик перенесены без скрипта-результата Oblivion — транслятора нет (фаза 11)", "{plugin}: {} lines converted without their Oblivion result script — there is no translator (phase 11)",
            r.results_dropped
        ));
    }
    if !r.rejected.is_empty() {
        outcome.problems.push(lang::tr!(
            "{plugin}: копий реплик не записано (причина: число): {:?}",
            "{plugin}: line copies not written (reason: count): {:?}",
            r.rejected
        ));
    }
    d
}

/// Текстуры земли worldspace: распаковать и положить в `Textures\` по путям из `LTEX`.
/// Отсутствующая в архивах текстура — предупреждение: в игре на её месте будет заглушка.
fn write_land_textures(
    config: &Config,
    archives: &[PathBuf],
    works: &[PluginWork<'_>],
    by_plugin: &mut PluginAssets,
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> Result<(), PipelineError> {
    for work in works {
        let Some(world) = work.world.as_ref().and_then(|w| w.converted.as_ref()) else {
            continue;
        };
        let wanted: Vec<String> = world.texture_files.values().cloned().collect();
        let extracted = extract_sources(config, archives, &wanted).map_err(failed(lang::tr(
            "extract (текстуры земли)",
            "extract (land textures)",
        )))?;
        log(&lang::tr!(
            "extract: текстуры земли — распаковано {}, пропущено (кеш) {}, нет в архиве {}",
            "extract: land textures — unpacked {}, skipped (cache) {}, not in the archive {}",
            extracted.extracted,
            extracted.skipped,
            extracted.missing.len()
        ));
        let set = by_plugin.entry(work.output_name()).or_default();
        for (rel, source_path) in &world.texture_files {
            let from = extracted_path(&config.work_dir, source_path).map_err(failed("extract"))?;
            if !from.is_file() {
                outcome.textures_missing.push(source_path.clone());
                continue;
            }
            let to = join_backslash(&config.output_dir.join("Textures"), rel);
            if let Some(dir) = to.parent() {
                fs::create_dir_all(dir).map_err(failed(&dir.display().to_string()))?;
            }
            fs::copy(&from, &to).map_err(failed(&to.display().to_string()))?;
            set.insert(asset_relative("Textures", rel));
            outcome.textures_copied += 1;
        }
    }
    Ok(())
}

/// Карта мира для мода MWSE-Lua (`crate::world_map`): лист из `WRLD` источника в `Textures\`,
/// описание, где он лежит в мире Morrowind, и сам мод. Мир без листа (`ICON` и `MNAM`) или с
/// листом, которого нет в архивах, — замечание: карты у него не будет, остальное не страдает.
fn write_world_maps(
    config: &Config,
    archives: &[PathBuf],
    works: &[PluginWork<'_>],
    by_plugin: &mut PluginAssets,
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> Result<(), PipelineError> {
    let output = &config.output_dir;
    for work in works {
        let Some(world) = &work.world else { continue };
        let Some(bounds) = world.converted.as_ref().and_then(|c| c.report.bounds) else {
            continue;
        };
        let name = &world.source.editor_id;
        let Some(map) = &world.source.map else {
            outcome.problems.push(lang::tr!(
                "{name}: у мира нет листа карты (ICON и MNAM в WRLD) — карта для MWSE не записана",
                "{name}: the world has no map sheet (ICON and MNAM in WRLD) — no map for MWSE written"
            ));
            continue;
        };
        let copy = match mesh_convert::textures::target_of(&map.texture) {
            Ok(copy) => copy,
            Err(e) => {
                outcome.problems.push(lang::tr!(
                    "{name}: лист карты не перенести ({e}) — карта для MWSE не записана",
                    "{name}: the map sheet cannot be converted ({e}) — no map for MWSE written"
                ));
                continue;
            }
        };
        extract_sources(config, archives, std::slice::from_ref(&copy.source_path)).map_err(
            failed(lang::tr("extract (карта мира)", "extract (world map)")),
        )?;
        let from =
            extracted_path(&config.work_dir, &copy.source_path).map_err(failed("extract"))?;
        if !from.is_file() {
            outcome.problems.push(lang::tr!(
                "{name}: листа карты {} нет в архивах — карта для MWSE не записана",
                "{name}: the map sheet {} is not in the archives — no map for MWSE written",
                copy.source_path
            ));
            continue;
        }
        let bytes = fs::read(&from).map_err(failed(&from.display().to_string()))?;
        let rect = crate::world_map::dds_size(&bytes).and_then(|file| {
            Some((
                file,
                transform::world_map::sheet_rect(&map.size, file, world.offset)?,
            ))
        });
        let Some(((width, height), sheet)) = rect else {
            outcome.problems.push(lang::tr!(
                "{name}: лист карты {} — не DDS или MNAM не описывает ни одной ячейки; карта для MWSE не записана",
                "{name}: the map sheet {} is not a DDS or its MNAM covers no cells; no map for MWSE written",
                copy.source_path
            ));
            continue;
        };
        if !width.is_power_of_two() || !height.is_power_of_two() {
            outcome.problems.push(lang::tr!(
                "{name}: лист карты {width}×{height} — стороны не степени двойки, движок может его не показать",
                "{name}: the map sheet is {width}×{height} — the sides are not powers of two, the engine may not show it"
            ));
        }
        write_file(&output.join("Textures").join(&copy.target_name), &bytes)?;
        let description = crate::world_map::Description {
            world: name.clone(),
            name: world.source.name.clone(),
            plugin: work.output_name(),
            texture: copy.target_name.clone(),
            width,
            height,
            cells: bounds,
            sheet,
        };
        let json = crate::world_map::description_file(name);
        for (relative, text) in [
            (json.as_str(), description.to_json()),
            (
                crate::world_map::SCRIPT_FILE,
                crate::world_map::SCRIPT.to_owned(),
            ),
        ] {
            let path = relative.split('/').fold(output.clone(), |p, s| p.join(s));
            write_file(&path, text.as_bytes())?;
        }
        // Лист — обычная текстура плагина (с BSA уйдёт в архив); описание и мод всегда лежат
        // отдельными файлами (`package::is_loose_only`).
        let set = by_plugin.entry(work.output_name()).or_default();
        set.insert(format!("Textures/{}", copy.target_name));
        set.insert(json.clone());
        set.insert(crate::world_map::SCRIPT_FILE.to_owned());
        outcome.world_maps_written += 1;
        log(&lang::tr!(
            "write: карта мира {name} для MWSE — лист {}, описание {json}",
            "write: world map of {name} for MWSE — sheet {}, description {json}",
            copy.target_name
        ));
    }
    Ok(())
}

/// Ячейки, ландшафт, регионы и размещения worldspace. Идёт после отбраковки мешей: ссылка
/// ставится, только если её объект попал в плагин.
fn convert_worlds(
    request: &Request,
    works: &mut [PluginWork<'_>],
    ids: &mut IdAllocator,
    door_pivots: &HashMap<u32, [f32; 3]>,
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> Result<(), PipelineError> {
    for work in works.iter_mut() {
        let Some(world) = &mut work.world else {
            continue;
        };
        let markers = place_sound_markers(
            &world.base_kinds,
            &work.sound_sources,
            &mut work.sounds,
            &mut work.sound_origins,
            &mut work.scripts,
            &mut work.sound_markers,
            outcome,
            &work.info.name,
        );
        if !markers.is_empty() {
            log(&lang::tr!(
                "world: размещённых SOUN {} — невидимые активаторы со скриптом петли",
                "world: placed SOUN {} — invisible activators with a loop script",
                markers.len()
            ));
        }
        // ID, модель источника, актёр ли (ссылка актёра пишется постоянной).
        let mut placed: HashMap<u32, (String, String, bool)> = work
            .objects
            .iter()
            .zip(&work.origins)
            .filter(|(_, o)| o.body.is_none() && o.tree.is_none())
            .map(|(obj, o)| (o.form_id, (obj.id().to_owned(), o.model.clone(), false)))
            .collect();
        // Двери с перенесённой петлёй (T2.6b): их ссылки сдвигаются вслед за началом меша.
        let pivots: HashMap<u32, [f32; 3]> = work
            .origins
            .iter()
            .filter_map(|o| Some((o.form_id, *door_pivots.get(&o.form_id)?)))
            .collect();
        for (form_id, id) in &world.vanilla {
            placed.insert(*form_id, (id.clone(), String::new(), false));
        }
        // Размещённые звуки — маркеры с ванильным мешем: модели источника нет, маркером Oblivion
        // не считаются.
        for (form_id, id) in markers {
            placed.insert(form_id, (id, String::new(), false));
        }
        // NPC: модели у записи нет — маркером его не счесть.
        for (form_id, npc) in work.actors.iter().flat_map(|a| &a.npcs) {
            placed.insert(*form_id, (npc.id.clone(), String::new(), true));
        }
        // Существа (`ACRE`, T11.4) — актёры, как NPC. Список существ стоит временной ссылкой,
        // как у ванили: существо на его месте создаёт движок.
        for (form_id, c) in &work.creatures {
            placed.insert(*form_id, (c.id.clone(), String::new(), true));
        }
        for (form_id, l) in &work.creature_lists {
            placed.insert(*form_id, (l.id.clone(), String::new(), false));
        }
        // Деревья: ID вариантов по номеру семени. Вариант, чей меш не собрался, заменяется
        // первым уцелевшим — номера остальных не сдвигаются.
        let mut tree_slots: HashMap<u32, BTreeMap<usize, String>> = HashMap::new();
        for (obj, o) in work.objects.iter().zip(&work.origins) {
            if let Some(tree) = &o.tree {
                tree_slots
                    .entry(o.form_id)
                    .or_default()
                    .insert(tree.variant, obj.id().to_owned());
            }
        }
        let trees: HashMap<u32, Vec<String>> = tree_slots
            .into_iter()
            .map(|(form_id, slots)| {
                let last = slots.keys().max().copied().unwrap_or_default();
                let first = slots.values().next().cloned().unwrap_or_default();
                let ids = (0..=last)
                    .map(|k| slots.get(&k).cloned().unwrap_or_else(|| first.clone()))
                    .collect();
                (form_id, ids)
            })
            .collect();
        let offset = request.offset.map_or_else(
            || default_offset(&world.source),
            |(dx, dy)| Offset { dx, dy },
        );
        let base =
            |form_id: u32| match (placed.get(&form_id), trees.get(&form_id)) {
                (Some((id, model, actor)), _) => BaseObject::Placed {
                    id: id.clone(),
                    model: model.clone(),
                    actor: *actor,
                    pivot: pivots.get(&form_id).copied(),
                },
                (None, Some(variants)) => BaseObject::Tree {
                    variants: variants.clone(),
                },
                (None, None) => {
                    BaseObject::Unsupported(world.base_kinds.get(&form_id).cloned().unwrap_or_else(
                        || lang::tr("нет в плагине", "not in the plugin").to_owned(),
                    ))
                }
            };
        let converted = convert_exterior(&world.source, offset, ids, base)
            .map_err(failed(&format!("world ({})", world.source.editor_id)))?;
        let r = &converted.report;
        let (x0, y0, x1, y1) = r.bounds.unwrap_or_default();
        log(&lang::tr!(
            "world: {} → смещение ({}, {}), ячейки x {x0}..{x1}, y {y0}..{y1}: CELL {}, LAND {}, LTEX {}, REGN {}, ссылок {}",
            "world: {} → offset ({}, {}), cells x {x0}..{x1}, y {y0}..{y1}: CELL {}, LAND {}, LTEX {}, REGN {}, references {}",
            world.source.editor_id,
            offset.dx,
            offset.dy,
            converted.cells.len(),
            converted.lands.len(),
            converted.land_textures.len(),
            converted.regions.len(),
            r.placed
        ));
        log(&lang::tr!(
            "world: интерьеров {}, дверей с назначением {}, заперто {} (с ключом {})",
            "world: interiors {}, doors with a destination {}, locked {} (with a key {})",
            converted.interiors.len(),
            r.doors_linked,
            r.locks,
            r.locks_keyed
        ));
        let count_of = |ids: BTreeSet<&str>| {
            converted
                .cells
                .iter()
                .flat_map(|c| &c.references)
                .chain(converted.interiors.iter().flat_map(|c| &c.references))
                .filter(|r| ids.contains(r.id.as_str()))
                .count()
        };
        let creature_refs = count_of(work.creatures.iter().map(|(_, c)| c.id.as_str()).collect());
        let list_refs = count_of(
            work.creature_lists
                .iter()
                .map(|(_, l)| l.id.as_str())
                .collect(),
        );
        log(&lang::tr!(
            "world: ссылок актёров {} (постоянные), из них существ {}; ссылок на списки существ {}",
            "world: actor references {} (persistent), creatures among them {}; references to creature lists {}",
            r.actors,
            creature_refs,
            list_refs
        ));
        if r.doors_rehinged > 0 {
            log(&lang::tr!(
                "world: ссылок на двери с перенесённой петлёй {} — сдвинуты вслед за началом координат меша",
                "world: references to doors with a moved hinge {} — shifted along with the mesh origin",
                r.doors_rehinged
            ));
        }
        let pg = &r.path_grids;
        log(&lang::tr!(
            "world: PGRD {}, точек {}; связей между ячейками Oblivion стало рёбрами {}, ушло в соседнюю ячейку Morrowind и потеряно {}",
            "world: PGRD {}, points {}; links between Oblivion cells turned into edges {}, lost into a neighbouring Morrowind cell {}",
            pg.grids,
            pg.points,
            pg.links_joined,
            pg.links_outside
        ));
        let merged = &world.source.merged;
        if !merged.children.is_empty() {
            log(&lang::tr!(
                "world: влиты города {}: дублей выброшено {}, ворот между ними {}",
                "world: cities merged in {}: duplicates dropped {}, gates between them {}",
                merged.children.join(", "),
                merged.duplicates,
                merged.gates
            ));
        }
        let name = &world.source.editor_id;
        let skipped: Vec<String> = r
            .skipped_by_kind
            .iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect();
        if !skipped.is_empty() {
            outcome.problems.push(lang::tr!(
                "{name}: не поставлены размещения, чей объект не перенесён (у переносимых типов вроде STAT — меш не сконвертировался): {}", "{name}: references whose object was not converted are not placed (for converted types such as STAT — the mesh failed to convert): {}",
                skipped.join(", ")
            ));
        }
        for (count, what) in [
            (
                r.disabled,
                lang::tr(
                    "выключенных размещений не поставлено",
                    "disabled references not placed",
                ),
            ),
            (
                r.markers,
                lang::tr("маркеров не поставлено", "markers not placed"),
            ),
            (
                r.duplicates,
                lang::tr(
                    "повторных ссылок не поставлено — тот же объект уже стоит в той же точке ячейки с тем же поворотом",
                    "repeated references not placed — the same object already stands at the same point of the cell with the same rotation",
                ),
            ),
            (
                r.trees_without_seed,
                lang::tr(
                    "деревьев без XSED — вариант выбран по FormID ссылки (игра выбирает случайно при каждой загрузке)",
                    "trees without XSED — the variant is picked by the reference FormID (the game picks one at random on every load)",
                ),
            ),
            (
                r.scale_clamped,
                lang::tr(
                    "размещений с масштабом вне 0.5…2.0 — масштаб зажат",
                    "references with a scale outside 0.5…2.0 — scale clamped",
                ),
            ),
            (
                r.teleport_doors,
                lang::tr(
                    "дверей-телепортов стоят, но никуда не ведут (другой worldspace или интерьер, которого нет в плагине)",
                    "teleport doors are placed but lead nowhere (another worldspace or an interior missing from the plugin)",
                ),
            ),
            (
                r.water_levels_lost,
                lang::tr(
                    "ячеек со своим уровнем воды — у Morrowind вода снаружи на нуле",
                    "cells with their own water level — Morrowind keeps exterior water at zero",
                ),
            ),
            (
                r.seam_conflicts,
                lang::tr(
                    "вершин на кромках ячеек разошлись в источнике — взята южная/западная",
                    "vertices on cell edges disagree in the source — the southern/western one is used",
                ),
            ),
            (
                r.clamped_heights,
                lang::tr(
                    "перепадов высоты круче предела VHGT (1016 юнитов на 128) — стали склоном",
                    "height steps steeper than the VHGT limit (1016 units per 128) — turned into a slope",
                ),
            ),
            (
                r.absorbed_patches,
                lang::tr(
                    "одиночных патчей земли получили текстуру соседей",
                    "single land patches took the texture of their neighbours",
                ),
            ),
            (
                r.missing_land_textures,
                lang::tr(
                    "текстур земли без записи LTEX — легли текстурой по умолчанию",
                    "land textures without an LTEX record — replaced by the default texture",
                ),
            ),
            (
                r.lock_keys_lost,
                lang::tr(
                    "замков стоят без ключа — ключ не перенесён",
                    "locks placed without a key — the key was not converted",
                ),
            ),
            (
                r.path_grids.links_unmatched,
                lang::tr(
                    "связей сетки путей потеряно — точки на том конце не нашлось",
                    "path grid links lost — no point found at the other end",
                ),
            ),
            (
                r.path_grids.neighbours_cut,
                lang::tr(
                    "соседей точек сетки путей сверх 255 отрезано",
                    "path grid point neighbours beyond 255 cut off",
                ),
            ),
        ] {
            if count > 0 {
                outcome.problems.push(format!("{name}: {count} {what}"));
            }
        }
        if !r.weather_by_class.is_empty() {
            outcome.problems.push(lang::tr!(
                "{name}: погода не из data/weather.tsv, тип по классу WTHR: {}",
                "{name}: weather missing from data/weather.tsv, type taken from the WTHR class: {}",
                r.weather_by_class.join(", ")
            ));
        }
        world.converted = Some(converted);
        world.offset = offset;
    }
    Ok(())
}

/// Размещённые звуки (T10.3): на каждый `SOUN`, который стоит в мире, — `SOUN` плагина (если
/// на него ещё никто не сослался), скрипт петли и невидимый активатор-маркер
/// (`transform::sounds::sound_marker`). Возвращает `FormID` звука → ID маркера, по возрастанию
/// `FormID`.
#[expect(
    clippy::too_many_arguments,
    reason = "поля одной PluginWork, занятой по частям"
)]
fn place_sound_markers(
    base_kinds: &HashMap<u32, String>,
    sources: &BTreeMap<u32, (String, tes4_read::Sound)>,
    sounds: &mut Vec<Sound>,
    sound_origins: &mut Vec<SoundOrigin>,
    scripts: &mut Vec<Script>,
    markers: &mut Vec<Activator>,
    outcome: &mut Outcome,
    plugin: &str,
) -> Vec<(u32, String)> {
    let mut placed: Vec<u32> = base_kinds
        .iter()
        .filter(|(_, kind)| kind.as_str() == "SOUN")
        .map(|(&id, _)| id)
        .collect();
    placed.sort_unstable();
    let mut out = Vec::new();
    for form_id in placed {
        let Some((id, source)) = sources.get(&form_id) else {
            continue;
        };
        if !sounds.iter().any(|s| s.id.eq_ignore_ascii_case(id)) {
            match transform::sounds::convert_sound(source, id.clone()) {
                Ok(c) => {
                    sounds.push(c.sound);
                    sound_origins.push(c.origin);
                }
                Err(message) => {
                    outcome
                        .problems
                        .push(format!("{plugin}: SOUN {}: {message}", source.editor_id));
                    continue;
                }
            }
        }
        let (marker, script) = transform::sounds::sound_marker(id);
        if !scripts.iter().any(|s| s.id() == script.id()) {
            scripts.push(script);
        }
        out.push((form_id, marker.id.clone()));
        markers.push(marker);
    }
    out
}

/// Звуки регионов (T10.2): `RDSD` региона-источника → список `REGN.SNAM` и `SOUN` к нему (с
/// вариантами по файлам каталогов, `transform::sounds::region_sounds`). `SOUN` ищется среди
/// прочитанных в том же плагине; каталог раскрывается по свободным файлам и архивам.
fn add_region_sounds(
    config: &Config,
    archives: &[PathBuf],
    installed: &[PluginInfo],
    works: &mut [PluginWork<'_>],
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> Result<(), PipelineError> {
    for work in works.iter_mut() {
        let Some(world) = &mut work.world else {
            continue;
        };
        let Some(converted) = &mut world.converted else {
            continue;
        };
        let (mut unresolved, mut dropped) = (0usize, 0usize);
        let mut files_of: HashMap<String, Vec<String>> = HashMap::new();
        let mut known: BTreeSet<String> = work
            .sounds
            .iter()
            .map(|s| s.id.to_ascii_lowercase())
            .collect();
        let mut rows = 0usize;
        for region in &mut converted.regions {
            let Some(list) = converted.region_sounds.get(&region.id) else {
                continue;
            };
            // Файлы каталогов — до разбора, чтобы `RegionEntry` держал на них ссылки.
            for rs in list {
                let Some((_, source)) = plugins::absolute_form_id(installed, work.info, rs.sound)
                    .and_then(|id| work.sound_sources.get(&id))
                else {
                    continue;
                };
                if source.is_directory() && !files_of.contains_key(&source.file) {
                    let dir = transform::paths::normalize(&source.file).unwrap_or_default();
                    let files = extract::list_directory(
                        &config.source_data,
                        archives,
                        &format!("sound\\{dir}"),
                        "wav",
                    )
                    .map_err(failed(lang::tr(
                        "extract (каталоги звуков)",
                        "extract (sound folders)",
                    )))?
                    .into_iter()
                    .map(|p| p.trim_start_matches("sound\\").to_owned())
                    .collect();
                    files_of.insert(source.file.clone(), files);
                }
            }
            let mut entries = Vec::new();
            for rs in list {
                let Some((id, source)) = plugins::absolute_form_id(installed, work.info, rs.sound)
                    .and_then(|id| work.sound_sources.get(&id))
                else {
                    unresolved += 1;
                    continue;
                };
                entries.push(transform::sounds::RegionEntry {
                    id,
                    source,
                    chance: rs.chance,
                    files: files_of.get(&source.file).map_or(&[], Vec::as_slice),
                });
            }
            let out = transform::sounds::region_sounds(&entries);
            dropped += out.dropped;
            outcome
                .problems
                .extend(out.notes.iter().map(|n| format!("{}: {n}", work.info.name)));
            rows += out.list.len();
            region.sounds = out.list;
            for c in out.sounds {
                if known.insert(c.sound.id.to_ascii_lowercase()) {
                    work.sounds.push(c.sound);
                    work.sound_origins.push(c.origin);
                }
            }
        }
        log(&lang::tr!(
            "world: звуки регионов — у {} из {} REGN, строк {rows}",
            "world: region sounds — in {} of {} REGN, rows {rows}",
            converted
                .regions
                .iter()
                .filter(|r| !r.sounds.is_empty())
                .count(),
            converted.regions.len()
        ));
        if unresolved + dropped > 0 {
            outcome.problems.push(lang::tr!(
                "{}: звуков регионов не перенесено: {unresolved} — нет SOUN в плагине, {dropped} — шанс 0 или пустой каталог", "{}: region sounds not converted: {unresolved} — no SOUN in the plugin, {dropped} — chance 0 or an empty folder",
                world.source.editor_id
            ));
        }
    }
    Ok(())
}

/// Основная работа: чтение плагинов, преобразование, меши, текстуры, запись, проверка.
pub fn run(
    config: &Config,
    request: &Request,
    log: &mut dyn FnMut(&str),
) -> Result<Outcome, PipelineError> {
    let tools = tools(config, request)?;
    let mut outcome = Outcome::default();

    // read + transform
    let installed = plugins::list(&config.source_data)
        .map_err(failed(&config.source_data.display().to_string()))?;
    let (mut works, mut ids) = read_plugins(config, request, &installed, &mut outcome, log)?;
    log(&lang::tr!(
        "transform: {} объектов к конвертации в {} плагинах",
        "transform: {} objects to convert in {} plugins",
        outcome.statics_seen,
        works.len()
    ));
    if request.morrowind_prices {
        let mut raised = transform::prices::Raised::default();
        for work in &mut works {
            let r = transform::prices::raise_prices(&mut work.objects);
            raised.weapons += r.weapons;
            raised.armor += r.armor;
        }
        outcome.prices_raised = raised.weapons + raised.armor;
        log(&lang::tr!(
            "transform: цены подтянуты к Morrowind — оружие {}, броня {}",
            "transform: prices raised to Morrowind's — weapons {}, armor {}",
            raised.weapons,
            raised.armor
        ));
    }

    // extract: меши. Сначала «свободные» файлы в `Data\Meshes` (они перекрывают архивы), потом
    // архивы источника.
    let archives = source_archives(&config.source_data);
    // Пустая модель — свет без меша (T2.2): конвертировать нечего.
    let models: BTreeSet<String> = works
        .iter()
        .flat_map(|w| &w.origins)
        .filter(|o| o.body.is_none() && o.tree.is_none() && o.door.is_none() && !o.model.is_empty())
        .map(|o| o.model.clone())
        .collect();
    // Двери (T2.6b): ключ → (модель источника, путь меша в плагине).
    let door_jobs: BTreeMap<String, (String, String)> = works
        .iter()
        .flat_map(|w| &w.origins)
        .filter_map(|o| Some((mesh_key(o), (o.model.clone(), o.door.clone()?))))
        .collect();
    let body_jobs: BTreeMap<String, transform::worn::BodyMesh> = works
        .iter()
        .flat_map(|w| &w.origins)
        .filter_map(|o| Some((mesh_key(o), o.body.clone()?)))
        .collect();
    // Деревья: `.spt` лежит не в `Meshes`, а в `Trees` (путь в `model` уже с ним).
    let tree_jobs: BTreeMap<String, (String, transform::TreeSource)> = works
        .iter()
        .flat_map(|w| &w.origins)
        .filter_map(|o| Some((mesh_key(o), (o.model.clone(), o.tree.clone()?))))
        .collect();
    let wanted: Vec<String> = models
        .iter()
        .chain(body_jobs.values().map(|b| &b.model))
        .chain(door_jobs.values().map(|(model, _)| model))
        .map(|m| format!("meshes\\{m}"))
        .chain(tree_jobs.values().map(|(spt, _)| spt.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    outcome.extract_meshes = extract_sources(config, &archives, &wanted)
        .map_err(failed(lang::tr("extract (меши)", "extract (meshes)")))?;
    log(&lang::tr!(
        "extract: меши — распаковано {}, пропущено (кеш) {}, нет в архиве {}",
        "extract: meshes — unpacked {}, skipped (cache) {}, not in the archive {}",
        outcome.extract_meshes.extracted,
        outcome.extract_meshes.skipped,
        outcome.extract_meshes.missing.len()
    ));

    // meshes
    let mut registry = TextureRegistry::new();
    let mut results: BTreeMap<String, MeshEntry> = BTreeMap::new();
    for model in &models {
        let archive_path = format!("meshes\\{model}");
        let rel = mesh_path(model).ok_or_else(|| {
            PipelineError::Failed(lang::tr!(
                "путь меша {model:?} не представим в выходе",
                "mesh path {model:?} cannot be represented in the output"
            ))
        })?;
        results.insert(
            model.clone(),
            MeshEntry {
                rel,
                source: model.clone(),
                result: convert_one(&config.work_dir, &archive_path, &mut registry, &mut outcome),
            },
        );
    }
    let mut door_info: BTreeMap<String, DoorInfo> = BTreeMap::new();
    for (key, (model, mesh)) in &door_jobs {
        let (result, info) = door_one(
            &config.work_dir,
            &format!("meshes\\{model}"),
            &mut registry,
            &mut outcome,
        );
        if let Some(info) = info {
            door_info.insert(key.clone(), info);
        }
        results.insert(
            key.clone(),
            MeshEntry {
                rel: mesh.clone(),
                source: format!("{model}#door"),
                result,
            },
        );
    }
    for (key, (spt, tree)) in &tree_jobs {
        let seed = tree
            .seed
            .map_or_else(|| "spt".to_owned(), |s| s.to_string());
        results.insert(
            key.clone(),
            MeshEntry {
                rel: tree.mesh.clone(),
                source: format!("{spt}#{seed}"),
                result: tree_one(&config.work_dir, spt, tree, &mut registry),
            },
        );
    }
    if !body_jobs.is_empty() {
        let skeletons = bodies::load(&config.target_data);
        if let Err(e) = &skeletons {
            outcome.problems.push(lang::tr!(
                "скелет Morrowind не собран, броня и одежда без тел: {e}", "the Morrowind skeleton could not be built, armor and clothing have no body parts: {e}"
            ));
        }
        for (key, body) in &body_jobs {
            let result = match &skeletons {
                Ok(s) => bodies::convert(&config.work_dir, body, s, &mut registry),
                Err(e) => Err(lang::tr!(
                    "нет скелета Morrowind: {e}",
                    "no Morrowind skeleton: {e}"
                )),
            };
            let side = match body.side {
                None => "",
                Some(transform::worn::Side::Left) => "#l",
                Some(transform::worn::Side::Right) => "#r",
            };
            let gender = if body.female { "#f" } else { "#m" };
            results.insert(
                key.clone(),
                MeshEntry {
                    rel: body.mesh.clone(),
                    source: format!("{}{gender}{side}", body.model),
                    result,
                },
            );
        }
    }
    bake_faces(
        config,
        &archives,
        &mut works,
        &mut results,
        &mut registry,
        &mut outcome,
        log,
    )?;
    build_creature_models(
        config,
        &archives,
        &works,
        &mut results,
        &mut registry,
        &mut outcome,
        log,
    )?;
    let ok_meshes = results.values().filter(|r| r.result.is_ok()).count();
    log(&lang::tr!(
        "meshes: сконвертировано {ok_meshes} из {}",
        "meshes: converted {ok_meshes} of {}",
        results.len()
    ));
    if outcome.particles_converted + outcome.particles_dropped > 0 {
        log(&lang::tr!(
            "meshes: систем частиц — стало эмиттерами Morrowind {} (с потерями {}), не перенесено {} (эффекты по событию, частицы-меши)",
            "meshes: particle systems — turned into Morrowind emitters {} (with losses {}), not converted {} (event-triggered effects, mesh particles)",
            outcome.particles_converted,
            outcome.particles_lossy,
            outcome.particles_dropped
        ));
    }

    // Что попало в плагины, а что нет.
    for work in &mut works {
        let mut objects = Vec::new();
        let mut origins = Vec::new();
        let mut dropped_bodies: BTreeSet<String> = BTreeSet::new();
        for (object, origin) in work.objects.drain(..).zip(work.origins.drain(..)) {
            if origin.body.is_none() && origin.model.is_empty() {
                // Свет без меша: записи меш не нужен.
                objects.push(object);
                origins.push(origin);
                continue;
            }
            match &results[&mesh_key(&origin)].result {
                Ok(_) => {
                    objects.push(object);
                    origins.push(origin);
                }
                Err(reason) => {
                    if let Object::BodyPart(b) = &object {
                        dropped_bodies.insert(b.id.clone());
                    }
                    outcome.statics_skipped.push(format!(
                        "{}: {} ({}): {reason}",
                        work.info.name, origin.editor_id, origin.model
                    ));
                }
            }
        }
        // Вещь без своей части тела была бы невидимой на персонаже: отбрасываем и её, и
        // оставшиеся её части.
        let broken: BTreeSet<String> = objects
            .iter()
            .zip(&origins)
            .filter(|(o, _)| {
                referenced_bodies(o)
                    .iter()
                    .any(|id| dropped_bodies.contains(*id))
            })
            .flat_map(|(o, origin)| {
                outcome.statics_skipped.push(lang::tr!(
                    "{}: {} ({}): часть тела не сконвертировалась",
                    "{}: {} ({}): the body part failed to convert",
                    work.info.name,
                    origin.editor_id,
                    o.id()
                ));
                referenced_bodies(o)
                    .into_iter()
                    .map(str::to_owned)
                    .chain([o.id().to_owned()])
            })
            .collect();
        let (kept_objects, kept_origins): (Vec<_>, Vec<_>) = objects
            .into_iter()
            .zip(origins)
            .filter(|(o, _)| !broken.contains(o.id()))
            .unzip();
        work.objects = kept_objects;
        work.origins = kept_origins;
        outcome.worn_slots_unhidden += unhide_exposed(&mut work.objects, &work.origins, &results);
        // Существо, чья своя модель не собралась, было бы невидимым: в плагин оно не идёт.
        let failed_models: BTreeMap<String, String> = work
            .creature_models
            .iter()
            .filter_map(|m| match &results.get(&creature_key(&m.mesh))?.result {
                Err(reason) => Some((m.mesh.clone(), reason.clone())),
                Ok(_) => None,
            })
            .collect();
        let plugin = &work.info.name;
        work.creatures
            .retain(|(_, c)| match failed_models.get(&c.mesh) {
                Some(reason) => {
                    outcome
                        .statics_skipped
                        .push(format!("{plugin}: {} ({}): {reason}", c.id, c.mesh));
                    false
                }
                None => true,
            });
    }
    if outcome.worn_slots_unhidden > 0 {
        log(&lang::tr!(
            "meshes: кожа Oblivion убрана из вещей — открыто слотов тела {}",
            "meshes: Oblivion skin removed from items — body slots uncovered {}",
            outcome.worn_slots_unhidden
        ));
    }
    prune_contents(&mut works, &mut outcome);
    let door_pivots = report_doors(&works, &door_info, &mut outcome, log);
    convert_worlds(
        request,
        &mut works,
        &mut ids,
        &door_pivots,
        &mut outcome,
        log,
    )?;
    add_region_sounds(config, &archives, &installed, &mut works, &mut outcome, log)?;
    if !request.only.is_empty() && !outcome.statics_skipped.is_empty() {
        return Err(PipelineError::Unresolved(outcome.statics_skipped));
    }

    let output = &config.output_dir;
    let mut written = write_assets(config, &archives, &results, &works, &mut outcome, log)?;
    write_land_textures(
        config,
        &archives,
        &works,
        &mut written.by_plugin,
        &mut outcome,
        log,
    )?;
    if request.lua_map {
        write_world_maps(
            config,
            &archives,
            &works,
            &mut written.by_plugin,
            &mut outcome,
            log,
        )?;
    }
    let mesh_files = written.mesh_files;
    package::write_assets(output, &written.by_plugin).map_err(failed("output"))?;
    silence_missing_sounds(&mut works, &written.sound_files);
    silence_missing_voices(&mut works, &written.voice_files);

    // write: плагины
    let master = config.target_data.join(TARGET_MASTER);
    let master_size = fs::metadata(&master)
        .map_err(failed(&master.display().to_string()))?
        .len();
    let plugin_paths = write_plugins(output, master_size, &works, &mut outcome)?;
    for p in &outcome.plugins {
        log(&lang::tr!(
            "write: {} — STAT {}, WEAP {}, ARMO {}, CLOT {}, BODY {}, предметов {}, объектов мира {}, LEVI {}, GLOB {}, CREA {}, LEVC {}, ENCH {}, SPEL {}{}",
            "write: {} — STAT {}, WEAP {}, ARMO {}, CLOT {}, BODY {}, items {}, world objects {}, LEVI {}, GLOB {}, CREA {}, LEVC {}, ENCH {}, SPEL {}{}",
            p.name,
            p.statics,
            p.weapons,
            p.armor,
            p.clothing,
            p.bodies,
            p.items,
            p.world,
            p.leveled,
            p.globals,
            p.creatures,
            p.creature_lists,
            p.enchantments,
            p.spells,
            if p.masters.is_empty() {
                String::new()
            } else {
                lang::tr!(", мастера: {}", ", masters: {}", p.masters.join(", "))
            }
        ));
    }
    log(&lang::tr!(
        "write: {} плагинов, STAT {}, WEAP {}, ARMO {}, CLOT {}, BODY {}, предметов {}, объектов мира {}, LEVI {}, GLOB {}, CREA {}, LEVC {}, ENCH {}, SPEL {}, {} мешей, {} текстур, {} иконок, {} картинок книг; камней душ → ванильные: {}",
        "write: {} plugins, STAT {}, WEAP {}, ARMO {}, CLOT {}, BODY {}, items {}, world objects {}, LEVI {}, GLOB {}, CREA {}, LEVC {}, ENCH {}, SPEL {}, {} meshes, {} textures, {} icons, {} book images; soul gems → vanilla: {}",
        outcome.plugins.len(),
        outcome.statics_written,
        outcome.weapons_written,
        outcome.armor_written,
        outcome.clothing_written,
        outcome.bodies_written,
        outcome.items_written,
        outcome.world_written,
        outcome.leveled_written,
        outcome.globals_written,
        outcome.creatures_written,
        outcome.creature_lists_written,
        outcome.enchantments_written,
        outcome.spells_written,
        outcome.meshes_written,
        outcome.textures_copied,
        outcome.icons_copied,
        outcome.book_art_copied,
        outcome.soul_gems_mapped
    ));
    if outcome.textures_generated > 0 {
        log(&lang::tr!(
            "write: запечено текстур кожи лиц {}",
            "write: face skin textures baked {}",
            outcome.textures_generated
        ));
    }
    for missing in &outcome.textures_missing {
        log(&lang::tr!(
            "предупреждение: текстуры нет в архиве: {missing}",
            "warning: texture not in the archive: {missing}"
        ));
    }
    for missing in &outcome.icons_missing {
        log(&lang::tr!(
            "предупреждение: иконки нет в архиве: {missing}",
            "warning: icon not in the archive: {missing}"
        ));
    }
    if outcome.sounds_written > 0 {
        log(&lang::tr!(
            "write: SOUN {}, файлов звуков {} (из стерео в моно {})",
            "write: SOUN {}, sound files {} (stereo to mono {})",
            outcome.sounds_written,
            outcome.sound_files_copied,
            outcome.sound_files_mixed
        ));
    }
    if outcome.voice_files_written > 0 {
        log(&lang::tr!(
            "write: файлов голоса {}",
            "write: voice files {}",
            outcome.voice_files_written
        ));
    }
    for missing in &outcome.sound_files_missing {
        log(&lang::tr!(
            "предупреждение: файла звука нет в архиве: {missing}",
            "warning: sound file not in the archive: {missing}"
        ));
    }
    for missing in &outcome.book_art_missing {
        log(&lang::tr!(
            "предупреждение: картинки книги нет в архиве: {missing}",
            "warning: book image not in the archive: {missing}"
        ));
    }

    let Some((esmtool, niftest)) = tools else {
        return Ok(outcome);
    };
    validate_output(
        &esmtool,
        &niftest,
        &plugin_paths,
        mesh_files,
        &mut outcome,
        log,
    )?;
    Ok(outcome)
}

/// Лица NPC (фаза 8): распаковать части голов и причёски, запечь, добавить меши в общий список
/// стадии мешей, а записи `BODY` — к объектам плагина. Не собрался скелет Morrowind — NPC остаются
/// с ванильными головами (фаза 7), с замечанием.
fn bake_faces(
    config: &Config,
    archives: &[PathBuf],
    works: &mut [PluginWork<'_>],
    results: &mut BTreeMap<String, MeshEntry>,
    registry: &mut TextureRegistry,
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> Result<(), PipelineError> {
    let wanted: Vec<String> = works
        .iter()
        .filter_map(|w| w.actors.as_ref())
        .flat_map(crate::faces::wanted_files)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if wanted.is_empty() {
        return Ok(());
    }
    let rigs = match crate::faces::load_rigs(&config.target_data) {
        Ok(r) => r,
        Err(e) => {
            outcome.problems.push(lang::tr!(
                "лица NPC не запечены — скелет Morrowind не собран ({e}); у NPC ванильные головы", "NPC faces not baked — the Morrowind skeleton could not be built ({e}); NPCs keep vanilla heads"
            ));
            return Ok(());
        }
    };
    let extracted = extract_sources(config, archives, &wanted)
        .map_err(failed(lang::tr("extract (лица)", "extract (faces)")))?;
    log(&lang::tr!(
        "extract: лица — распаковано {}, пропущено (кеш) {}, нет в архиве {} (.egm/.tri/.egt есть не у всех частей)",
        "extract: faces — unpacked {}, skipped (cache) {}, not in the archive {} (not every part has .egm/.tri/.egt)",
        extracted.extracted,
        extracted.skipped,
        extracted.missing.len()
    ));
    for work in works.iter_mut() {
        let Some(actors) = &mut work.actors else {
            continue;
        };
        let baked = crate::faces::bake(&config.work_dir, actors, &rigs, registry);
        let meshes = baked.meshes.iter().filter(|m| m.result.is_ok()).count();
        log(&lang::tr!(
            "faces: {} — голов {}, причёсок {}, мешей {meshes} (одинаковые лица делят меш), не запеклось {}",
            "faces: {} — heads {}, hair {}, meshes {meshes} (identical faces share a mesh), failed to bake {}",
            work.info.name,
            baked.heads,
            baked.hairs,
            baked.failed.len()
        ));
        if !baked.failed.is_empty() {
            outcome.problems.push(lang::tr!(
                "{}: лиц не запеклось {} — у них ванильные голова или причёска; например: {}",
                "{}: faces failed to bake {} — they keep a vanilla head or hair; e.g. {}",
                work.info.name,
                baked.failed.len(),
                baked
                    .failed
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        for m in baked.meshes {
            results.insert(
                m.key,
                MeshEntry {
                    rel: m.rel,
                    source: m.source,
                    result: m.result,
                },
            );
        }
        for (object, origin) in baked.parts {
            work.objects.push(object);
            work.origins.push(origin);
        }
    }
    Ok(())
}

/// Ключ меша своей модели существа в общем списке стадии мешей — по пути результата, как у мешей
/// на теле и деревьев.
fn creature_key(rel: &str) -> String {
    format!("@{rel}")
}

/// Свои модели существ (T11.5e): распаковать скелеты, части тела и клипы папок, собрать по три
/// файла на модель (`<имя>.nif`, `x<имя>.nif`, `x<имя>.kf`) и добавить их в общий список стадии
/// мешей. Модель, которая не собралась, остаётся в списке с причиной: существа с ней в плагин не
/// попадут.
fn build_creature_models(
    config: &Config,
    archives: &[PathBuf],
    works: &[PluginWork<'_>],
    results: &mut BTreeMap<String, MeshEntry>,
    registry: &mut TextureRegistry,
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> Result<(), PipelineError> {
    let models: BTreeMap<&str, &transform::creatures::CreatureModel> = works
        .iter()
        .flat_map(|w| &w.creature_models)
        .map(|m| (m.mesh.as_str(), m))
        .collect();
    if models.is_empty() {
        return Ok(());
    }
    // Клипы — все `.kf` прямо в папке модели: какие из них нужны, решает таблица групп.
    let mut clips: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for model in models.values() {
        if !clips.contains_key(model.folder.as_str()) {
            let files = extract::list_directory(
                &config.source_data,
                archives,
                &crate::creature_models::clips_dir(model),
                "kf",
            )
            .map_err(failed(lang::tr(
                "extract (клипы существ)",
                "extract (creature clips)",
            )))?;
            clips.insert(&model.folder, files);
        }
    }
    let wanted: Vec<String> = models
        .values()
        .flat_map(|m| crate::creature_models::wanted_files(m))
        .chain(clips.values().flatten().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let extracted = extract_sources(config, archives, &wanted).map_err(failed(lang::tr(
        "extract (существа)",
        "extract (creatures)",
    )))?;
    log(&lang::tr!(
        "extract: существа — распаковано {}, пропущено (кеш) {}, нет в архиве {}",
        "extract: creatures — unpacked {}, skipped (cache) {}, not in the archive {}",
        extracted.extracted,
        extracted.skipped,
        extracted.missing.len()
    ));

    let (mut built, mut without_parts) = (0usize, 0usize);
    let mut parts_missing: BTreeSet<String> = BTreeSet::new();
    let mut groups_missing: BTreeMap<String, usize> = BTreeMap::new();
    let mut groups_crowded: BTreeMap<String, usize> = BTreeMap::new();
    for model in models.values() {
        let source = format!("{}|{}", model.skeleton, model.parts.join("|"));
        let folder_clips = clips
            .get(model.folder.as_str())
            .map_or(&[][..], Vec::as_slice);
        match crate::creature_models::build(&config.work_dir, model, folder_clips, registry) {
            Ok(model_files) => {
                built += 1;
                without_parts += usize::from(!model_files.parts_missing.is_empty());
                parts_missing.extend(
                    model_files
                        .parts_missing
                        .iter()
                        .map(|p| format!("{}\\{p}", model.folder)),
                );
                for group in &model_files.groups_missing {
                    *groups_missing.entry(group.clone()).or_default() += 1;
                }
                for group in &model_files.groups_crowded {
                    *groups_crowded.entry(group.clone()).or_default() += 1;
                }
                let (x_nif, x_kf) = (model_files.x_nif, model_files.x_kf);
                for (rel, mark, result) in [
                    (model.mesh.clone(), "creature", model_files.full),
                    (x_nif.0, "creature-x", x_nif.1),
                    (x_kf.0, "creature-kf", x_kf.1),
                ] {
                    results.insert(
                        creature_key(&rel),
                        MeshEntry {
                            rel,
                            source: format!("{source}#{mark}"),
                            result: Ok(result),
                        },
                    );
                }
            }
            Err(reason) => {
                results.insert(
                    creature_key(&model.mesh),
                    MeshEntry {
                        rel: model.mesh.clone(),
                        source: format!("{source}#creature"),
                        result: Err(reason),
                    },
                );
            }
        }
    }
    log(&lang::tr!(
        "creatures: своих моделей собрано {built} из {} (по три файла: <имя>.nif, x<имя>.nif, x<имя>.kf)",
        "creatures: own models built {built} of {} (three files each: <name>.nif, x<name>.nif, x<name>.kf)",
        models.len()
    ));
    if !parts_missing.is_empty() {
        outcome.problems.push(lang::tr!(
            "модели существ: {without_parts} собрано без частей тела, которых нет в архивах: {}",
            "creature models: {without_parts} built without body parts missing from the archives: {}",
            parts_missing.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if !groups_missing.is_empty() {
        outcome.problems.push(lang::tr!(
            "модели существ: в папке нет клипа для групп анимации (группа — моделей): {}",
            "creature models: the folder has no clip for animation groups (group — models): {}",
            groups_missing
                .iter()
                .map(|(group, n)| format!("{group} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !groups_crowded.is_empty() {
        outcome.problems.push(lang::tr!(
            "модели существ: группы анимации не записаны — на отрезке их клипа уже {} групп, больше движок разом не держит (группа — моделей): {}",
            "creature models: animation groups not written — their clip's stretch already holds {} groups, the engine keeps no more open at once (group — models): {}",
            mesh_convert::creature::tape::MAX_OPEN_GROUPS,
            groups_crowded
                .iter()
                .map(|(group, n)| format!("{group} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(())
}

/// Записанные ассеты: меши для `niftest` и принадлежность плагинам для BSA.
struct WrittenAssets {
    mesh_files: Vec<PathBuf>,
    by_plugin: PluginAssets,
    /// Пути звуков в плагине (относительно `Sound\`), чей файл записан.
    sound_files: BTreeSet<String>,
    /// Файлы голоса (`SNAM`, T10.8), которые записаны.
    voice_files: BTreeSet<String>,
}

/// `SOUN` без файла (в `Oblivion.esm` таких 33 — файла нет ни в архивах, ни свободным) не
/// пишется, а двери и свет, что на него ссылались, остаются без звука: ссылка на несуществующий
/// файл — ошибка движка при каждом открытии двери. Скрипт петли такого звука (T10.3) тоже не
/// пишется; активаторы и маркеры остаются без скрипта (на маркер уже ссылаются ячейки).
fn silence_missing_sounds(works: &mut [PluginWork<'_>], written: &BTreeSet<String>) {
    for work in works {
        let mut silent: BTreeSet<String> = BTreeSet::new();
        let mut kept = Vec::with_capacity(work.sounds.len());
        let mut kept_origins = Vec::with_capacity(work.sounds.len());
        for (sound, origin) in work.sounds.drain(..).zip(work.sound_origins.drain(..)) {
            if written.contains(&sound.file) {
                kept.push(sound);
                kept_origins.push(origin);
            } else {
                silent.insert(sound.id.to_ascii_lowercase());
            }
        }
        work.sounds = kept;
        work.sound_origins = kept_origins;
        if silent.is_empty() {
            continue;
        }
        work.sound_gens
            .retain(|g| !silent.contains(&g.sound.to_ascii_lowercase()));
        let clear = |id: &mut String| {
            if silent.contains(&id.to_ascii_lowercase()) {
                id.clear();
            }
        };
        for object in &mut work.objects {
            match object {
                Object::Door(d) => {
                    clear(&mut d.open_sound);
                    clear(&mut d.close_sound);
                }
                Object::Light(l) => clear(&mut l.sound),
                _ => {}
            }
        }
        let mut dropped: BTreeSet<String> = BTreeSet::new();
        work.scripts.retain(|script| match script {
            Script::LoopSound { id, sound } if silent.contains(&sound.to_ascii_lowercase()) => {
                dropped.insert(id.to_ascii_lowercase());
                false
            }
            _ => true,
        });
        let activators = work
            .objects
            .iter_mut()
            .filter_map(|o| match o {
                Object::Activator(a) => Some(a),
                _ => None,
            })
            .chain(&mut work.sound_markers);
        for a in activators {
            if dropped.contains(&a.script.to_ascii_lowercase()) {
                a.script.clear();
            }
        }
        let regions = work
            .world
            .iter_mut()
            .filter_map(|w| w.converted.as_mut())
            .flat_map(|c| &mut c.regions);
        for region in regions {
            region
                .sounds
                .retain(|(id, _)| !silent.contains(&id.to_ascii_lowercase()));
        }
    }
}

/// Пишет меши, текстуры и иконки в общие папки `output/` и считает, какой плагин что использует.
fn write_assets(
    config: &Config,
    archives: &[PathBuf],
    results: &BTreeMap<String, MeshEntry>,
    works: &[PluginWork<'_>],
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> Result<WrittenAssets, PipelineError> {
    let output = &config.output_dir;
    let mut mesh_files: Vec<PathBuf> = Vec::new();
    // Для каждого меша — его путь в `Data Files` и имена его текстур.
    let mut mesh_assets: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    let mut textures: BTreeMap<String, String> = BTreeMap::new(); // имя → путь в архиве
    let mut generated: BTreeMap<&str, &Vec<u8>> = BTreeMap::new(); // имя → байты
    let mut short_paths: BTreeMap<String, &str> = BTreeMap::new();
    for (key, entry) in results {
        let Ok(result) = &entry.result else { continue };
        let rel = entry.rel.clone();
        claim_short_path(&mut short_paths, rel.clone(), &entry.source)?;
        let file = join_backslash(&output.join("Meshes"), &rel);
        write_file(&file, &result.bytes)?;
        mesh_files.push(file);
        for t in &result.textures {
            textures.insert(t.target_name.clone(), t.source_path.clone());
        }
        for (name, bytes) in &result.generated {
            generated.entry(name.as_str()).or_insert(bytes);
        }
        let names = result
            .textures
            .iter()
            .map(|t| t.target_name.clone())
            .chain(result.generated.iter().map(|(name, _)| name.clone()))
            .collect();
        mesh_assets.insert(key.clone(), (asset_relative("Meshes", &rel), names));
    }
    outcome.meshes_written = mesh_files.len();

    // extract + write: текстуры
    let wanted: Vec<String> = textures.values().cloned().collect();
    outcome.extract_textures = extract_sources(config, archives, &wanted)
        .map_err(failed(lang::tr("extract (текстуры)", "extract (textures)")))?;
    log(&lang::tr!(
        "extract: текстуры — распаковано {}, пропущено (кеш) {}, нет в архиве {}",
        "extract: textures — unpacked {}, skipped (cache) {}, not in the archive {}",
        outcome.extract_textures.extracted,
        outcome.extract_textures.skipped,
        outcome.extract_textures.missing.len()
    ));
    let textures_dir = output.join("Textures");
    let mut copied_textures: BTreeSet<&str> = BTreeSet::new();
    for (target, source_path) in &textures {
        let from = extracted_path(&config.work_dir, source_path).map_err(failed("extract"))?;
        if !from.is_file() {
            outcome.textures_missing.push(source_path.clone());
            continue;
        }
        let to = textures_dir.join(target);
        fs::create_dir_all(&textures_dir).map_err(failed("Textures"))?;
        fs::copy(&from, &to).map_err(failed(&to.display().to_string()))?;
        copied_textures.insert(target);
        outcome.textures_copied += 1;
    }
    // Сделанные конвертером (кожа лица): имена `obface_…` не пересекаются с копиями `ob_…`.
    for (name, bytes) in &generated {
        fs::create_dir_all(&textures_dir).map_err(failed("Textures"))?;
        let to = textures_dir.join(name);
        fs::write(&to, bytes).map_err(failed(&to.display().to_string()))?;
        copied_textures.insert(name);
        outcome.textures_generated += 1;
    }

    // extract + write: иконки оружия
    let icons: BTreeSet<&str> = works
        .iter()
        .flat_map(|w| &w.origins)
        .filter_map(|origin| origin.icon.as_deref())
        .collect();
    let wanted: Vec<String> = icons
        .iter()
        .map(|i| format!("textures\\menus\\icons\\{i}"))
        .collect();
    let extracted = extract_sources(config, archives, &wanted)
        .map_err(failed(lang::tr("extract (иконки)", "extract (icons)")))?;
    log(&lang::tr!(
        "extract: иконки — распаковано {}, пропущено (кеш) {}, нет в архиве {}",
        "extract: icons — unpacked {}, skipped (cache) {}, not in the archive {}",
        extracted.extracted,
        extracted.skipped,
        extracted.missing.len()
    ));
    let mut icon_paths: BTreeMap<String, &str> = BTreeMap::new();
    let mut icon_assets: BTreeMap<&str, String> = BTreeMap::new();
    for (icon, archive_path) in icons.iter().zip(&wanted) {
        let rel = icon_path(icon).ok_or_else(|| {
            PipelineError::Failed(lang::tr!(
                "путь иконки {icon:?} не представим в выходе",
                "icon path {icon:?} cannot be represented in the output"
            ))
        })?;
        claim_short_path(&mut icon_paths, rel.clone(), icon)?;
        let from = extracted_path(&config.work_dir, archive_path).map_err(failed("extract"))?;
        if !from.is_file() {
            outcome.icons_missing.push(archive_path.clone());
            continue;
        }
        let mut bytes = fs::read(&from).map_err(failed(&from.display().to_string()))?;
        // Иконка Oblivion 64×64 в инвентаре Morrowind вдвое крупнее ванильных 32×32.
        match mesh_convert::icons::shrink_icon(&bytes) {
            Ok(Some(small)) => bytes = small,
            Ok(None) => {}
            Err(e) => outcome.problems.push(lang::tr!(
                "иконка {archive_path} скопирована как есть: {e}",
                "icon {archive_path} copied as is: {e}"
            )),
        }
        write_file(&join_backslash(&output.join("Icons"), &rel), &bytes)?;
        icon_assets.insert(icon, asset_relative("Icons", &rel));
        outcome.icons_copied += 1;
    }

    // extract + write: картинки книг. `src` в тексте Oblivion — относительно `Textures\Menus\`;
    // одно и то же имя, записанное по-разному, сводится по нормальной форме.
    let mut book_art: BTreeMap<String, String> = BTreeMap::new();
    for src in works
        .iter()
        .flat_map(|w| &w.origins)
        .flat_map(|o| &o.book_art)
    {
        let (Some(rel), Some(normal)) = (book_art_path(src), transform::paths::normalize(src))
        else {
            continue;
        };
        match book_art.get(&rel) {
            Some(other) if *other != normal => {
                return Err(PipelineError::Failed(lang::tr!(
                    "имя картинки {rel} совпало у {other:?} и {normal:?}",
                    "image name {rel} is shared by {other:?} and {normal:?}"
                )));
            }
            Some(_) => {}
            None => {
                book_art.insert(rel, normal);
            }
        }
    }
    let wanted: Vec<String> = book_art
        .values()
        .map(|src| format!("textures\\menus\\{src}"))
        .collect();
    let extracted = extract_sources(config, archives, &wanted).map_err(failed(lang::tr(
        "extract (картинки книг)",
        "extract (book images)",
    )))?;
    log(&lang::tr!(
        "extract: картинки книг — распаковано {}, пропущено (кеш) {}, нет в архиве {}",
        "extract: book images — unpacked {}, skipped (cache) {}, not in the archive {}",
        extracted.extracted,
        extracted.skipped,
        extracted.missing.len()
    ));
    let mut book_art_assets: BTreeMap<&str, String> = BTreeMap::new();
    for ((rel, src), archive_path) in book_art.iter().zip(&wanted) {
        let from = extracted_path(&config.work_dir, archive_path).map_err(failed("extract"))?;
        if !from.is_file() {
            outcome.book_art_missing.push(archive_path.clone());
            continue;
        }
        let to = output.join("BookArt").join(rel);
        fs::create_dir_all(output.join("BookArt")).map_err(failed("BookArt"))?;
        fs::copy(&from, &to).map_err(failed(&to.display().to_string()))?;
        book_art_assets.insert(src, asset_relative("BookArt", rel));
        outcome.book_art_copied += 1;
    }

    let sound_assets = write_sounds(config, archives, works, outcome, log)?;
    let voice_assets = write_voices(config, works, outcome, log)?;

    // Какой плагин что использует.
    let mut by_plugin = PluginAssets::new();
    for work in works {
        let set = by_plugin.entry(work.output_name()).or_default();
        for origin in &work.origins {
            if let Some((mesh, names)) = mesh_assets.get(&mesh_key(origin)) {
                set.insert(mesh.clone());
                for name in names
                    .iter()
                    .filter(|n| copied_textures.contains(n.as_str()))
                {
                    set.insert(format!("Textures/{name}"));
                }
            }
            if let Some(icon) = origin.icon.as_deref().and_then(|i| icon_assets.get(i)) {
                set.insert(icon.clone());
            }
            for art in origin
                .book_art
                .iter()
                .filter_map(|src| transform::paths::normalize(src))
                .filter_map(|src| book_art_assets.get(src.as_str()))
            {
                set.insert(art.clone());
            }
        }
        // Свои модели существ: три файла и текстуры тела.
        for model in &work.creature_models {
            let Some((x_nif, x_kf)) = transform::paths::creature_pair(&model.mesh) else {
                continue;
            };
            for rel in [&model.mesh, &x_nif, &x_kf] {
                if let Some((mesh, names)) = mesh_assets.get(&creature_key(rel)) {
                    set.insert(mesh.clone());
                    for name in names
                        .iter()
                        .filter(|n| copied_textures.contains(n.as_str()))
                    {
                        set.insert(format!("Textures/{name}"));
                    }
                }
            }
        }
        for sound in &work.sounds {
            if let Some(asset) = sound_assets.get(&sound.file) {
                set.insert(asset.clone());
            }
        }
        for voice in work.voices.keys() {
            if let Some(asset) = voice_assets.get(voice) {
                set.insert(asset.clone());
            }
        }
    }
    Ok(WrittenAssets {
        mesh_files,
        by_plugin,
        sound_files: sound_assets.into_keys().collect(),
        voice_files: voice_assets.into_keys().collect(),
    })
}

/// Файлы голоса реплик (T10.8) в `output/Sound`: mp3 ответов из архивов голосов склеиваются
/// кадрами (`mesh_convert::audio::mp3`) в один файл на реплику. Путь в плагине → путь ассета.
/// Файл, который не собрался (нет в архиве, не тот формат mp3), — замечание; реплики с ним
/// выбрасывает [`silence_missing_voices`].
fn write_voices(
    config: &Config,
    works: &[PluginWork<'_>],
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> Result<BTreeMap<String, String>, PipelineError> {
    // Путь в плагине → источники. Один путь у разных склеек — столкновение хеша.
    let mut wanted: BTreeMap<&str, &Vec<String>> = BTreeMap::new();
    for work in works {
        for (rel, sources) in &work.voices {
            match wanted.get(rel.as_str()) {
                Some(other) if *other != sources => {
                    return Err(PipelineError::Failed(lang::tr!(
                        "путь голоса {rel} совпал у {other:?} и {sources:?}",
                        "voice path {rel} is shared by {other:?} and {sources:?}"
                    )));
                }
                Some(_) => {}
                None => {
                    wanted.insert(rel, sources);
                }
            }
        }
    }
    if wanted.is_empty() {
        return Ok(BTreeMap::new());
    }
    let archives = voice_archives(&config.source_data);
    let paths: Vec<String> = wanted
        .values()
        .flat_map(|s| s.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let extracted = extract_sources(config, &archives, &paths)
        .map_err(failed(lang::tr("extract (голоса)", "extract (voices)")))?;
    log(&lang::tr!(
        "extract: голоса — распаковано {}, пропущено (кеш) {}, нет в архиве {}",
        "extract: voices — unpacked {}, skipped (cache) {}, not in the archive {}",
        extracted.extracted,
        extracted.skipped,
        extracted.missing.len()
    ));
    let mut assets = BTreeMap::new();
    for (rel, sources) in wanted {
        let mut parts = Vec::with_capacity(sources.len());
        for s in sources {
            let from = extracted_path(&config.work_dir, s).map_err(failed("extract"))?;
            match fs::read(&from) {
                Ok(bytes) => parts.push(bytes),
                Err(_) => break,
            }
        }
        if parts.len() < sources.len() {
            outcome.problems.push(lang::tr!(
                "голос {rel}: нет файла ответа в архивах, реплика без голоса выброшена", "voice {rel}: the response file is not in the archives, the voiceless line is dropped"
            ));
            continue;
        }
        let joined = match mesh_convert::audio::mp3::join_voice(parts.iter().map(Vec::as_slice)) {
            Ok(j) => j,
            Err(e) => {
                outcome.problems.push(lang::tr!(
                    "голос {rel} ({}): {e}, реплика без голоса выброшена",
                    "voice {rel} ({}): {e}, the voiceless line is dropped",
                    sources.join(", ")
                ));
                continue;
            }
        };
        write_file(
            &join_backslash(&config.output_dir.join("Sound"), rel),
            &joined,
        )?;
        assets.insert(rel.to_owned(), asset_relative("Sound", rel));
        outcome.voice_files_written += 1;
    }
    Ok(assets)
}

/// Реплика голоса без файла (не собрался в [`write_voices`]) не пишется: голос без звука движок
/// не показывает. Тема, у которой не осталось реплик, — тоже.
fn silence_missing_voices(works: &mut [PluginWork<'_>], written: &BTreeSet<String>) {
    for work in works {
        for topic in &mut work.topics {
            topic
                .infos
                .retain(|i| i.sound.is_empty() || written.contains(&i.sound));
        }
        work.topics.retain(|t| !t.infos.is_empty());
    }
}

/// Файлы звуков (T10.1) в `output/Sound`: путь в плагине → путь ассета. `SOUN` с каталогом
/// получает первый `.wav` каталога по имени; стерео сводится в моно (`mesh_convert::audio`).
fn write_sounds(
    config: &Config,
    archives: &[PathBuf],
    works: &[PluginWork<'_>],
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> Result<BTreeMap<String, String>, PipelineError> {
    // Путь в плагине → источник. Один путь у двух разных источников — столкновение хеша.
    let mut sources: BTreeMap<&str, &SoundOrigin> = BTreeMap::new();
    for work in works {
        for (sound, origin) in work.sounds.iter().zip(&work.sound_origins) {
            match sources.get(sound.file.as_str()) {
                Some(other) if other.source != origin.source => {
                    return Err(PipelineError::Failed(lang::tr!(
                        "путь звука {} совпал у {:?} и {:?}",
                        "sound path {} is shared by {:?} and {:?}",
                        sound.file,
                        other.source,
                        origin.source
                    )));
                }
                Some(_) => {}
                None => {
                    sources.insert(&sound.file, origin);
                }
            }
        }
    }
    let mut wanted: Vec<(&str, String)> = Vec::new();
    for (&rel, origin) in &sources {
        let archive_path = if origin.directory {
            let dir = format!("sound\\{}", origin.source);
            let files = extract::list_directory(&config.source_data, archives, &dir, "wav")
                .map_err(failed(lang::tr(
                    "extract (каталоги звуков)",
                    "extract (sound folders)",
                )))?;
            let Some(first) = files.into_iter().next() else {
                outcome.sound_files_missing.push(format!("{dir}\\"));
                continue;
            };
            first
        } else {
            format!("sound\\{}", origin.source)
        };
        wanted.push((rel, archive_path));
    }
    let paths: Vec<String> = wanted.iter().map(|(_, p)| p.clone()).collect();
    let extracted = extract_sources(config, archives, &paths)
        .map_err(failed(lang::tr("extract (звуки)", "extract (sounds)")))?;
    log(&lang::tr!(
        "extract: звуки — распаковано {}, пропущено (кеш) {}, нет в архиве {}",
        "extract: sounds — unpacked {}, skipped (cache) {}, not in the archive {}",
        extracted.extracted,
        extracted.skipped,
        extracted.missing.len()
    ));
    let mut assets = BTreeMap::new();
    for (rel, archive_path) in wanted {
        let from = extracted_path(&config.work_dir, &archive_path).map_err(failed("extract"))?;
        if !from.is_file() {
            outcome.sound_files_missing.push(archive_path);
            continue;
        }
        let bytes = fs::read(&from).map_err(failed(&from.display().to_string()))?;
        let bytes = match mesh_convert::audio::to_mono(&bytes) {
            Ok(Some(mono)) => {
                outcome.sound_files_mixed += 1;
                mono
            }
            Ok(None) => bytes,
            Err(e) => {
                outcome.problems.push(lang::tr!(
                    "звук {archive_path} скопирован как есть: {e}",
                    "sound {archive_path} copied as is: {e}"
                ));
                bytes
            }
        };
        write_file(
            &join_backslash(&config.output_dir.join("Sound"), rel),
            &bytes,
        )?;
        assets.insert(rel.to_owned(), asset_relative("Sound", rel));
        outcome.sound_files_copied += 1;
    }
    Ok(assets)
}

/// Пишет по плагину Morrowind на каждый плагин источника; возвращает пути записанного.
fn write_plugins(
    output: &Path,
    master_size: u64,
    works: &[PluginWork<'_>],
    outcome: &mut Outcome,
) -> Result<Vec<PathBuf>, PipelineError> {
    fs::create_dir_all(output).map_err(failed("output"))?;
    // Размеры уже записанных плагинов: мастер пишется раньше зависимого (порядок загрузки).
    let mut sizes: BTreeMap<String, (String, u64)> = BTreeMap::new();
    let mut paths = Vec::new();
    for work in works {
        let name = &work.output_name();
        let mut builder = PluginBuilder::new("back2mw", "Converted from Oblivion by back2mw")
            .with_master(TARGET_MASTER, master_size);
        if work.info.kind == plugins::Kind::Esm && work.world.is_none() {
            builder = builder.as_master();
        }
        let mut masters = Vec::new();
        for master in &work.info.masters {
            if let Some((real, size)) = sizes.get(&master.to_ascii_lowercase()) {
                builder = builder.with_master(real, *size);
                masters.push(real.clone());
            }
        }
        let mut result = PluginOutcome {
            name: name.clone(),
            masters,
            start: work
                .world
                .as_ref()
                .and_then(|w| start_hint(&w.source.editor_id, w.converted.as_ref()?)),
            ..PluginOutcome::default()
        };
        // `GLOB` — в начале, как в `Morrowind.esm` (после `GMST`, которых мы не пишем).
        for g in &work.globals {
            builder.add_global(g.clone());
        }
        result.globals = work.globals.len();
        // `SOUN` — раньше дверей и света, как в `Morrowind.esm`.
        for s in &work.sounds {
            builder.add_sound(s.clone());
        }
        result.sounds = work.sounds.len();
        // `SNDG` — сразу за `SOUN`, на которые ссылаются.
        for g in &work.sound_gens {
            builder.add_sound_gen(g.clone());
        }
        for script in &work.scripts {
            builder
                .add_script(script)
                .map_err(failed(&format!("{name}: SCPT {}", script.id())))?;
        }
        for object in &work.objects {
            match object {
                Object::Static(_) => result.statics += 1,
                Object::Weapon(_) => result.weapons += 1,
                Object::Armor(_) => result.armor += 1,
                Object::Clothing(_) => result.clothing += 1,
                Object::BodyPart(_) => result.bodies += 1,
                Object::Item(_) => result.items += 1,
                Object::Activator(_)
                | Object::Door(_)
                | Object::Container(_)
                | Object::Light(_) => {
                    result.world += 1;
                }
            }
            builder.add(object.clone());
        }
        for marker in &work.sound_markers {
            builder.add(Object::Activator(marker.clone()));
        }
        result.world += work.sound_markers.len();
        for list in &work.leveled {
            builder.add_leveled_item(list.clone());
        }
        result.leveled = work.leveled.len();
        // ENCH раньше вещей не обязательно: движок разрешает ссылки после загрузки всех записей.
        for e in &work.enchantments {
            builder.add_enchantment(e.clone());
        }
        for s in &work.spells {
            builder.add_spell(s.clone());
        }
        result.enchantments = work.enchantments.len();
        result.spells = work.spells.len();
        // NPC и то, на что они ссылаются, — до ячеек, как в `Morrowind.esm`.
        if let Some(actors) = &work.actors {
            for f in &actors.factions {
                builder.add_faction(f.clone());
            }
            for c in &actors.classes {
                builder.add_class(c.clone());
            }
            for r in &actors.races {
                builder.add_race(r.clone());
            }
            for b in &actors.bodies {
                builder.add(b.clone());
            }
            // Части тела своих рас — тоже `BODY`: их считает сверка с `esmtool`.
            result.bodies += actors.bodies.len();
            for (_, n) in &actors.npcs {
                builder.add_npc(n.clone());
            }
            result.npcs = actors.npcs.len();
        }
        for (_, c) in &work.creatures {
            builder.add_creature(c.clone());
        }
        result.creatures = work.creatures.len();
        // `LEVC` — после существ и NPC, до ячеек, как в `Morrowind.esm`.
        for (_, l) in &work.creature_lists {
            builder.add_leveled_creature(l.clone());
        }
        result.creature_lists = work.creature_lists.len();
        // Порядок как в `Morrowind.esm`: LTEX и REGN раньше ячеек, LAND и PGRD после.
        if let Some(world) = work.world.as_ref().and_then(|w| w.converted.as_ref()) {
            for t in &world.land_textures {
                builder.add_land_texture(t.clone());
            }
            for r in &world.regions {
                builder.add_region(r.clone());
            }
            for c in &world.cells {
                builder.add_cell(c.clone());
            }
            for c in &world.interiors {
                builder.add_interior(c.clone());
            }
            for l in &world.lands {
                builder.add_land(l.clone());
            }
            for g in &world.path_grids {
                builder.add_path_grid(g.clone());
            }
            result.cells = world.cells.len();
            result.lands = world.lands.len();
            result.interiors = world.interiors.len();
        }
        // Темы — последними, как в `Morrowind.esm`.
        for t in &work.topics {
            result.topics += 1;
            result.infos += t.infos.len();
            builder
                .add_topic(t.clone())
                .map_err(failed(&format!("{name}: DIAL {}", t.name)))?;
        }
        let path = output.join(name);
        builder
            .save(&path)
            .map_err(failed(&path.display().to_string()))?;
        let size = fs::metadata(&path)
            .map_err(failed(&path.display().to_string()))?
            .len();
        // Ключ — имя источника: по нему зависимые плагины ищут мастера.
        sizes.insert(work.info.name.to_ascii_lowercase(), (name.clone(), size));
        outcome.statics_written += result.statics;
        outcome.weapons_written += result.weapons;
        outcome.armor_written += result.armor;
        outcome.clothing_written += result.clothing;
        outcome.bodies_written += result.bodies;
        outcome.items_written += result.items;
        outcome.world_written += result.world;
        outcome.leveled_written += result.leveled;
        outcome.globals_written += result.globals;
        outcome.creatures_written += result.creatures;
        outcome.creature_lists_written += result.creature_lists;
        outcome.enchantments_written += result.enchantments;
        outcome.spells_written += result.spells;
        outcome.sounds_written += result.sounds;
        outcome.plugins.push(result);
        paths.push(path);
    }
    Ok(paths)
}

/// `esmtool` по каждому плагину, `niftest` по всем мешам. Любой брак валит прогон.
fn validate_output(
    esmtool: &Path,
    niftest: &Path,
    plugin_paths: &[PathBuf],
    mut mesh_files: Vec<PathBuf>,
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> Result<(), PipelineError> {
    let mut problems: Vec<String> = Vec::new();
    for (path, expected) in plugin_paths.iter().zip(outcome.plugins.clone()) {
        let check = validate::check_plugin(esmtool, path).map_err(failed("validate"))?;
        outcome.plugin_stat_records += check.stat_records;
        outcome.plugin_weapon_records += check.weapon_records;
        if !check.ok {
            problems.push(lang::tr!(
                "esmtool отверг {}: {}",
                "esmtool rejected {}: {}",
                expected.name,
                check.message
            ));
        } else {
            let counted = [
                check.stat_records,
                check.weapon_records,
                check.armor_records,
                check.clothing_records,
                check.body_records,
                check.item_records,
                check.world_records,
                check.npc_records,
                check.topic_records,
                check.info_records,
            ];
            let written = [
                expected.statics,
                expected.weapons,
                expected.armor,
                expected.clothing,
                expected.bodies,
                expected.items,
                expected.world,
                expected.npcs,
                expected.topics,
                expected.infos,
            ];
            if counted != written {
                problems.push(lang::tr!(
                    "{}: esmtool насчитал STAT/WEAP/ARMO/CLOT/BODY/предметы/объекты мира/NPC_/DIAL/INFO {counted:?}, записано {written:?}", "{}: esmtool counted STAT/WEAP/ARMO/CLOT/BODY/items/world objects/NPC_/DIAL/INFO {counted:?}, written {written:?}",
                    expected.name
                ));
            }
        }
    }
    mesh_files.sort();
    let failures = validate::check_meshes(niftest, &mesh_files).map_err(failed("validate"))?;
    outcome.meshes_validated = mesh_files.len();
    for f in &failures {
        problems.push(format!("niftest: {} — {}", f.path, f.reason));
    }
    log(&lang::tr!(
        "validate: плагинов {}, мешей проверено {}, брака {}",
        "validate: plugins {}, meshes checked {}, failed {}",
        plugin_paths.len(),
        outcome.meshes_validated,
        problems.len()
    ));
    if problems.is_empty() {
        Ok(())
    } else {
        Err(PipelineError::Validation(problems))
    }
}

/// Занимает короткий путь за исходным файлом. Два разных источника с одним коротким путём —
/// столкновение хеша (`transform::paths`): второй файл молча затёр бы первый, поэтому это
/// отказ прогона с именами обоих.
fn claim_short_path<'a>(
    taken: &mut BTreeMap<String, &'a str>,
    short: String,
    source: &'a str,
) -> Result<(), PipelineError> {
    match taken.get(&short) {
        Some(other) if !other.eq_ignore_ascii_case(source) => {
            Err(PipelineError::Failed(lang::tr!(
                "короткий путь {short} совпал у {other:?} и {source:?}",
                "short path {short} is shared by {other:?} and {source:?}"
            )))
        }
        Some(_) => Ok(()),
        None => {
            taken.insert(short, source);
            Ok(())
        }
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), PipelineError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(failed(&dir.display().to_string()))?;
    }
    fs::write(path, bytes).map_err(failed(&path.display().to_string()))
}

/// Читает распакованный меш, конвертирует, сериализует. Ошибка — текст для отчёта. Системы
/// частиц становятся эмиттерами Morrowind (T2.7); счёт — в `outcome`.
fn convert_one(
    work_dir: &Path,
    archive_path: &str,
    registry: &mut TextureRegistry,
    outcome: &mut Outcome,
) -> Result<MeshResult, String> {
    let path = extracted_path(work_dir, archive_path).map_err(|e| e.to_string())?;
    let bytes = fs::read(&path)
        .map_err(|_| lang::tr("нет в архиве мешей", "not in the mesh archive").to_owned())?;
    let (source, offsets) =
        parse_with_offsets(&bytes).map_err(|e| lang::tr!("чтение: {e}", "read: {e}"))?;
    let particles = mesh_convert::particles::Systems::read(&bytes, &source, &offsets);
    let mut converted = convert_with_particles(&source, registry, &particles)
        .map_err(|e| lang::tr!("конвертация: {e}", "conversion: {e}"))?;
    count_particles(&converted.stats, outcome);
    let out = converted
        .save_bytes()
        .map_err(|e| lang::tr!("запись NIF: {e}", "NIF write: {e}"))?;
    Ok(MeshResult {
        bytes: out,
        textures: converted.textures,
        generated: Vec::new(),
        exposed_slots: BTreeSet::new(),
    })
}

/// Меш двери (T2.6b): тот же перенос, что у прочих мешей, плюс петля. По последовательности
/// `Open` находится створка; если она одна, на вертикальной петле и рамы в меше нет, начало
/// координат меша переносится в петлю — туда, где дверь повернёт Morrowind. Анимация, которая
/// не читается, — не отказ: дверь переносится в позе файла, как раньше.
fn door_one(
    work_dir: &Path,
    archive_path: &str,
    registry: &mut TextureRegistry,
    outcome: &mut Outcome,
) -> (Result<MeshResult, String>, Option<DoorInfo>) {
    use mesh_convert::door;
    let read = || -> Result<Vec<u8>, String> {
        let path = extracted_path(work_dir, archive_path).map_err(|e| e.to_string())?;
        fs::read(&path)
            .map_err(|_| lang::tr("нет в архиве мешей", "not in the mesh archive").to_owned())
    };
    let bytes = match read() {
        Ok(bytes) => bytes,
        Err(e) => return (Err(e), None),
    };
    let (source, offsets) = match parse_with_offsets(&bytes) {
        Ok(parsed) => parsed,
        Err(e) => return (Err(lang::tr!("чтение: {e}", "read: {e}")), None),
    };
    let motion = mesh_convert::read::motion::sequences(&bytes, &source, &offsets)
        .ok()
        .and_then(|sequences| door::door_motion(&source, &sequences));
    let hinge = motion.as_ref().and_then(door::DoorMotion::hinge);
    let info = DoorInfo {
        fate: door::fate(motion.as_ref()),
        pivot: hinge.map(|h| h.to_array()),
        counter_clockwise: motion
            .as_ref()
            .is_some_and(door::DoorMotion::opens_counter_clockwise),
    };
    let particles = mesh_convert::particles::Systems::read(&bytes, &source, &offsets);
    let mut converted = match convert_with_particles(&source, registry, &particles) {
        Ok(converted) => converted,
        Err(e) => {
            return (
                Err(lang::tr!("конвертация: {e}", "conversion: {e}")),
                Some(info),
            );
        }
    };
    count_particles(&converted.stats, outcome);
    if let Some(hinge) = hinge {
        door::rehinge(&mut converted, hinge);
    }
    let result = converted
        .save_bytes()
        .map_err(|e| lang::tr!("запись NIF: {e}", "NIF write: {e}"))
        .map(|out| MeshResult {
            bytes: out,
            textures: converted.textures,
            generated: Vec::new(),
            exposed_slots: BTreeSet::new(),
        });
    (result, Some(info))
}

/// Прибавляет системы частиц одного меша к счёту прогона (T2.7).
fn count_particles(stats: &mesh_convert::geometry::Stats, outcome: &mut Outcome) {
    outcome.particles_converted += stats.converted_particles;
    outcome.particles_lossy += stats.lossy_particles;
    outcome.particles_dropped += stats.dropped_particles;
}

/// Итог по дверям (T2.6): строка журнала, замечание о дверях, которые поворот Morrowind не
/// выражает, и смещения начала координат мешей с перенесённой петлёй — по `FormID` двери.
fn report_doors(
    works: &[PluginWork<'_>],
    door_info: &BTreeMap<String, DoorInfo>,
    outcome: &mut Outcome,
    log: &mut dyn FnMut(&str),
) -> HashMap<u32, [f32; 3]> {
    use mesh_convert::door::DoorFate;
    let mut pivots = HashMap::new();
    for work in works {
        let mut by_fate: BTreeMap<DoorFate, usize> = BTreeMap::new();
        let mut counter = 0usize;
        for origin in work.origins.iter().filter(|o| o.door.is_some()) {
            let Some(info) = door_info.get(&mesh_key(origin)) else {
                continue;
            };
            *by_fate.entry(info.fate).or_default() += 1;
            counter += usize::from(info.counter_clockwise);
            if let Some(pivot) = info.pivot {
                pivots.insert(origin.form_id, pivot);
            }
        }
        let n = |fate: DoorFate| by_fate.get(&fate).copied().unwrap_or(0);
        let total: usize = by_fate.values().sum();
        if total == 0 {
            continue;
        }
        log(&lang::tr!(
            "meshes: {} — дверей {total}: без анимации {}, петля в начале меша {}, петля перенесена в начало меша {}",
            "meshes: {} — doors {total}: without animation {}, hinge at the mesh origin {}, hinge moved to the mesh origin {}",
            work.info.name,
            n(DoorFate::NoOpen) + n(DoorFate::Still),
            n(DoorFate::AtOrigin),
            n(DoorFate::Rehinged)
        ));
        let (framed, leaves, sliding, tilting) = (
            n(DoorFate::Framed),
            n(DoorFate::Leaves),
            n(DoorFate::Sliding),
            n(DoorFate::Tilting),
        );
        if framed + leaves + sliding + tilting > 0 {
            outcome.problems.push(lang::tr!(
                "{}: дверей, которые Morrowind повернёт не так, как они открываются в Oblivion: рама в том же меше {framed}, несколько створок {leaves}, сдвиг (решётки) {sliding}, наклон {tilting}", "{}: doors Morrowind will turn differently from how they open in Oblivion: frame in the same mesh {framed}, several leaves {leaves}, sliding (portcullises) {sliding}, tilting {tilting}",
                work.info.name
            ));
        }
        if counter > 0 {
            outcome.problems.push(lang::tr!(
                "{}: дверей на петле, открывающихся в другую сторону, чем в Oblivion: {counter} — Morrowind поворачивает дверь только по часовой стрелке", "{}: hinged doors that open the other way than in Oblivion: {counter} — Morrowind only turns a door clockwise",
                work.info.name
            ));
        }
    }
    pivots
}

/// Меш дерева (T2.4): кора и листва по `.spt`, как их строит движок при семени записи.
fn tree_one(
    work_dir: &Path,
    spt: &str,
    tree: &transform::TreeSource,
    registry: &mut TextureRegistry,
) -> Result<MeshResult, String> {
    use mesh_convert::speedtree;
    let path = extracted_path(work_dir, spt).map_err(|e| e.to_string())?;
    let bytes =
        fs::read(&path).map_err(|_| lang::tr("нет в архиве", "not in the archive").to_owned())?;
    let source =
        speedtree::parse(&bytes).map_err(|e| lang::tr!("чтение .spt: {e}", ".spt read: {e}"))?;
    let bark = speedtree::branch::generate(&source, tree.seed);
    let cards = speedtree::cards::leaf_cards(&source, &bark.leaves);
    let textures = speedtree::nif::TreeTextures {
        bark: speedtree::nif::bark_texture_path(&source.bark_texture),
        leaves: tree
            .leaves
            .as_deref()
            .map(speedtree::nif::leaf_texture_path),
    };
    let mut built = speedtree::nif::build(&bark, &cards, &textures, registry)
        .map_err(|e| lang::tr!("текстуры: {e}", "textures: {e}"))?;
    let out = built
        .stream
        .save_bytes()
        .map_err(|e| lang::tr!("запись NIF: {e}", "NIF write: {e}"))?;
    Ok(MeshResult {
        bytes: out,
        textures: built.textures,
        generated: Vec::new(),
        exposed_slots: BTreeSet::new(),
    })
}

#[cfg(test)]
mod unhide_tests {
    use super::*;
    use tes3_out::WornPart;

    fn part(slot: u8, male: &str, female: &str) -> WornPart {
        WornPart {
            slot,
            male: male.to_owned(),
            female: female.to_owned(),
        }
    }

    /// Кираса: грудь — часть тела, плечи и бёдра спрятаны. Женский меш убрал кожу с плеч,
    /// мужской — нет: плечи открываются для обоих (лучше тело под тканью, чем дыра), бёдра — нет.
    #[test]
    fn empty_slots_opened_by_either_gender_are_unhidden_and_body_parts_stay() {
        let mut parts = vec![
            part(3, "obm_Cuirass", "obf_Cuirass"),
            part(13, "", ""),
            part(14, "", ""),
            part(21, "", ""),
        ];
        let exposed = HashMap::from([
            ("obf_Cuirass".to_owned(), BTreeSet::from([3, 13, 14])),
            ("obm_Cuirass".to_owned(), BTreeSet::new()),
        ]);
        assert_eq!(unhide_parts(&mut parts, &exposed), 2);
        let slots: Vec<u8> = parts.iter().map(|p| p.slot).collect();
        assert_eq!(slots, [3, 21], "грудь с телом остаётся, хоть и открыта");
    }

    /// Контрольный опыт: без открытых слотов вещь не меняется.
    #[test]
    fn nothing_exposed_changes_nothing() {
        let mut parts = vec![part(3, "obm_Shirt", ""), part(11, "", ""), part(12, "", "")];
        let before = parts.clone();
        assert_eq!(unhide_parts(&mut parts, &HashMap::new()), 0);
        assert_eq!(parts, before);
    }
}

#[cfg(test)]
mod world_tests {
    use super::*;
    use transform::exterior::SourceCell;

    #[test]
    fn default_offset_puts_the_world_east_of_vanilla() {
        let cell = |x, y| SourceCell {
            grid: (x, y),
            name: None,
            regions: Vec::new(),
            water_height: None,
            land: None,
        };
        // Как SEWorld: x −32…31, y −44…37 → целевые x −16…15, y −22…18.
        let source = ExteriorSource {
            cells: vec![cell(-32, 5), cell(31, -44), cell(0, 37)],
            ..ExteriorSource::default()
        };
        let o = default_offset(&source);
        assert_eq!((o.dx, o.dy), (48, 3));
        // Западный край — через зазор от восточного края ванили, южный — на уровне её южного.
        assert_eq!(-16 + o.dx, VANILLA_EXTERIOR.2 + 1 + WORLD_GAP);
        assert_eq!(-22 + o.dy, VANILLA_EXTERIOR.1);
    }
}

#[cfg(test)]
mod prune_tests {
    use super::*;
    use tes3_out::Container;

    fn info(name: &str) -> PluginInfo {
        PluginInfo {
            name: name.to_owned(),
            kind: plugins::Kind::Esp,
            size: 0,
            official: false,
            masters: Vec::new(),
            error: None,
            load_index: 0,
        }
    }

    fn list(id: &str, items: &[&str]) -> LeveledItem {
        LeveledItem {
            id: id.to_owned(),
            all_levels: false,
            each_item: false,
            chance_none: 0,
            items: items.iter().map(|i| ((*i).to_owned(), 1)).collect(),
        }
    }

    fn work<'a>(
        info: &'a PluginInfo,
        objects: Vec<Object>,
        leveled: Vec<LeveledItem>,
    ) -> PluginWork<'a> {
        PluginWork {
            info,
            objects,
            origins: Vec::new(),
            leveled,
            globals: Vec::new(),
            creatures: Vec::new(),
            creature_models: Vec::new(),
            creature_lists: Vec::new(),
            enchantments: Vec::new(),
            spells: Vec::new(),
            sounds: Vec::new(),
            sound_origins: Vec::new(),
            sound_gens: Vec::new(),
            sound_sources: BTreeMap::new(),
            scripts: Vec::new(),
            sound_markers: Vec::new(),
            world: None,
            actors: None,
            topics: Vec::new(),
            voices: BTreeMap::new(),
        }
    }

    /// T10.1: `SOUN`, чей файл не записан, уходит из плагина, дверь теряет только этот звук.
    #[test]
    fn a_sound_without_a_file_is_dropped_and_the_door_keeps_the_other() {
        let plugin = info("Master.esm");
        let door = Object::Door(tes3_out::Door {
            id: "ob_door_a".to_owned(),
            name: String::new(),
            mesh: String::new(),
            open_sound: "ob_Open_1".to_owned(),
            close_sound: "OB_LOST_2".to_owned(),
        });
        let sound = |id: &str, file: &str| Sound {
            id: id.to_owned(),
            file: file.to_owned(),
            volume: 255,
            min_range: 0,
            max_range: 0,
        };
        let origin = |source: &str| SoundOrigin {
            editor_id: String::new(),
            source: source.to_owned(),
            directory: false,
        };
        let mut w = work(&plugin, vec![door], Vec::new());
        w.sounds = vec![
            sound("ob_Open_1", "ob\\open.wav"),
            sound("ob_Lost_2", "ob\\lost.wav"),
        ];
        w.sound_origins = vec![origin("fx\\open.wav"), origin("fx\\lost.wav")];
        // T10.2: регион тоже теряет только звук без файла.
        let region = tes3_out::Region {
            id: "ob_region_c".to_owned(),
            name: String::new(),
            weather: tes3_out::WeatherChances::default(),
            map_color: [0; 3],
            sounds: vec![("ob_Open_1".to_owned(), 30), ("ob_lost_2".to_owned(), 25)],
        };
        w.world = Some(WorldWork {
            source: ExteriorSource::default(),
            base_kinds: HashMap::new(),
            vanilla: HashMap::new(),
            converted: Some(ConvertedExterior {
                regions: vec![region],
                ..ConvertedExterior::default()
            }),
            offset: Offset::default(),
        });
        let mut works = vec![w];
        silence_missing_sounds(&mut works, &BTreeSet::from(["ob\\open.wav".to_owned()]));
        let regions = &works[0]
            .world
            .as_ref()
            .unwrap()
            .converted
            .as_ref()
            .unwrap()
            .regions;
        assert_eq!(regions[0].sounds, [("ob_Open_1".to_owned(), 30)]);
        let ids: Vec<&str> = works[0].sounds.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["ob_Open_1"]);
        assert_eq!(works[0].sound_origins.len(), 1);
        let Object::Door(d) = &works[0].objects[0] else {
            panic!("дверь");
        };
        assert_eq!(
            (d.open_sound.as_str(), d.close_sound.as_str()),
            ("ob_Open_1", "")
        );
    }

    fn source_sound(editor_id: &str, file: &str) -> tes4_read::Sound {
        tes4_read::Sound {
            editor_id: editor_id.to_owned(),
            file: file.to_owned(),
            min_distance: 0,
            max_distance: 0,
            frequency: 0,
            flags: 0,
            attenuation: 0,
        }
    }

    /// T10.3: стоящий в мире `SOUN` → маркер со скриптом; звук, уже взятый дверью, не дублируется,
    /// скрипт, уже взятый активатором, — тоже; `SOUN` не из плагина и не-звуки не трогаются.
    #[test]
    fn placed_sounds_get_markers_with_loop_scripts() {
        let kinds = HashMap::from([
            (0x31, "SOUN".to_owned()),
            (0x30, "SOUN".to_owned()),
            (0x40, "STAT".to_owned()),
            (0x99, "SOUN".to_owned()),
        ]);
        let sources = BTreeMap::from([
            (
                0x30,
                (
                    "ob_Wind_30".to_owned(),
                    source_sound("Wind", "fx\\wind.wav"),
                ),
            ),
            (
                0x31,
                ("ob_Drip_31".to_owned(), source_sound("Drip", "fx\\drip\\")),
            ),
        ]);
        let existing =
            transform::sounds::convert_sound(&sources[&0x30].1, "ob_wind_30".into()).unwrap();
        let mut sounds = vec![existing.sound];
        let mut origins = vec![existing.origin];
        let mut scripts = vec![transform::sounds::loop_script("ob_Wind_30")];
        let (mut markers, mut outcome) = (Vec::new(), Outcome::default());
        let placed = place_sound_markers(
            &kinds,
            &sources,
            &mut sounds,
            &mut origins,
            &mut scripts,
            &mut markers,
            &mut outcome,
            "Test.esp",
        );
        assert_eq!(
            placed,
            [
                (0x30, "obsm_Wind_30".to_owned()),
                (0x31, "obsm_Drip_31".to_owned())
            ]
        );
        let ids: Vec<&str> = sounds.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["ob_wind_30", "ob_Drip_31"], "Wind уже был");
        assert!(origins[1].directory);
        let script_ids: Vec<&str> = scripts.iter().map(Script::id).collect();
        assert_eq!(script_ids, ["obls_Wind_30", "obls_Drip_31"]);
        assert_eq!(markers.len(), 2);
        assert!(
            markers
                .iter()
                .all(|m| m.mesh == transform::sounds::MARKER_MESH)
        );
        assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
    }

    /// T10.3: скрипт петли звука без файла не пишется, активатор и маркер остаются без скрипта;
    /// скрипт звука с файлом остаётся.
    #[test]
    fn a_loop_script_of_a_silent_sound_is_dropped() {
        let plugin = info("Master.esm");
        let machine = Object::Activator(Activator {
            id: "ob_machine_a".to_owned(),
            name: "Machine".to_owned(),
            mesh: String::new(),
            script: "obls_Lost_2".to_owned(),
        });
        let mut w = work(&plugin, vec![machine], Vec::new());
        for (id, file) in [("ob_Kept_1", "ob\\kept.wav"), ("ob_Lost_2", "ob\\lost.wav")] {
            w.sounds.push(Sound {
                id: id.to_owned(),
                file: file.to_owned(),
                volume: 255,
                min_range: 0,
                max_range: 0,
            });
            w.sound_origins.push(SoundOrigin {
                editor_id: String::new(),
                source: String::new(),
                directory: false,
            });
        }
        let (kept_marker, kept_script) = transform::sounds::sound_marker("ob_Kept_1");
        let (lost_marker, lost_script) = transform::sounds::sound_marker("ob_Lost_2");
        w.scripts = vec![kept_script, lost_script];
        w.sound_markers = vec![kept_marker, lost_marker];
        let mut works = vec![w];
        silence_missing_sounds(&mut works, &BTreeSet::from(["ob\\kept.wav".to_owned()]));
        let w = &works[0];
        let ids: Vec<&str> = w.scripts.iter().map(Script::id).collect();
        assert_eq!(ids, ["obls_Kept_1"]);
        let Object::Activator(a) = &w.objects[0] else {
            panic!("активатор");
        };
        assert!(a.script.is_empty());
        let on_markers: Vec<&str> = w.sound_markers.iter().map(|m| m.script.as_str()).collect();
        assert_eq!(
            on_markers,
            ["obls_Kept_1", ""],
            "маркер остаётся: на него ссылаются ячейки"
        );
    }

    /// Список мастера опустел (меш его единственного предмета не сконвертировался) — из списка
    /// мода и из сундука мода ссылка на него уходит, хотя это другой плагин.
    #[test]
    fn an_emptied_list_is_removed_from_other_plugins_too() {
        let (master, module) = (info("Master.esm"), info("Mod.esp"));
        let chest = Object::Container(Container {
            id: "ob_chest_a".to_owned(),
            name: String::new(),
            mesh: String::new(),
            capacity: 200.0,
            organic: false,
            respawns: false,
            items: vec![(1, "ob_modlist_b".to_owned()), (3, "Gold_001".to_owned())],
        });
        let mut works = vec![
            work(
                &master,
                Vec::new(),
                vec![list("ob_masterlist_a", &["ob_lostcup_c"])],
            ),
            work(
                &module,
                vec![chest],
                vec![list("ob_modlist_b", &["ob_masterlist_a"])],
            ),
        ];
        let mut outcome = Outcome::default();
        prune_contents(&mut works, &mut outcome);
        assert!(works.iter().all(|w| w.leveled.is_empty()));
        let Object::Container(c) = &works[1].objects[0] else {
            unreachable!()
        };
        assert_eq!(
            c.items,
            [(3, "Gold_001".to_owned())],
            "ванильное не трогаем"
        );
        assert_eq!(outcome.problems.len(), 3, "{:?}", outcome.problems);
    }

    fn creature(id: &str) -> tes3_out::Creature {
        tes3_out::Creature {
            id: id.to_owned(),
            name: String::new(),
            mesh: "r\\Rust rat.NIF".to_owned(),
            sound: "rat".to_owned(),
            kind: 0,
            level: 1,
            attributes: [10; 8],
            health: 10,
            magicka: 0,
            fatigue: 10,
            soul: 10,
            combat: 10,
            magic: 10,
            stealth: 10,
            attack: (1, 3),
            gold: 0,
            biped: false,
            weapon_and_shield: false,
            swims: false,
            flies: false,
            walks: true,
            essential: false,
            respawn: false,
            blood_type: 0,
            scale: None,
            inventory: Vec::new(),
            spells: Vec::new(),
            ai: tes3_out::NpcAi {
                hello: 0,
                fight: 90,
                flee: 0,
                alarm: 0,
                services: 0,
                wander_distance: 0,
                wander_duration: 5,
                idles: [0; 8],
            },
        }
    }

    fn creature_list(id: &str, rows: &[&str]) -> LeveledCreature {
        LeveledCreature {
            id: id.to_owned(),
            all_levels: false,
            chance_none: 0,
            creatures: rows.iter().map(|r| ((*r).to_owned(), 1)).collect(),
        }
    }

    /// T11.4: строка списка существ на запись, которой нет ни в одном плагине прогона, уходит;
    /// опустевший список не пишется, и ссылка на него из другого плагина — тоже. Ванильное
    /// существо (без нашего префикса) остаётся.
    #[test]
    fn creature_lists_lose_rows_to_unwritten_records_across_plugins() {
        let (master, module) = (info("Master.esm"), info("Mod.esp"));
        let mut works = vec![
            work(&master, Vec::new(), Vec::new()),
            work(&module, Vec::new(), Vec::new()),
        ];
        works[0].creatures = vec![(0x10, creature("ob_rat_a"))];
        works[0].creature_lists = vec![
            (
                0x20,
                creature_list("ob_rats_b", &["ob_rat_a", "ob_lostwolf_c"]),
            ),
            (0x21, creature_list("ob_wolves_d", &["ob_lostwolf_c"])),
        ];
        works[1].creature_lists = vec![
            (
                0x0100_0030,
                creature_list("ob_modpack_e", &["ob_wolves_d", "rat"]),
            ),
            (
                0x0100_0031,
                creature_list("ob_modwolves_f", &["ob_wolves_d"]),
            ),
        ];
        let mut outcome = Outcome::default();
        prune_contents(&mut works, &mut outcome);
        assert_eq!(
            works[0].creature_lists,
            [(0x20, creature_list("ob_rats_b", &["ob_rat_a"]))]
        );
        assert_eq!(
            works[1].creature_lists,
            [(0x0100_0030, creature_list("ob_modpack_e", &["rat"]))]
        );
        assert_eq!(outcome.problems.len(), 2, "{:?}", outcome.problems);
        assert!(
            outcome.problems[0].starts_with(
                "Master.esm: rows removed from creature lists: 2, emptied lists not written: 1"
            ),
            "{:?}",
            outcome.problems
        );

        // Контроль: когда все цели записаны, списки не меняются и замечаний нет.
        let mut whole = vec![work(&master, Vec::new(), Vec::new())];
        whole[0].creatures = vec![(0x10, creature("ob_rat_a"))];
        whole[0].creature_lists = vec![(0x20, creature_list("ob_rats_b", &["ob_rat_a", "rat"]))];
        let before = whole[0].creature_lists.clone();
        let mut outcome = Outcome::default();
        prune_contents(&mut whole, &mut outcome);
        assert_eq!(whole[0].creature_lists, before);
        assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
    }

    /// T2.6: итог по дверям — смещения только у дверей с перенесённой петлёй; двери, которые
    /// Morrowind повернёт не так, и открывающиеся в другую сторону — замечаниями.
    #[test]
    fn door_report_gives_pivots_and_notes_the_doors_morrowind_cannot_express() {
        use mesh_convert::door::DoorFate;
        let plugin = info("Master.esm");
        let mut w = work(&plugin, Vec::new(), Vec::new());
        let origin = |form_id: u32, door: Option<&str>| ObjectOrigin {
            form_id,
            editor_id: format!("Door{form_id}"),
            model: format!("doors\\door{form_id}.nif"),
            icon: None,
            book_art: Vec::new(),
            body: None,
            tree: None,
            door: door.map(str::to_owned),
        };
        w.origins = vec![
            origin(1, Some("ob\\door1_a.nif")),
            origin(2, Some("ob\\door2_b.nif")),
            origin(3, Some("ob\\door3_c.nif")),
            origin(4, Some("ob\\door4_d.nif")),
            // Статика с той же моделью, что дверь 1: к дверям не относится.
            origin(5, None),
        ];
        let info_of = |fate, pivot, counter_clockwise| DoorInfo {
            fate,
            pivot,
            counter_clockwise,
        };
        let door_info = BTreeMap::from([
            (
                "@ob\\door1_a.nif".to_owned(),
                info_of(DoorFate::Rehinged, Some([48.8, 2.9, 0.0]), true),
            ),
            (
                "@ob\\door2_b.nif".to_owned(),
                info_of(DoorFate::AtOrigin, None, false),
            ),
            (
                "@ob\\door3_c.nif".to_owned(),
                info_of(DoorFate::Sliding, None, false),
            ),
            // У двери 4 меш не прочитался: сведений нет, в счёт она не идёт.
        ]);
        let mut outcome = Outcome::default();
        let mut lines = Vec::new();
        let pivots = report_doors(&[w], &door_info, &mut outcome, &mut |l| {
            lines.push(l.to_owned());
        });
        assert_eq!(pivots, HashMap::from([(1, [48.8, 2.9, 0.0])]));
        assert_eq!(
            lines,
            [
                "meshes: Master.esm — doors 3: without animation 0, hinge at the mesh origin 1, hinge moved to the mesh origin 1"
            ]
        );
        assert_eq!(outcome.problems.len(), 2, "{:?}", outcome.problems);
        assert!(outcome.problems[0].contains("sliding (portcullises) 1"));
        assert!(outcome.problems[1].contains("open the other way than in Oblivion: 1"));

        // Контроль: плагин без дверей — ни строки, ни замечаний.
        let mut outcome = Outcome::default();
        let empty = work(&plugin, Vec::new(), Vec::new());
        let pivots = report_doors(&[empty], &door_info, &mut outcome, &mut |_| {
            panic!("строк быть не должно")
        });
        assert!(pivots.is_empty() && outcome.problems.is_empty());
    }

    /// Какие записи переносить: `--only` и мир — оба фильтра вместе, без обоих — все.
    #[test]
    fn wanted_records_combine_the_only_filter_with_the_world() {
        use tes4_read::{GameFormat, synth};
        let lvlc = |form_id: u32, edid: &str| {
            synth::record(
                GameFormat::Tes4,
                b"LVLC",
                form_id,
                0,
                &synth::field(b"EDID", format!("{edid}\0").as_bytes()),
            )
        };
        let records = [
            lvlc(0x10, "LLRat"),
            lvlc(0x11, "LLWolf"),
            lvlc(0x12, "LLBear"),
        ]
        .concat();
        let bytes = synth::plugin(
            GameFormat::Tes4,
            &synth::header_fields(3),
            &[synth::group(GameFormat::Tes4, b"LVLC", 0, &records)],
        );
        let plugin = Plugin::parse(&bytes).unwrap();
        let only: BTreeSet<String> = ["llrat".to_owned(), "llwolf".to_owned()].into();
        let world = BTreeSet::from([0x11, 0x12]);
        let none = BTreeSet::new();
        let tag = *b"LVLC";
        assert_eq!(wanted_records(&plugin, tag, Some, &none, None), None);
        assert_eq!(
            wanted_records(&plugin, tag, Some, &none, Some(&world)),
            Some(world.clone())
        );
        assert_eq!(
            wanted_records(&plugin, tag, Some, &only, None),
            Some(BTreeSet::from([0x10, 0x11]))
        );
        assert_eq!(
            wanted_records(&plugin, tag, Some, &only, Some(&world)),
            Some(BTreeSet::from([0x11]))
        );
    }

    /// Инвентарь и заклинания NPC чистятся по тем же правилам, что сундук.
    #[test]
    fn npc_loses_unwritten_items_and_spells_but_keeps_vanilla_ones() {
        let master = info("Master.esm");
        let mut w = work(
            &master,
            Vec::new(),
            vec![list("ob_keptlist_a", &["Gold_001"])],
        );
        let npc = tes3_out::Npc {
            id: "ob_npc_a".into(),
            name: String::new(),
            race: "Nord".into(),
            class: "ob_class_a".into(),
            faction: None,
            head: String::new(),
            hair: String::new(),
            mesh: String::new(),
            female: false,
            essential: false,
            respawn: false,
            level: 1,
            gold: 0,
            inventory: vec![
                (1, "ob_keptlist_a".into()),
                (1, "ob_lostsword_b".into()),
                (5, "Gold_001".into()),
            ],
            spells: vec!["ob_lostspell_c".into()],
            ai: tes3_out::NpcAi {
                hello: 30,
                fight: 30,
                flee: 30,
                alarm: 0,
                services: 0,
                wander_distance: 0,
                wander_duration: 5,
                idles: [0; 8],
            },
            script: String::new(),
        };
        w.actors = Some(ConvertedActors {
            npcs: vec![(1, npc)],
            ..ConvertedActors::default()
        });
        let mut works = vec![w];
        let mut outcome = Outcome::default();
        prune_contents(&mut works, &mut outcome);
        let npc = &works[0].actors.as_ref().unwrap().npcs[0].1;
        assert_eq!(
            npc.inventory,
            [(1, "ob_keptlist_a".to_owned()), (5, "Gold_001".to_owned())]
        );
        assert!(npc.spells.is_empty());
        assert_eq!(outcome.problems.len(), 1, "{:?}", outcome.problems);
    }

    /// T10.8: реплика с голосом, чей файл не записан, выбрасывается; тема без реплик — тоже;
    /// реплика без голоса и с записанным голосом остаются.
    #[test]
    fn a_voice_line_without_its_file_is_dropped() {
        use tes3_out::dialogue::{Info, Topic, TopicKind};
        let plugin = info("Master.esm");
        let mut w = work(&plugin, Vec::new(), Vec::new());
        let voiced = |id: &str, sound: &str| {
            let mut i = Info::new(id, "Die!");
            i.sound = sound.to_owned();
            i
        };
        w.topics = vec![
            Topic {
                name: "Attack".into(),
                kind: TopicKind::Voice,
                before_vanilla: String::new(),
                infos: vec![
                    voiced("1", r"Vo\ob\kept.mp3"),
                    voiced("2", r"Vo\ob\lost.mp3"),
                ],
            },
            Topic {
                name: "Hit".into(),
                kind: TopicKind::Voice,
                before_vanilla: String::new(),
                infos: vec![voiced("3", r"Vo\ob\lost.mp3")],
            },
            Topic {
                name: "Bruma".into(),
                kind: TopicKind::Topic,
                before_vanilla: String::new(),
                infos: vec![Info::new("4", "A town.")],
            },
        ];
        let mut works = vec![w];
        silence_missing_voices(&mut works, &BTreeSet::from([r"Vo\ob\kept.mp3".to_owned()]));
        let left: Vec<(&str, Vec<&str>)> = works[0]
            .topics
            .iter()
            .map(|t| {
                (
                    t.name.as_str(),
                    t.infos.iter().map(|i| i.id.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(left, [("Attack", vec!["1"]), ("Bruma", vec!["4"])]);
    }
}

#[cfg(test)]
mod start_hint_tests {
    use super::*;
    use transform::exterior::{StartCell, StartPoint};

    fn reference() -> tes3_out::Reference {
        tes3_out::Reference {
            index: 1,
            id: "ob_Rock_a".to_owned(),
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: None,
            destination: None,
            lock: None,
            persistent: false,
        }
    }

    fn interior(name: &str, with_references: bool) -> tes3_out::InteriorCell {
        tes3_out::InteriorCell {
            name: name.to_owned(),
            ambient: [0; 4],
            sunlight: [0; 4],
            fog_color: [0; 4],
            fog_density: 0.0,
            water_height: None,
            references: if with_references {
                vec![reference()]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn the_start_marker_gives_an_exterior_cell_and_the_console_command() {
        let mut converted = ConvertedExterior::default();
        converted.report.start = Some(StartPoint {
            cell: StartCell::Exterior(7, -3),
            position: [57_344.4, -24_000.6, 1_200.0],
        });
        let hint = start_hint("SEWorld", &converted).unwrap();
        assert_eq!(hint.cell, StartTarget::Exterior(7, -3));
        assert_eq!(hint.position, Some([57_344, -24_001, 1_200]));
        let line = hint.line("SEWorld.esp");
        assert!(
            line.starts_with("teleport: SEWorld.esp (SEWorld)"),
            "{line}"
        );
        assert!(line.contains("coe 7 -3"), "{line}");
        assert!(line.contains("(57344, -24001, 1200)"), "{line}");
    }

    #[test]
    fn a_marker_in_an_interior_gives_coc_with_the_cell_name() {
        let mut converted = ConvertedExterior {
            interiors: vec![interior("Hall", true), interior("Cellar", true)],
            ..ConvertedExterior::default()
        };
        converted.report.start = Some(StartPoint {
            cell: StartCell::Interior(1),
            position: [1.0, 2.0, 3.0],
        });
        let line = start_hint("SEManiaGarden", &converted)
            .unwrap()
            .line("SEManiaGarden.esp");
        assert!(line.contains("coc \"Cellar\""), "{line}");
    }

    #[test]
    fn without_a_marker_the_first_cell_with_objects_is_named_and_empty_ones_are_skipped() {
        let mut converted = ConvertedExterior {
            cells: vec![
                tes3_out::ExteriorCell {
                    grid: (1, 1),
                    name: String::new(),
                    region: None,
                    references: Vec::new(),
                },
                tes3_out::ExteriorCell {
                    grid: (2, 5),
                    name: String::new(),
                    region: None,
                    references: vec![reference()],
                },
            ],
            ..ConvertedExterior::default()
        };
        let hint = start_hint("SEWorld", &converted).unwrap();
        assert_eq!(
            (hint.cell.clone(), hint.position),
            (StartTarget::Exterior(2, 5), None)
        );
        assert!(hint.line("SEWorld.esp").contains("coe 2 5"));
        assert_eq!(start_hint("Empty", &ConvertedExterior::default()), None);
        // Интерьер — запасной, только когда снаружи объектов нет.
        converted.cells.clear();
        converted.interiors = vec![interior("Empty", false), interior("Hall", true)];
        assert_eq!(
            start_hint("W", &converted).unwrap().cell,
            StartTarget::Interior("Hall".to_owned())
        );
    }
}
