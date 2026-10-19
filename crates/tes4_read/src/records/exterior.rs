//! Записи внешнего мира кроме `LAND`: `WRLD`, `CELL`, `REFR`, `LTEX`, `REGN`, `WTHR`, `CLMT`.
//!
//! Раскладки — OpenMW `components/esm4/load{wrld,cell,refr,regn,ltex}.cpp`; `WTHR`, `CLMT` и
//! `RDWT` OpenMW не разбирает, их раскладка измерена на `Oblivion.esm` (2026-09-25,
//! `docs/ru/formats/tes4.md`, «Внешний мир»).

use crate::container::{Record, Tag};
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// Флаг заголовка записи-ссылки: изначально выключена.
pub const REF_INITIALLY_DISABLED: u32 = 0x0000_0800;

fn u32_at(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().expect("срез ровно 4 байта"))
}

fn f32_at(raw: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(raw[at..at + 4].try_into().expect("срез ровно 4 байта"))
}

/// Массив записей фиксированной длины в одном поле.
fn chunks<'a>(
    record: &'static str,
    field: Tag,
    raw: &'a [u8],
    size: usize,
) -> Result<std::slice::ChunksExact<'a, u8>> {
    if !raw.len().is_multiple_of(size) {
        return Err(Error::FieldSize {
            record,
            field,
            expected: raw.len() / size * size + size,
            got: raw.len(),
        });
    }
    Ok(raw.chunks_exact(size))
}

fn text(fields: &Fields<'_>, tag: Tag) -> Option<String> {
    fields.opt_zstring(tag).filter(|s| !s.is_empty())
}

/// Worldspace.
#[derive(Debug, Clone, PartialEq)]
pub struct Worldspace {
    pub editor_id: String,
    pub name: Option<String>,
    /// `WNAM`: родитель, чей ландшафт worldspace использует (города внутри `Tamriel`).
    pub parent: Option<u32>,
    /// `CNAM`: климат — погода там, где регион её не задаёт.
    pub climate: Option<u32>,
    /// `ICON`: рисованный лист карты мира, путь относительно `Textures\`
    /// (`Menus\Map\world\TES4SEmap.dds`). В `Oblivion.esm` он есть у пяти миров: `Tamriel`,
    /// `SEWorld` и ещё трёх миров Дрожащих островов с тем же листом (2026-10-02).
    pub map_texture: Option<String>,
    /// `MNAM`: какие ячейки покрывает лист. У тестовых миров поле из одних нулей — это `None`.
    pub map_size: Option<MapSize>,
}

/// `WRLD` `MNAM`, 16 байт: размер листа карты в пикселях и ячейки его углов. Как лист лежит на
/// этих ячейках — `docs/ru/formats/tes4.md`, «Карта мира».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapSize {
    pub width: u32,
    pub height: u32,
    /// Северо-западная ячейка: (x, y).
    pub north_west: (i16, i16),
    /// Юго-восточная ячейка: (x, y).
    pub south_east: (i16, i16),
}

impl FromFields for Worldspace {
    const SIG: Tag = *b"WRLD";
    const NAME: &'static str = "WRLD";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let i16_at = |raw: &[u8; 16], at: usize| i16::from_le_bytes([raw[at], raw[at + 1]]);
        let map_size = fields
            .opt_fixed::<16>(*b"MNAM")?
            .map(|raw| MapSize {
                width: u32_at(&raw, 0),
                height: u32_at(&raw, 4),
                north_west: (i16_at(&raw, 8), i16_at(&raw, 10)),
                south_east: (i16_at(&raw, 12), i16_at(&raw, 14)),
            })
            .filter(|m| m.width > 0 && m.height > 0);
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            name: text(fields, *b"FULL"),
            parent: fields.opt_u32(*b"WNAM")?,
            climate: fields.opt_u32(*b"CNAM")?,
            map_texture: text(fields, *b"ICON"),
            map_size,
        })
    }
}

