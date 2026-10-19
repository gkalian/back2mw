//! Внешний мир: `LTEX`, `REGN`, внешние `CELL` со ссылками, `LAND`, `PGRD`.
//!
//! Модель — простые структуры; решения о значениях (что куда, какой погоде сколько) принимает
//! `transform`. Здесь только перевод в типы `tes3::esp`.

use tes3::esp::{
    AtmosphereData, Cell, CellData, CellFlags, FixedString, Landscape, LandscapeFlags,
    LandscapeTexture, ObjectFlags, PathGrid as EspPathGrid, PathGridData, PathGridPoint,
    Reference as EspReference, Region as EspRegion, TES3Object, TextureIndices, TravelDestination,
    VertexColors, VertexHeights, VertexNormals, WeatherChances as EspWeather, WorldMapData,
};

/// Вершин на сторону `LAND`.
pub const LAND_SIDE: usize = 65;

/// Текстура земли. `index` — номер в `VTEX` минус один (0 в `VTEX` — текстура по умолчанию).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandTexture {
    pub id: String,
    pub index: u32,
    /// Путь относительно `Textures\`.
    pub texture: String,
}

/// Шансы погоды Morrowind в порядке `WEAT`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WeatherChances {
    pub clear: u8,
    pub cloudy: u8,
    pub foggy: u8,
    pub overcast: u8,
    pub rain: u8,
    pub thunder: u8,
    pub ash: u8,
    pub blight: u8,
    pub snow: u8,
    pub blizzard: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub id: String,
    pub name: String,
    pub weather: WeatherChances,
    /// RGB.
    pub map_color: [u8; 3],
    /// `SNAM`: ID звука и шанс, % (T10.2). Движок раз в несколько секунд выбирает один звук из
    /// max(сумма, 100).
    pub sounds: Vec<(String, u8)>,
}

/// Ссылка на объект в ячейке.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    /// Номер ссылки в плагине (`FRMR`), уникальный на весь плагин: по нему сохранения узнают
    /// ссылку.
    pub index: u32,
    pub id: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    /// `None` — 1.0. Движок держит 0.5…2.0; зажимать и сообщать о потере — дело `transform`.
    pub scale: Option<f32>,
    /// Назначение двери (`DODT` + `DNAM`); `None` — дверь не телепорт.
    pub destination: Option<DoorDestination>,
    /// Замок двери или контейнера (`FLTV` + `KNAM`); `None` — не заперто.
    pub lock: Option<Lock>,
    /// Постоянная ссылка: пишется до `NAM0`. Нужна актёрам: в `Morrowind.esm` все 3 043 ссылки
    /// на `NPC_` и 863 на `CREA` постоянные, а NPC из временных ссылок движок ставит без
    /// «живой» части (mobile actor) — они не ходят, и разговор с ними роняет игру в функции
    /// активации, `Morrowind.exe+0xEAB3F` (игра, 2026-09-30, `SEWorld`). Прочее — временное.
    pub persistent: bool,
}

/// Замок ссылки. В плагине ссылка заперта, если уровень больше 0 или есть ключ (OpenMW
/// `components/esm3/cellref.cpp`); уровень 0 без ключа — замок открыт.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lock {
    /// `FLTV`: 1…100 у ванили (`Morrowind.esm`, 2026-09-27).
    pub level: i32,
    /// `KNAM`: ID предмета-ключа.
    pub key: Option<String>,
}

/// Куда ведёт дверь.
#[derive(Debug, Clone, PartialEq)]
pub struct DoorDestination {
    /// Где появится игрок и куда он смотрит.
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    /// Имя внутренней ячейки; `None` — внешний мир (ячейка — по позиции).
    pub cell: Option<String>,
}

/// Внутренняя ячейка. Имя — её ID: по нему ищут двери и `COC`, поэтому оно уникально.
#[derive(Debug, Clone, PartialEq)]
pub struct InteriorCell {
    pub name: String,
    /// `AMBI`: цвета — R, G, B, 0.
    pub ambient: [u8; 4],
    pub sunlight: [u8; 4],
    pub fog_color: [u8; 4],
    pub fog_density: f32,
    /// Уровень воды; `None` — воды нет.
    pub water_height: Option<f32>,
    pub references: Vec<Reference>,
}

