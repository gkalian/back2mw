//! Exterior world records except `LAND`: `WRLD`, `CELL`, `REFR`, `LTEX`, `REGN`, `WTHR`, `CLMT`.
//!
//! The layouts come from OpenMW `components/esm4/load{wrld,cell,refr,regn,ltex}.cpp`; OpenMW does
//! not parse `WTHR`, `CLMT` and `RDWT`, so their layout was measured on `Oblivion.esm` (2026-09-25,
//! `docs/en/formats/tes4.md`, "Exterior world").

use crate::container::{Record, Tag};
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// A reference record header flag: initially disabled.
pub const REF_INITIALLY_DISABLED: u32 = 0x0000_0800;

fn u32_at(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(
        raw[at..at + 4]
            .try_into()
            .expect("the slice is exactly 4 bytes"),
    )
}

fn f32_at(raw: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(
        raw[at..at + 4]
            .try_into()
            .expect("the slice is exactly 4 bytes"),
    )
}

/// An array of fixed-length entries in one field.
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
    /// `WNAM`: the parent whose landscape the worldspace uses (cities inside `Tamriel`).
    pub parent: Option<u32>,
    /// `CNAM`: the climate — the weather where a region does not set it.
    pub climate: Option<u32>,
    /// `ICON`: the painted world map sheet, a path relative to `Textures\`
    /// (`Menus\Map\world\TES4SEmap.dds`). In `Oblivion.esm` five worlds have it: `Tamriel`,
    /// `SEWorld` and three more Shivering Isles worlds with the same sheet (2026-10-02).
    pub map_texture: Option<String>,
    /// `MNAM`: which cells the sheet covers. Test worlds have a field of only zeros — that is
    /// `None`.
    pub map_size: Option<MapSize>,
}

/// `WRLD` `MNAM`, 16 bytes: the map sheet size in pixels and the cells of its corners. How the
/// sheet lies on these cells — `docs/en/formats/tes4.md`, "World map".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapSize {
    pub width: u32,
    pub height: u32,
    /// The north-western cell: (x, y).
    pub north_west: (i16, i16),
    /// The south-eastern cell: (x, y).
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

/// A cell. An exterior one has `XCLC`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub editor_id: Option<String>,
    pub name: Option<String>,
    /// `XCLC`: cell coordinates (4096 units); interiors and the worldspace's persistent cell do not
    /// have it.
    pub grid: Option<(i32, i32)>,
    /// `XCLR`: the regions the cell belongs to.
    pub regions: Vec<u32>,
    /// `XCLW`: its own water level; without it — the worldspace level.
    pub water_height: Option<f32>,
    /// `DATA`: 0x01 interior, 0x02 has water, 0x20 public place, 0x80 behaves like an exterior
    /// (UESP, `CELL`; OpenMW `loadcell.cpp`).
    pub flags: u8,
    /// `XCLL`: interior cell lighting.
    pub lighting: Option<CellLighting>,
}

/// Cell `DATA` flag: has water.
pub const CELL_HAS_WATER: u8 = 0x02;

/// Interior cell lighting (`XCLL`, 36 bytes in all 162 interiors of `SEWorld`, 2026-09-26). Colors
/// are R, G, B and an unused byte. The tail — the directional light's rotation and fade, the fog
/// clip distance — Morrowind cannot express.
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
            // 12 bytes — plus quadrant hiding flags, we do not need them.
            Some(raw) if raw.len() == 8 || raw.len() == 12 => Some((
                i32::from_le_bytes(raw[0..4].try_into().expect("4 bytes")),
                i32::from_le_bytes(raw[4..8].try_into().expect("4 bytes")),
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
                    ambient: raw[0..4].try_into().expect("4 bytes"),
                    directional: raw[4..8].try_into().expect("4 bytes"),
                    fog_color: raw[8..12].try_into().expect("4 bytes"),
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

/// The enable parent (`XESP`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnableParent {
    pub parent: u32,
    /// The state is opposite to the parent.
    pub inverted: bool,
}

/// Door destination (`XTEL`, 28 bytes: the `FormID` of the door at the other end, the position and
/// angles where the player appears — OpenMW `loadrefr.cpp`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Teleport {
    pub door: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
}

/// A lock (`XLOC`, 12 or 16 bytes): level (u8), 3 junk bytes, key `FormID`, flags — the last 4
/// bytes (UESP, "Mod File Format/REFR"; OpenMW `loadrefr.cpp` reads the same level and key).
/// `Oblivion.esm` has 2,866 locks: 2,841 of 12 bytes, 25 of 16 — in all 16-byte ones bytes 8 and 12
/// are zero, so the data cannot tell where their flags are (2026-09-27, a script over the file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lock {
    /// 0…100; 100 means it opens only with a key.
    pub level: u8,
    /// `KEYM`; `None` means no key.
    pub key: Option<u32>,
    /// Flag 0x4: the difficulty scales with the player's level.
    pub leveled: bool,
}

/// `XLOC` flag: the lock difficulty scales with the player's level.
pub const LOCK_LEVELED: u32 = 0x4;