/// Ячейка. Внешняя — с `XCLC`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub editor_id: Option<String>,
    pub name: Option<String>,
    /// `XCLC`: координаты ячейки (4096 юнитов); у внутренних и у постоянной ячейки worldspace нет.
    pub grid: Option<(i32, i32)>,
    /// `XCLR`: регионы, к которым ячейка принадлежит.
    pub regions: Vec<u32>,
    /// `XCLW`: свой уровень воды; без него — уровень worldspace.
    pub water_height: Option<f32>,
    /// `DATA`: 0x01 внутренняя, 0x02 есть вода, 0x20 общественное место, 0x80 ведёт себя как
    /// внешняя (UESP, `CELL`; OpenMW `loadcell.cpp`).
    pub flags: u8,
    /// `XCLL`: свет внутренней ячейки.
    pub lighting: Option<CellLighting>,
}

/// Флаг `DATA` ячейки: есть вода.
pub const CELL_HAS_WATER: u8 = 0x02;

/// Свет внутренней ячейки (`XCLL`, 36 байт у всех 162 интерьеров `SEWorld`, 2026-09-26). Цвета —
/// R, G, B и неиспользуемый байт. Хвост — поворот и затухание направленного света, дальность
/// отсечения тумана — Morrowind не выражает.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellLighting {
    pub ambient: [u8; 4],
    pub directional: [u8; 4],
    pub fog_color: [u8; 4],
    pub fog_near: f32,
    pub fog_far: f32,
}

impl FromFields for Cell {
    const SIG: Tag = *b"CELL";
    const NAME: &'static str = "CELL";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let grid = match fields.first(*b"XCLC") {
            None => None,
            // 12 байт — ещё флаги скрытия квадрантов, нам не нужны.
            Some(raw) if raw.len() == 8 || raw.len() == 12 => Some((
                i32::from_le_bytes(raw[0..4].try_into().expect("4 байта")),
                i32::from_le_bytes(raw[4..8].try_into().expect("4 байта")),
            )),
            Some(raw) => {
                return Err(Error::FieldSize {
                    record: "CELL",
                    field: *b"XCLC",
                    expected: 8,
                    got: raw.len(),
                });
            }
        };
        let regions = match fields.first(*b"XCLR") {
            None => Vec::new(),
            Some(raw) => chunks("CELL", *b"XCLR", raw, 4)?
                .map(|c| u32_at(c, 0))
                .collect(),
        };
        Ok(Self {
            editor_id: text(fields, *b"EDID"),
            name: text(fields, *b"FULL"),
            grid,
            regions,
            water_height: fields.opt_f32(*b"XCLW")?,
            flags: fields
                .first(*b"DATA")
                .and_then(|d| d.first().copied())
                .unwrap_or(0),
            lighting: match fields.first(*b"XCLL") {
                None => None,
                Some(raw) if raw.len() >= 20 => Some(CellLighting {
                    ambient: raw[0..4].try_into().expect("4 байта"),
                    directional: raw[4..8].try_into().expect("4 байта"),
                    fog_color: raw[8..12].try_into().expect("4 байта"),
                    fog_near: f32_at(raw, 12),
                    fog_far: f32_at(raw, 16),
                }),
                Some(raw) => {
                    return Err(Error::FieldSize {
                        record: "CELL",
                        field: *b"XCLL",
                        expected: 36,
                        got: raw.len(),
                    });
                }
            },
        })
    }
}

/// Родитель включения (`XESP`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnableParent {
    pub parent: u32,
    /// Состояние противоположно родителю.
    pub inverted: bool,
}

/// Назначение двери (`XTEL`, 28 байт: `FormID` двери на том конце, позиция и углы, где
/// появится игрок — OpenMW `loadrefr.cpp`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Teleport {
    pub door: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
}