/// Внешняя ячейка.
#[derive(Debug, Clone, PartialEq)]
pub struct ExteriorCell {
    pub grid: (i32, i32),
    /// Пусто — безымянная ячейка (на карте имя региона).
    pub name: String,
    pub region: Option<String>,
    pub references: Vec<Reference>,
}

/// Ландшафт ячейки. Сетки — `[y][x]`, `y` растёт на север.
#[derive(Debug, Clone, PartialEq)]
pub struct Land {
    pub grid: (i32, i32),
    /// `VHGT`: высота первой вершины / 8 и дельты.
    pub height_offset: f32,
    pub height_deltas: Box<[[i8; LAND_SIDE]; LAND_SIDE]>,
    pub normals: Box<[[[i8; 3]; LAND_SIDE]; LAND_SIDE]>,
    /// `None` — без `VCLR`.
    pub colors: Option<Box<[[[u8; 3]; LAND_SIDE]; LAND_SIDE]>>,
    /// `VTEX` **в дисковом порядке** (после `transform::land::transpose_texture_data`).
    pub textures: Box<[[u16; 16]; 16]>,
    pub world_map: [[i8; 9]; 9],
}

/// Сетка путей ячейки.
///
/// Точек не больше `u16::MAX`, соседей у точки не больше 255 — это держит `transform`
/// (`transform::pathgrid`), здесь поля формата уже такой ширины.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathGrid {
    /// Сетка внешней ячейки; у внутренней — (0, 0), как в ванили.
    pub grid: (i32, i32),
    /// `NAME`: имя ячейки; у безымянной внешней — имя региона, как в `Morrowind.esm`.
    pub cell: String,
    /// У внешней ячейки — от её юго-западного угла, у внутренней — как есть.
    pub points: Vec<[i32; 3]>,
    /// `edges[i]` — соседи точки `i`. Ребро записывается у обоих концов, как в ванили.
    pub edges: Vec<Vec<u16>>,
}

/// `Granularity` — шаг автогенерации сетки в CS; движку не нужен. 1024 — у 737 из 1 194
/// `PGRD` `Morrowind.esm` (2026-09-27, `docs/ru/formats/tes3.md`, «Сетки путей»).
const PATH_GRID_GRANULARITY: u16 = 1024;

pub(crate) fn land_texture(t: LandTexture) -> TES3Object {
    TES3Object::LandscapeTexture(LandscapeTexture {
        flags: ObjectFlags::default(),
        id: t.id,
        index: t.index,
        file_name: t.texture,
    })
}

pub(crate) fn region(r: Region) -> TES3Object {
    let w = r.weather;
    let [red, green, blue] = r.map_color;
    TES3Object::Region(EspRegion {
        flags: ObjectFlags::default(),
        id: r.id,
        name: r.name,
        weather_chances: EspWeather {
            clear: w.clear,
            cloudy: w.cloudy,
            foggy: w.foggy,
            overcast: w.overcast,
            rain: w.rain,
            thunder: w.thunder,
            ash: w.ash,
            blight: w.blight,
            snow: w.snow,
            blizzard: w.blizzard,
        },
        sleep_creature: String::new(),
        map_color: [red, green, blue, 0],
        sounds: r
            .sounds
            .into_iter()
            .map(|(id, chance)| (FixedString(id), chance))
            .collect(),
    })
}