/// A placement of an object (`REFR`), an NPC (`ACHR`) or a creature (`ACRE`).
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// `FormID` of the reference itself (from the header).
    pub form_id: u32,
    /// `NAME`: the base object.
    pub base: u32,
    /// `DATA`: position, world units.
    pub position: [f32; 3],
    /// `DATA`: angles, radians. The same `ESM::Position` structure as in TES3.
    pub rotation: [f32; 3],
    pub scale: Option<f32>,
    /// Header flag 0x800.
    pub initially_disabled: bool,
    pub enable_parent: Option<EnableParent>,
    /// `XTEL`: a teleport door.
    pub teleport: Option<Teleport>,
    /// `XMRK`: a map marker.
    pub map_marker: bool,
    /// `XSED`, the first byte: the tree seed number in the `SNAM` of its `TREE` (taken modulo the
    /// number of seeds).
    pub tree_seed: Option<u8>,
    /// `XLOC`: the lock of a door or container.
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

    /// The disabled flag and the `FormID` are in the record header, not in the fields. `ACHR` (NPC
    /// placement, T7.6) is read with the same spec: its `NAME`, `DATA`, `XESP`, `XSCL` are the
    /// same, and the fields `REFR` lacks (`XHRS`, `XMRC`, `XRGD`) are not used by the conversion.
    /// `ACRE` (creature placement, T11.4) likewise: `NAME`, a 24-byte `DATA`, `XSCL`, `XESP` (all
    /// 1,473 `ACRE` of `Oblivion.esm` are read, 2026-10-01, `real_creatures`).
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

/// A land texture.
#[derive(Debug, Clone, PartialEq)]
pub struct LandTexture {
    pub editor_id: String,
    /// `ICON`: a path relative to `Textures\Landscape\`.
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

/// Region weather: an `RDAT` of type 3 and the `RDWT` following it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionWeather {
    /// `RDAT` flag 0x01 — "override".
    pub overrides: bool,
    pub priority: u8,
    /// `WTHR` and a chance.
    pub weathers: Vec<(u32, u32)>,
}

/// A region sound: an `RDSD` element (12 bytes) after an `RDAT` of type 7 (T10.2).
///
/// Measured on `Oblivion.esm` (2026-09-28, `docs/en/formats/tes4.md`, "Region sounds"): the `RDSD`
/// length is a multiple of 12 in all 127, there are 2,395 sounds. The chance is in
/// hundred-thousandths of a percent: 10,000,000 (100%) is set on all 520 loops (`SOUN` with flag
/// 0x10), random sounds have 0…4,500,000, and their sum per region is about 100% — these are
/// selection weights, not probabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionSound {
    /// `SOUN`, `FormID` as in the file.
    pub sound: u32,
    /// 0x01 clear, 0x02 cloudy, 0x04 rain, 0x08 snow — in which weather it plays.
    pub flags: u32,
    /// Chance, hundred-thousandths of a percent.
    pub chance: u32,
}

/// An `RDSD` chance equal to 100%.
pub const REGION_SOUND_CHANCE_FULL: u32 = 10_000_000;

/// `RDAT` type: weather.
const RDAT_WEATHER: u32 = 3;
/// `RDAT` type: map name.
const RDAT_MAP: u32 = 4;
/// `RDAT` type: sounds.
const RDAT_SOUNDS: u32 = 7;

/// A region.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub editor_id: String,
    /// `RCLR`: map color, RGBA.
    pub color: [u8; 4],
    /// `WNAM`: worldspace.
    pub world: Option<u32>,
    /// `RDMP`: map name.
    pub map_name: Option<String>,
    pub weather: Option<RegionWeather>,
    /// `RDSD`: region sounds.
    pub sounds: Vec<RegionSound>,
}

impl FromFields for Region {
    const SIG: Tag = *b"REGN";
    const NAME: &'static str = "REGN";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let mut map_name = None;
        let mut weather = None;
        let mut sounds = Vec::new();
        // The meaning of a data field is set by the nearest `RDAT` before it.
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

/// Weather classes in `WTHR` `DATA`[11].
pub const WEATHER_PLEASANT: u8 = 0x01;
pub const WEATHER_CLOUDY: u8 = 0x02;
pub const WEATHER_RAINY: u8 = 0x04;
pub const WEATHER_SNOW: u8 = 0x08;

/// Weather. Of the 15 `DATA` bytes only the class is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weather {
    pub editor_id: String,
    /// `DATA`[11]: 0x01 clear, 0x02 cloudy, 0x04 rain, 0x08 snow; the weather of Oblivion's planes
    /// has 0, and `CamoranWeather` has junk in the high bits (0xC1).
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

/// A climate: a list of weathers with chances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Climate {
    pub editor_id: String,
    /// `WLST`: `WTHR` and a chance.
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
            "a short XCLL is an error"
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
        // 16 bytes: the flags are the last four.
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
                // Level 40, junk, key 0x99, flags with "scaling".
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
            "RDSD after a weather RDAT"
        );
        // RDWT after a non-weather RDAT is an error, not silently weather.
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

    /// `MNAM` as in `SEWorld`: 1024×1024, corners (−32, 33) and (31, −29).
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
        // Oblivion's test worlds keep an all-zero `MNAM`: they have no map.
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