/// Замок (`XLOC`, 12 или 16 байт): уровень (u8), 3 байта мусора, `FormID` ключа, флаги — последние
/// 4 байта (UESP, «Mod File Format/REFR»; OpenMW `loadrefr.cpp` читает те же уровень и ключ). В
/// `Oblivion.esm` 2 866 замков: 2 841 по 12 байт, 25 по 16 — у всех 16-байтных байты 8 и 12
/// нулевые, так что где у них флаги, данные не различают (2026-09-27, скрипт по файлу).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lock {
    /// 0…100; 100 — открывается только ключом.
    pub level: u8,
    /// `KEYM`; `None` — ключа нет.
    pub key: Option<u32>,
    /// Флаг 0x4: сложность растёт с уровнем игрока.
    pub leveled: bool,
}

/// Флаг `XLOC`: сложность замка растёт с уровнем игрока.
pub const LOCK_LEVELED: u32 = 0x4;

/// Размещение объекта (`REFR`), NPC (`ACHR`) или существа (`ACRE`).
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// `FormID` самой ссылки (из заголовка).
    pub form_id: u32,
    /// `NAME`: базовый объект.
    pub base: u32,
    /// `DATA`: позиция, юниты мира.
    pub position: [f32; 3],
    /// `DATA`: углы, радианы. Та же структура `ESM::Position`, что у TES3.
    pub rotation: [f32; 3],
    pub scale: Option<f32>,
    /// Флаг заголовка 0x800.
    pub initially_disabled: bool,
    pub enable_parent: Option<EnableParent>,
    /// `XTEL`: дверь-телепорт.
    pub teleport: Option<Teleport>,
    /// `XMRK`: маркер карты.
    pub map_marker: bool,
    /// `XSED`, первый байт: номер семени дерева в `SNAM` его `TREE` (берётся по модулю числа
    /// семян).
    pub tree_seed: Option<u8>,
    /// `XLOC`: замок двери или контейнера.
    pub lock: Option<Lock>,
}

impl FromFields for Placement {
    const SIG: Tag = *b"REFR";
    const NAME: &'static str = "REFR";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let data = fields.fixed::<24>(*b"DATA")?;
        let enable_parent = fields.opt_fixed::<8>(*b"XESP")?.map(|raw| EnableParent {
            parent: u32_at(&raw, 0),
            inverted: raw[4] & 1 != 0,
        });
        Ok(Self {
            form_id: 0,
            base: fields.u32(*b"NAME")?,
            position: [f32_at(&data, 0), f32_at(&data, 4), f32_at(&data, 8)],
            rotation: [f32_at(&data, 12), f32_at(&data, 16), f32_at(&data, 20)],
            scale: fields.opt_f32(*b"XSCL")?,
            initially_disabled: false,
            enable_parent,
            teleport: match fields.first(*b"XTEL") {
                None => None,
                Some(raw) if raw.len() == 28 => Some(Teleport {
                    door: u32_at(raw, 0),
                    position: [f32_at(raw, 4), f32_at(raw, 8), f32_at(raw, 12)],
                    rotation: [f32_at(raw, 16), f32_at(raw, 20), f32_at(raw, 24)],
                }),
                Some(raw) => {
                    return Err(Error::FieldSize {
                        record: "REFR",
                        field: *b"XTEL",
                        expected: 28,
                        got: raw.len(),
                    });
                }
            },
            map_marker: fields.first(*b"XMRK").is_some(),
            tree_seed: fields.first(*b"XSED").and_then(|raw| raw.first().copied()),
            lock: match fields.first(*b"XLOC") {
                None => None,
                Some(raw) if raw.len() == 12 || raw.len() == 16 => {
                    let key = u32_at(raw, 4);
                    Some(Lock {
                        level: raw[0],
                        key: (key != 0).then_some(key),
                        leveled: u32_at(raw, raw.len() - 4) & LOCK_LEVELED != 0,
                    })
                }
                Some(raw) => {
                    return Err(Error::FieldSize {
                        record: "REFR",
                        field: *b"XLOC",
                        expected: 12,
                        got: raw.len(),
                    });
                }
            },
        })
    }

    /// Флаг выключенности и `FormID` лежат в заголовке записи, а не в полях.
    /// `ACHR` (размещение NPC, T7.6) читается той же спекой: `NAME`, `DATA`, `XESP`, `XSCL` у него
    /// те же, а полей, которых нет у `REFR` (`XHRS`, `XMRC`, `XRGD`), перенос не использует.
    /// `ACRE` (размещение существа, T11.4) — так же: `NAME`, `DATA` 24 байта, `XSCL`, `XESP`
    /// (все 1 473 `ACRE` `Oblivion.esm` читаются, 2026-10-01, `real_creatures`).
    fn from_record(record: &Record) -> Result<Self> {
        if record.header.tag != Self::SIG && !matches!(&record.header.tag, b"ACHR" | b"ACRE") {
            return Err(Error::WrongRecord {
                expected: Self::SIG,
                got: record.header.tag,
            });
        }
        let mut out = Self::from_fields(&Fields::parse(&record.data, Self::NAME)?)?;
        out.form_id = record.header.form_id;
        out.initially_disabled = record.header.flags & REF_INITIALLY_DISABLED != 0;
        Ok(out)
    }
}