/// Ссылки ячейки с ключами словаря. Тип словаря — библиотечный (`hashbrown`), поэтому
/// собирают его прямо в поле.
fn references(list: Vec<Reference>) -> impl Iterator<Item = ((u32, u32), EspReference)> {
    list.into_iter().map(|r| {
        (
            (0, r.index),
            EspReference {
                mast_index: 0,
                refr_index: r.index,
                id: r.id,
                temporary: !r.persistent,
                translation: r.position,
                rotation: r.rotation,
                scale: r.scale,
                destination: r.destination.map(|d| TravelDestination {
                    translation: d.position,
                    rotation: d.rotation,
                    // Пустое имя библиотека не пишет: `DNAM` нет — внешний мир.
                    cell: d.cell.unwrap_or_default(),
                }),
                lock_level: r.lock.as_ref().map(|l| l.level),
                key: r.lock.and_then(|l| l.key),
                ..EspReference::default()
            },
        )
    })
}

pub(crate) fn cell(c: ExteriorCell) -> TES3Object {
    TES3Object::Cell(Cell {
        flags: ObjectFlags::default(),
        name: c.name,
        data: CellData {
            flags: CellFlags::default(),
            grid: c.grid,
        },
        region: c.region,
        map_color: None,
        water_height: None,
        atmosphere_data: None,
        references: references(c.references).collect(),
    })
}

pub(crate) fn interior_cell(c: InteriorCell) -> TES3Object {
    let mut flags = CellFlags::IS_INTERIOR;
    if c.water_height.is_some() {
        flags |= CellFlags::HAS_WATER;
    }
    TES3Object::Cell(Cell {
        flags: ObjectFlags::default(),
        name: c.name,
        // Сетка у внутренней ячейки не используется; ваниль пишет туда что попало.
        data: CellData {
            flags,
            grid: (0, 0),
        },
        region: None,
        map_color: None,
        water_height: c.water_height,
        atmosphere_data: Some(AtmosphereData {
            ambient_color: c.ambient,
            sunlight_color: c.sunlight,
            fog_color: c.fog_color,
            fog_density: c.fog_density,
        }),
        references: references(c.references).collect(),
    })
}

pub(crate) fn path_grid(g: PathGrid) -> TES3Object {
    let points = g
        .edges
        .iter()
        .zip(&g.points)
        .map(|(edges, &location)| PathGridPoint {
            location,
            // Решение владельца 2026-09-27: как у 89 % точек ванили.
            auto_generated: 1,
            connection_count: u8::try_from(edges.len())
                .expect("transform::pathgrid режет число соседей до 255"),
        })
        .collect();
    TES3Object::PathGrid(EspPathGrid {
        flags: ObjectFlags::default(),
        cell: g.cell,
        data: PathGridData {
            grid: g.grid,
            granularity: PATH_GRID_GRANULARITY,
            point_count: u16::try_from(g.points.len())
                .expect("transform::pathgrid отказывает сетке больше u16::MAX точек"),
        },
        points,
        connections: g.edges.into_iter().flatten().map(u32::from).collect(),
    })
}