/// Текстура земли.
#[derive(Debug, Clone, PartialEq)]
pub struct LandTexture {
    pub editor_id: String,
    /// `ICON`: путь относительно `Textures\Landscape\`.
    pub texture: Option<String>,
}

impl FromFields for LandTexture {
    const SIG: Tag = *b"LTEX";
    const NAME: &'static str = "LTEX";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            texture: text(fields, *b"ICON"),
        })
    }
}

/// Погода региона: `RDAT` типа 3 и следующий за ним `RDWT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionWeather {
    /// Флаг `RDAT` 0x01 — «перекрывает» (override).
    pub overrides: bool,
    pub priority: u8,
    /// `WTHR` и шанс.
    pub weathers: Vec<(u32, u32)>,
}

/// Звук региона: элемент `RDSD` (12 байт) после `RDAT` типа 7 (T10.2).
///
/// Измерено по `Oblivion.esm` (2026-09-28, `docs/ru/formats/tes4.md`, «Звуки регионов»): длина
/// `RDSD` кратна 12 у всех 127, звуков 2 395. Шанс — в стотысячных долях процента: 10 000 000
/// (100 %) стоит у всех 520 петель (`SOUN` с флагом 0x10), у случайных звуков 0…4 500 000, и их
/// сумма по региону около 100 % — это веса выбора, а не вероятность.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionSound {
    /// `SOUN`, `FormID` как в файле.
    pub sound: u32,
    /// 0x01 ясно, 0x02 облачно, 0x04 дождь, 0x08 снег — при какой погоде звучит.
    pub flags: u32,
    /// Шанс, стотысячные доли процента.
    pub chance: u32,
}

/// Шанс `RDSD`, равный 100 %.
pub const REGION_SOUND_CHANCE_FULL: u32 = 10_000_000;

/// Тип `RDAT`: погода.
const RDAT_WEATHER: u32 = 3;
/// Тип `RDAT`: имя на карте.
const RDAT_MAP: u32 = 4;
/// Тип `RDAT`: звуки.
const RDAT_SOUNDS: u32 = 7;

/// Регион.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub editor_id: String,
    /// `RCLR`: цвет на карте, RGBA.
    pub color: [u8; 4],
    /// `WNAM`: worldspace.
    pub world: Option<u32>,
    /// `RDMP`: имя на карте.
    pub map_name: Option<String>,
    pub weather: Option<RegionWeather>,
    /// `RDSD`: звуки региона.
    pub sounds: Vec<RegionSound>,
}

impl FromFields for Region {
    const SIG: Tag = *b"REGN";
    const NAME: &'static str = "REGN";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let mut map_name = None;
        let mut weather = None;
        let mut sounds = Vec::new();
        // Смысл поля данных задаёт ближайший `RDAT` перед ним.
        let mut current: Option<(u32, u8, u8)> = None;
        for (tag, raw) in fields.iter() {
            match &tag {
                b"RDAT" => {
                    if raw.len() < 6 {
                        return Err(Error::FieldSize {
                            record: "REGN",
                            field: tag,
                            expected: 8,
                            got: raw.len(),
                        });
                    }
                    current = Some((u32_at(raw, 0), raw[4], raw[5]));
                }
                b"RDMP" if matches!(current, Some((RDAT_MAP, ..))) => {
                    map_name = Some(crate::cp1252::decode_zstring(raw)).filter(|s| !s.is_empty());
                }
                b"RDWT" => {
                    let Some((RDAT_WEATHER, flags, priority)) = current else {
                        return Err(Error::BadValue {
                            record: "REGN",
                            field: tag,
                            detail: lang::tr(
                                "RDWT не после RDAT погоды",
                                "RDWT not after the weather RDAT",
                            )
                            .into(),
                        });
                    };
                    weather = Some(RegionWeather {
                        overrides: flags & 1 != 0,
                        priority,
                        weathers: chunks("REGN", tag, raw, 8)?
                            .map(|c| (u32_at(c, 0), u32_at(c, 4)))
                            .collect(),
                    });
                }
                b"RDSD" => {
                    if !matches!(current, Some((RDAT_SOUNDS, ..))) {
                        return Err(Error::BadValue {
                            record: "REGN",
                            field: tag,
                            detail: lang::tr(
                                "RDSD не после RDAT звуков",
                                "RDSD not after the sounds RDAT",
                            )
                            .into(),
                        });
                    }
                    sounds.extend(chunks("REGN", tag, raw, 12)?.map(|c| RegionSound {
                        sound: u32_at(c, 0),
                        flags: u32_at(c, 4),
                        chance: u32_at(c, 8),
                    }));
                }
                _ => {}
            }
        }
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            color: fields.opt_fixed::<4>(*b"RCLR")?.unwrap_or([0; 4]),
            world: fields.opt_u32(*b"WNAM")?,
            map_name,
            weather,
            sounds,
        })
    }
}

/// Классы погоды в `WTHR` `DATA`[11].
pub const WEATHER_PLEASANT: u8 = 0x01;
pub const WEATHER_CLOUDY: u8 = 0x02;
pub const WEATHER_RAINY: u8 = 0x04;
pub const WEATHER_SNOW: u8 = 0x08;

/// Погода. Из 15 байт `DATA` нужен только класс.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weather {
    pub editor_id: String,
    /// `DATA`[11]: 0x01 ясно, 0x02 облачно, 0x04 дождь, 0x08 снег; у погоды планов Обливиона 0,
    /// у `CamoranWeather` старшие биты мусорные (0xC1).
    pub classification: u8,
}

impl FromFields for Weather {
    const SIG: Tag = *b"WTHR";
    const NAME: &'static str = "WTHR";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            classification: fields.opt_fixed::<15>(*b"DATA")?.map_or(0, |d| d[11]),
        })
    }
}

/// Климат: список погоды с шансами.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Climate {
    pub editor_id: String,
    /// `WLST`: `WTHR` и шанс.
    pub weathers: Vec<(u32, i32)>,
}