pub(crate) fn land(l: Land) -> TES3Object {
    let mut flags = LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS | LandscapeFlags::USES_TEXTURES;
    let colors = match l.colors {
        Some(data) => {
            flags |= LandscapeFlags::USES_VERTEX_COLORS;
            VertexColors { data }
        }
        None => VertexColors::default(),
    };
    TES3Object::Landscape(Landscape {
        flags: ObjectFlags::default(),
        grid: l.grid,
        landscape_flags: flags,
        vertex_normals: VertexNormals { data: l.normals },
        vertex_heights: VertexHeights {
            offset: l.height_offset,
            data: l.height_deltas,
        },
        world_map_data: WorldMapData {
            data: Box::new(l.world_map),
        },
        vertex_colors: colors,
        texture_indices: TextureIndices { data: l.textures },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PluginBuilder;
    use tes3::esp::Plugin;

    #[test]
    fn world_records_read_back() {
        let mut b = PluginBuilder::new("back2mw", "");
        b.add_land_texture(LandTexture {
            id: "ob_Moss_1a".into(),
            index: 0,
            texture: r"ob\moss_0123456789.dds".into(),
        });
        b.add_region(Region {
            id: "ob_Mania_1b".into(),
            name: "Realm of Sheogorath".into(),
            weather: WeatherChances {
                clear: 50,
                foggy: 10,
                cloudy: 40,
                ..WeatherChances::default()
            },
            map_color: [136, 0, 0],
            sounds: vec![("ob_Bird_1c".into(), 3), ("obr1_Bird_1c".into(), 2)],
        });
        b.add_cell(ExteriorCell {
            grid: (70, -3),
            name: "Fellmoor".into(),
            region: Some("ob_Mania_1b".into()),
            references: vec![
                Reference {
                    index: 7,
                    id: "ob_Rock_2".into(),
                    position: [573_440.0, -24_000.0, 100.0],
                    rotation: [0.0, 0.1, 3.0],
                    scale: Some(1.5),
                    destination: Some(DoorDestination {
                        position: [1.0, 2.0, 3.0],
                        rotation: [0.0, 0.0, 1.5],
                        cell: Some("Books of Bliss".into()),
                    }),
                    lock: Some(Lock {
                        level: 55,
                        key: Some("ob_SEKey_1f".into()),
                    }),
                    persistent: false,
                },
                Reference {
                    index: 8,
                    id: "ob_Tree_3".into(),
                    position: [573_500.0, -24_100.0, 90.0],
                    rotation: [0.0; 3],
                    scale: None,
                    destination: None,
                    lock: None,
                    persistent: true,
                },
            ],
        });
        let mut deltas = Box::new([[0i8; LAND_SIDE]; LAND_SIDE]);
        deltas[3][4] = -5;
        let mut textures = Box::new([[0u16; 16]; 16]);
        textures[1][2] = 1;
        b.add_land(Land {
            grid: (70, -3),
            height_offset: 12.5,
            height_deltas: deltas,
            normals: Box::new([[[0, 0, 127]; LAND_SIDE]; LAND_SIDE]),
            colors: None,
            textures,
            world_map: [[3; 9]; 9],
        });
        let mut p = Plugin::new();
        p.load_bytes(&b.to_bytes().unwrap()).unwrap();

        let mut seen = 0;
        for o in &p.objects {
            match o {
                TES3Object::LandscapeTexture(t) => {
                    assert_eq!(
                        (t.index, t.file_name.as_str()),
                        (0, r"ob\moss_0123456789.dds")
                    );
                    seen += 1;
                }
                TES3Object::Region(r) => {
                    let w = &r.weather_chances;
                    assert_eq!((w.clear, w.cloudy, w.foggy, w.snow), (50, 40, 10, 0));
                    assert_eq!(r.map_color, [136, 0, 0, 0]);
                    let sounds: Vec<(&str, u8)> =
                        r.sounds.iter().map(|(s, c)| (s.as_str(), *c)).collect();
                    assert_eq!(sounds, [("ob_Bird_1c", 3), ("obr1_Bird_1c", 2)]);
                    seen += 1;
                }
                TES3Object::Cell(c) => {
                    assert_eq!(c.data.grid, (70, -3));
                    assert!(!c.data.flags.contains(CellFlags::IS_INTERIOR));
                    assert_eq!(c.region.as_deref(), Some("ob_Mania_1b"));
                    assert_eq!(c.name, "Fellmoor");
                    let r = &c.references[&(0, 7)];
                    assert_eq!(r.id, "ob_Rock_2");
                    assert_eq!(r.translation, [573_440.0, -24_000.0, 100.0]);
                    assert_eq!(r.scale, Some(1.5));
                    assert_eq!(
                        (r.lock_level, r.key.as_deref()),
                        (Some(55), Some("ob_SEKey_1f"))
                    );
                    assert_eq!(c.references[&(0, 8)].lock_level, None);
                    // Дверь-телепорт (7) библиотека пишет постоянной сама; 8 — по нашему флагу.
                    assert!(
                        c.references[&(0, 8)].persistent(),
                        "ссылка актёра — до NAM0"
                    );
                    assert!(c.references.contains_key(&(0, 8)));
                    seen += 1;
                }
                TES3Object::Landscape(l) => {
                    assert_eq!(l.grid, (70, -3));
                    assert_eq!(l.vertex_heights.offset, 12.5);
                    assert_eq!(l.vertex_heights.data[3][4], -5);
                    assert_eq!(l.texture_indices.data[1][2], 1);
                    assert_eq!(l.world_map_data.data[8][8], 3);
                    assert!(
                        !l.landscape_flags
                            .contains(LandscapeFlags::USES_VERTEX_COLORS)
                    );
                    seen += 1;
                }
                _ => {}
            }
        }
        assert_eq!(seen, 4);
    }

    #[test]
    fn path_grid_reads_back_with_vanilla_fields() {
        let mut b = PluginBuilder::new("back2mw", "");
        b.add_path_grid(PathGrid {
            grid: (70, -3),
            cell: "Mania Region".into(),
            points: vec![[10, 20, 30], [8000, 8100, -5], [0, 0, 0]],
            edges: vec![vec![1], vec![0, 2], vec![1]],
        });
        let mut p = Plugin::new();
        p.load_bytes(&b.to_bytes().unwrap()).unwrap();
        let g = p
            .objects
            .iter()
            .find_map(|o| match o {
                TES3Object::PathGrid(g) => Some(g),
                _ => None,
            })
            .unwrap();
        assert_eq!(g.cell, "Mania Region");
        assert_eq!(
            (g.data.grid, g.data.granularity, g.data.point_count),
            ((70, -3), 1024, 3)
        );
        assert_eq!(g.points[1].location, [8000, 8100, -5]);
        assert_eq!(
            g.points
                .iter()
                .map(|p| (p.auto_generated, p.connection_count))
                .collect::<Vec<_>>(),
            [(1, 1), (1, 2), (1, 1)]
        );
        assert_eq!(g.connections, [1, 0, 2, 1]);
    }

    #[test]
    fn interior_cell_with_light_water_and_doors_reads_back() {
        let door = |index: u32, cell: Option<&str>| Reference {
            index,
            id: "ob_Door_1".into(),
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: None,
            destination: Some(DoorDestination {
                position: [10.0, 20.0, 30.0],
                rotation: [0.0, 0.0, 3.0],
                cell: cell.map(str::to_owned),
            }),
            lock: None,
            persistent: false,
        };
        let mut b = PluginBuilder::new("back2mw", "");
        b.add_interior(InteriorCell {
            name: "Books of Bliss".into(),
            ambient: [10, 20, 30, 0],
            sunlight: [40, 50, 60, 0],
            fog_color: [1, 2, 3, 0],
            fog_density: 0.75,
            water_height: Some(-120.0),
            references: vec![door(1, None), door(2, Some("Books of Bliss Upstairs"))],
        });
        let mut p = Plugin::new();
        p.load_bytes(&b.to_bytes().unwrap()).unwrap();
        let c = p
            .objects
            .iter()
            .find_map(|o| match o {
                TES3Object::Cell(c) => Some(c),
                _ => None,
            })
            .unwrap();
        assert_eq!(c.name, "Books of Bliss");
        assert!(
            c.data
                .flags
                .contains(CellFlags::IS_INTERIOR | CellFlags::HAS_WATER)
        );
        assert_eq!(c.water_height, Some(-120.0));
        let a = c.atmosphere_data.as_ref().unwrap();
        assert_eq!(
            (
                a.ambient_color,
                a.sunlight_color,
                a.fog_color,
                a.fog_density
            ),
            ([10, 20, 30, 0], [40, 50, 60, 0], [1, 2, 3, 0], 0.75)
        );
        // Во внешний мир — без DNAM, в интерьер — с именем ячейки.
        let outside = c.references[&(0, 1)].destination.as_ref().unwrap();
        assert_eq!(
            (outside.translation, outside.cell.as_str()),
            ([10.0, 20.0, 30.0], "")
        );
        let inside = c.references[&(0, 2)].destination.as_ref().unwrap();
        assert_eq!(inside.cell, "Books of Bliss Upstairs");
    }
}