impl FromFields for Climate {
    const SIG: Tag = *b"CLMT";
    const NAME: &'static str = "CLMT";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let weathers = match fields.first(*b"WLST") {
            None => Vec::new(),
            Some(raw) => chunks("CLMT", *b"WLST", raw, 8)?
                .map(|c| (u32_at(c, 0), u32_at(c, 4).cast_signed()))
                .collect(),
        };
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            weathers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::RecordHeader;
    use crate::synth;

    fn fields_of<T: FromFields>(fields: &[Vec<u8>]) -> Result<T> {
        let data = fields.concat();
        T::from_fields(&Fields::parse(&data, T::NAME)?)
    }

    fn rdat(kind: u32, flags: u8, priority: u8) -> Vec<u8> {
        let mut d = kind.to_le_bytes().to_vec();
        d.extend([flags, priority, 0, 0]);
        synth::field(b"RDAT", &d)
    }

    fn pairs(list: &[(u32, u32)]) -> Vec<u8> {
        list.iter()
            .flat_map(|(a, b)| [a.to_le_bytes(), b.to_le_bytes()].concat())
            .collect()
    }

    #[test]
    fn cell_reads_grid_regions_and_water() {
        let mut xclc = (-3i32).to_le_bytes().to_vec();
        xclc.extend(7i32.to_le_bytes());
        xclc.extend(0u32.to_le_bytes());
        let cell: Cell = fields_of(&[
            synth::field(b"DATA", &[2]),
            synth::field(b"XCLC", &xclc),
            synth::field(b"XCLR", &pairs(&[(0x10, 0x11)])),
            synth::field(b"XCLW", &3000f32.to_le_bytes()),
        ])
        .unwrap();
        assert_eq!(cell.grid, Some((-3, 7)));
        assert_eq!(cell.regions, [0x10, 0x11]);
        assert_eq!(cell.water_height, Some(3000.0));
        assert_eq!(cell.editor_id, None);
    }

    #[test]
    fn interior_cell_reads_flags_and_light() {
        let mut xcll = vec![10, 20, 30, 0, 40, 50, 60, 0, 1, 2, 3, 0];
        xcll.extend(64f32.to_le_bytes());
        xcll.extend(2250f32.to_le_bytes());
        xcll.extend([0; 16]);
        let cell: Cell = fields_of(&[
            synth::field(b"EDID", b"SENSBooksofBliss\0"),
            synth::field(b"FULL", b"Books of Bliss\0"),
            synth::field(b"DATA", &[0x03]),
            synth::field(b"XCLL", &xcll),
        ])
        .unwrap();
        assert_eq!(cell.flags & CELL_HAS_WATER, CELL_HAS_WATER);
        assert_eq!(cell.grid, None);
        let light = cell.lighting.unwrap();
        assert_eq!(light.ambient, [10, 20, 30, 0]);
        assert_eq!(light.directional, [40, 50, 60, 0]);
        assert_eq!(light.fog_color, [1, 2, 3, 0]);
        assert_eq!((light.fog_near, light.fog_far), (64.0, 2250.0));
        assert!(
            fields_of::<Cell>(&[synth::field(b"XCLL", &[0; 12])]).is_err(),
            "короткий XCLL — ошибка"
        );
    }

    #[test]
    fn lock_of_both_sizes_without_key_and_broken() {
        let refr = |xloc: &[u8]| {
            fields_of::<Placement>(&[
                synth::field(b"NAME", &1u32.to_le_bytes()),
                synth::field(b"DATA", &[0; 24]),
                synth::field(b"XLOC", xloc),
            ])
        };
        // 16 байт: флаги — последние четыре.
        let mut long = vec![100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        long.extend(4u32.to_le_bytes());
        assert_eq!(
            refr(&long).unwrap().lock,
            Some(Lock {
                level: 100,
                key: None,
                leveled: true
            })
        );
        let short = refr(&[0; 12]).unwrap().lock.unwrap();
        assert_eq!((short.level, short.key, short.leveled), (0, None, false));
        assert!(refr(&[0; 8]).is_err());
    }

    #[test]
    fn achr_and_acre_are_placements_and_other_records_are_not() {
        let record = |tag: &[u8; 4]| Record {
            header: RecordHeader {
                tag: *tag,
                flags: 0,
                form_id: 0x77,
                vcs1: 0,
                form_version: 0,
                vcs2: 0,
            },
            data: [
                synth::field(b"NAME", &0x5003u32.to_le_bytes()),
                synth::field(b"DATA", &[0; 24]),
            ]
            .concat(),
        };
        let npc = Placement::from_record(&record(b"ACHR")).unwrap();
        assert_eq!((npc.form_id, npc.base), (0x77, 0x5003));
        let creature = Placement::from_record(&record(b"ACRE")).unwrap();
        assert_eq!((creature.form_id, creature.base), (0x77, 0x5003));
        assert!(matches!(
            Placement::from_record(&record(b"STAT")),
            Err(Error::WrongRecord { .. })
        ));
    }

    #[test]
    fn placement_takes_flags_and_id_from_the_header() {
        let mut data = Vec::new();
        for v in [1.0f32, -2.0, 3.0, 0.1, 0.2, 0.3] {
            data.extend(v.to_le_bytes());
        }
        let mut xesp = 0x55u32.to_le_bytes().to_vec();
        xesp.extend(1u32.to_le_bytes());
        let record = Record {
            header: RecordHeader {
                tag: *b"REFR",
                flags: REF_INITIALLY_DISABLED,
                form_id: 0xABCD,
                vcs1: 0,
                form_version: 0,
                vcs2: 0,
            },
            data: [
                synth::field(b"NAME", &0x1234u32.to_le_bytes()),
                synth::field(
                    b"XTEL",
                    &[
                        0x77u32,
                        10f32.to_bits(),
                        20f32.to_bits(),
                        30f32.to_bits(),
                        0,
                        0,
                        1f32.to_bits(),
                    ]
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<u8>>(),
                ),
                synth::field(b"XESP", &xesp),
                synth::field(b"DATA", &data),
                synth::field(b"XSCL", &2.5f32.to_le_bytes()),
                synth::field(b"XSED", &[3]),
                // Уровень 40, мусор, ключ 0x99, флаги с «растущим».
                synth::field(b"XLOC", &[40, 0x1C, 0x40, 0, 0x99, 0, 0, 0, 4, 0, 0, 0]),
            ]
            .concat(),
        };
        let p = Placement::from_record(&record).unwrap();
        assert_eq!((p.form_id, p.base), (0xABCD, 0x1234));
        assert_eq!(p.tree_seed, Some(3));
        assert_eq!(
            p.lock,
            Some(Lock {
                level: 40,
                key: Some(0x99),
                leveled: true
            })
        );
        assert_eq!(p.position, [1.0, -2.0, 3.0]);
        assert_eq!(p.rotation, [0.1, 0.2, 0.3]);
        assert_eq!(p.scale, Some(2.5));
        assert!(p.initially_disabled && !p.map_marker);
        assert_eq!(
            p.teleport,
            Some(Teleport {
                door: 0x77,
                position: [10.0, 20.0, 30.0],
                rotation: [0.0, 0.0, 1.0]
            })
        );
        assert_eq!(
            p.enable_parent,
            Some(EnableParent {
                parent: 0x55,
                inverted: true
            })
        );
    }

    #[test]
    fn region_pairs_data_with_its_rdat() {
        let region: Region = fields_of(&[
            synth::field(b"EDID", b"R\0"),
            synth::field(b"RCLR", &[1, 2, 3, 0]),
            synth::field(b"WNAM", &0x9F18u32.to_le_bytes()),
            rdat(2, 0, 50),
            rdat(4, 0, 50),
            synth::field(b"RDMP", b"Fellmoor\0"),
            rdat(3, 1, 70),
            synth::field(b"RDWT", &pairs(&[(0xA, 40), (0xB, 60)])),
        ])
        .unwrap();
        assert_eq!(region.map_name.as_deref(), Some("Fellmoor"));
        assert_eq!(region.color, [1, 2, 3, 0]);
        assert_eq!(
            region.weather,
            Some(RegionWeather {
                overrides: true,
                priority: 70,
                weathers: vec![(0xA, 40), (0xB, 60)]
            })
        );
        assert!(region.sounds.is_empty());
        let sound = |id: u32, flags: u32, chance: u32| {
            [id.to_le_bytes(), flags.to_le_bytes(), chance.to_le_bytes()].concat()
        };
        let with_sounds: Region = fields_of(&[
            synth::field(b"EDID", b"R\0"),
            rdat(7, 0, 50),
            synth::field(
                b"RDSD",
                &[
                    sound(0x10, 15, REGION_SOUND_CHANCE_FULL),
                    sound(0x11, 11, 300_000),
                ]
                .concat(),
            ),
        ])
        .unwrap();
        assert_eq!(
            with_sounds.sounds,
            [
                RegionSound {
                    sound: 0x10,
                    flags: 15,
                    chance: REGION_SOUND_CHANCE_FULL
                },
                RegionSound {
                    sound: 0x11,
                    flags: 11,
                    chance: 300_000
                }
            ]
        );
        assert!(
            fields_of::<Region>(&[
                synth::field(b"EDID", b"R\0"),
                rdat(3, 0, 50),
                synth::field(b"RDSD", &sound(1, 0, 0)),
            ])
            .is_err(),
            "RDSD после RDAT погоды"
        );
        // RDWT после RDAT не погоды — ошибка, а не молчаливая погода.
        assert!(
            fields_of::<Region>(&[
                synth::field(b"EDID", b"R\0"),
                rdat(4, 0, 50),
                synth::field(b"RDWT", &pairs(&[(1, 1)])),
            ])
            .is_err()
        );
    }

    #[test]
    fn weather_class_and_climate_list() {
        let mut data = [0xFFu8; 15];
        data[11] = WEATHER_RAINY;
        let w: Weather = fields_of(&[
            synth::field(b"EDID", b"Rain\0"),
            synth::field(b"DATA", &data),
        ])
        .unwrap();
        assert_eq!(w.classification, WEATHER_RAINY);
        let c: Climate = fields_of(&[
            synth::field(b"EDID", b"C\0"),
            synth::field(b"WLST", &pairs(&[(7, 30), (8, 70)])),
        ])
        .unwrap();
        assert_eq!(c.weathers, [(7, 30), (8, 70)]);
        assert!(
            fields_of::<Climate>(&[
                synth::field(b"EDID", b"C\0"),
                synth::field(b"WLST", &[0; 12]),
            ])
            .is_err()
        );
    }

    #[test]
    fn worldspace_and_land_texture() {
        let w: Worldspace = fields_of(&[
            synth::field(b"EDID", b"SEWorld\0"),
            synth::field(b"FULL", b"Realm\0"),
            synth::field(b"CNAM", &0x44F57u32.to_le_bytes()),
        ])
        .unwrap();
        assert_eq!((w.parent, w.climate), (None, Some(0x44F57)));
        assert_eq!((w.map_texture, w.map_size), (None, None));
        let t: LandTexture = fields_of(&[
            synth::field(b"EDID", b"Moss\0"),
            synth::field(b"ICON", b"Dementia\\Moss.dds\0"),
        ])
        .unwrap();
        assert_eq!(t.texture.as_deref(), Some("Dementia\\Moss.dds"));
    }

    /// `MNAM` как у `SEWorld`: 1024×1024, углы (−32, 33) и (31, −29).
    fn mnam(width: u32, height: u32, corners: [i16; 4]) -> Vec<u8> {
        let mut raw = width.to_le_bytes().to_vec();
        raw.extend(height.to_le_bytes());
        raw.extend(corners.iter().flat_map(|c| c.to_le_bytes()));
        raw
    }

    #[test]
    fn worldspace_reads_its_map_sheet() {
        let world = |mnam: &[u8]| {
            fields_of::<Worldspace>(&[
                synth::field(b"EDID", b"SEWorld\0"),
                synth::field(b"ICON", b"Menus\\Map\\world\\TES4SEmap.dds\0"),
                synth::field(b"MNAM", mnam),
            ])
        };
        let w = world(&mnam(1024, 1024, [-32, 33, 31, -29])).unwrap();
        assert_eq!(
            w.map_texture.as_deref(),
            Some("Menus\\Map\\world\\TES4SEmap.dds")
        );
        assert_eq!(
            w.map_size,
            Some(MapSize {
                width: 1024,
                height: 1024,
                north_west: (-32, 33),
                south_east: (31, -29),
            })
        );
        // Тестовые миры Oblivion держат `MNAM` из нулей: карты у них нет.
        assert_eq!(world(&[0; 16]).unwrap().map_size, None);
        assert!(matches!(
            world(&[0; 12]),
            Err(Error::FieldSize {
                field: [b'M', b'N', b'A', b'M'],
                got: 12,
                ..
            })
        ));
    }
}
