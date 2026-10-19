use crate::container::Tag;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// Вершин на сторону ячейки Oblivion (`ESM4::Land::sVertsPerSide`).
pub const LAND_SIDE: usize = 33;

/// Вершин в ячейке: 33×33.
pub const LAND_VERTS: usize = LAND_SIDE * LAND_SIDE;

/// Отсчётов прозрачности на сторону квадранта (`VTXT`, позиции 0…288).
pub const OPACITY_SIDE: usize = 17;

/// Ландшафт одной ячейки Oblivion.
///
/// Раскладка — `components/esm4/loadland.{hpp,cpp}` OpenMW и замер по `SEWorld`
/// (`docs/ru/formats/tes4.md`, «LAND»): `VHGT` 1096 байт, `VNML`/`VCLR` по 3267, `BTXT`/`ATXT` по 8,
/// `VTXT` кратно 8. Сетки 33×33 — строками, как в TES3.
#[derive(Debug, Clone, PartialEq)]
pub struct Land {
    /// `DATA`: какие части есть (0x1 высоты и нормали, 0x2 цвета, 0x4/0x8/0x10 текстуры).
    pub flags: u32,
    pub heights: Option<SourceHeightField>,
    /// `VNML`: нормаль на вершину, 1089 штук.
    pub normals: Option<Vec<[i8; 3]>>,
    /// `VCLR`: цвет на вершину, 1089 штук.
    pub colors: Option<Vec<[u8; 3]>>,
    /// Текстуры по квадрантам **в нумерации файла**. По OpenMW: 0 — юго-запад, 1 — юго-восток,
    /// 2 — северо-запад, 3 — северо-восток; сторонам света их сопоставляет `transform`.
    pub quadrants: [LandQuadrant; 4],
}

/// `VHGT`: смещение и дельты, кодирование то же, что у TES3.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceHeightField {
    pub offset: f32,
    /// 1089 дельт строками.
    pub deltas: Vec<i8>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LandQuadrant {
    /// `BTXT`: `FormID` базовой `LTEX`; `None` — текстура по умолчанию.
    pub base: Option<u32>,
    /// `ATXT` + `VTXT` в порядке файла.
    pub layers: Vec<LandLayer>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LandLayer {
    /// `FormID` `LTEX`; 0 — текстура по умолчанию.
    pub texture: u32,
    /// Номер слоя в квадранте, 0…7.
    pub layer: u16,
    /// Позиция в сетке 17×17 (`y * 17 + x`) и прозрачность. `ATXT` без `VTXT` даёт пустой список.
    pub opacity: Vec<(u16, f32)>,
}

fn size_error(field: Tag, expected: usize, got: usize) -> Error {
    Error::FieldSize {
        record: "LAND",
        field,
        expected,
        got,
    }
}

/// `BTXT`/`ATXT`: `FormID`, квадрант (u8), байт, u16 (у `ATXT` — номер слоя).
fn texture_header(raw: &[u8], field: Tag) -> Result<(u32, usize, u16)> {
    if raw.len() != 8 {
        return Err(size_error(field, 8, raw.len()));
    }
    let quadrant = usize::from(raw[4]);
    if quadrant >= 4 {
        return Err(Error::BadValue {
            record: "LAND",
            field,
            detail: lang::tr!(
                "квадрант {quadrant}, а их четыре",
                "quadrant {quadrant}, there are only four"
            ),
        });
    }
    Ok((
        u32::from_le_bytes(raw[0..4].try_into().expect("4 байта")),
        quadrant,
        u16::from_le_bytes([raw[6], raw[7]]),
    ))
}

impl FromFields for Land {
    const SIG: Tag = *b"LAND";
    const NAME: &'static str = "LAND";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let heights = match fields.first(*b"VHGT") {
            None => None,
            Some(raw) => {
                if raw.len() != 4 + LAND_VERTS + 3 {
                    return Err(size_error(*b"VHGT", 4 + LAND_VERTS + 3, raw.len()));
                }
                Some(SourceHeightField {
                    offset: f32::from_le_bytes(raw[0..4].try_into().expect("4 байта")),
                    deltas: raw[4..4 + LAND_VERTS]
                        .iter()
                        .map(|&b| i8::from_le_bytes([b]))
                        .collect(),
                })
            }
        };
        let triples = |tag: Tag| -> Result<Option<Vec<[u8; 3]>>> {
            match fields.first(tag) {
                None => Ok(None),
                Some(raw) if raw.len() == LAND_VERTS * 3 => Ok(Some(
                    raw.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
                )),
                Some(raw) => Err(size_error(tag, LAND_VERTS * 3, raw.len())),
            }
        };
        let normals = triples(*b"VNML")?.map(|v| {
            v.into_iter()
                .map(|[x, y, z]| [x, y, z].map(|b| i8::from_le_bytes([b])))
                .collect()
        });
        let colors = triples(*b"VCLR")?;

        let mut quadrants: [LandQuadrant; 4] = Default::default();
        // `VTXT` относится к ближайшему `ATXT` перед ним; `ATXT` без `VTXT` — слой без
        // отсчётов (так же поступает OpenMW).
        let mut pending: Option<(usize, LandLayer)> = None;
        for (tag, raw) in fields.iter() {
            match &tag {
                b"BTXT" => {
                    let (form_id, quadrant, _) = texture_header(raw, tag)?;
                    quadrants[quadrant].base = (form_id != 0).then_some(form_id);
                }
                b"ATXT" => {
                    if let Some((q, layer)) = pending.take() {
                        quadrants[q].layers.push(layer);
                    }
                    let (texture, quadrant, layer) = texture_header(raw, tag)?;
                    pending = Some((
                        quadrant,
                        LandLayer {
                            texture,
                            layer,
                            opacity: Vec::new(),
                        },
                    ));
                }
                b"VTXT" => {
                    if !raw.len().is_multiple_of(8) {
                        return Err(size_error(tag, raw.len() / 8 * 8 + 8, raw.len()));
                    }
                    let Some((q, mut layer)) = pending.take() else {
                        return Err(Error::BadValue {
                            record: "LAND",
                            field: tag,
                            detail: lang::tr(
                                "VTXT без предшествующего ATXT",
                                "VTXT without a preceding ATXT",
                            )
                            .into(),
                        });
                    };
                    for c in raw.chunks_exact(8) {
                        let position = u16::from_le_bytes([c[0], c[1]]);
                        if usize::from(position) >= OPACITY_SIDE * OPACITY_SIDE {
                            return Err(Error::BadValue {
                                record: "LAND",
                                field: tag,
                                detail: lang::tr!(
                                    "позиция {position} вне сетки 17×17",
                                    "position {position} is outside the 17×17 grid"
                                ),
                            });
                        }
                        let opacity = f32::from_le_bytes(c[4..8].try_into().expect("4 байта"));
                        layer.opacity.push((position, opacity));
                    }
                    quadrants[q].layers.push(layer);
                }
                _ => {}
            }
        }
        if let Some((q, layer)) = pending {
            quadrants[q].layers.push(layer);
        }

        Ok(Self {
            flags: fields
                .opt_fixed::<4>(*b"DATA")?
                .map_or(0, u32::from_le_bytes),
            heights,
            normals,
            colors,
            quadrants,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    fn parse(fields: &[Vec<u8>]) -> Result<Land> {
        let data = fields.concat();
        Land::from_fields(&Fields::parse(&data, "LAND")?)
    }

    fn txt(tag: &Tag, form_id: u32, quadrant: u8, layer: u16) -> Vec<u8> {
        let mut d = form_id.to_le_bytes().to_vec();
        d.extend([quadrant, 0]);
        d.extend(layer.to_le_bytes());
        synth::field(tag, &d)
    }

    fn vtxt(points: &[(u16, f32)]) -> Vec<u8> {
        let mut d = Vec::new();
        for (p, o) in points {
            d.extend(p.to_le_bytes());
            d.extend([0, 0]);
            d.extend(o.to_le_bytes());
        }
        synth::field(b"VTXT", &d)
    }

    #[test]
    fn heights_normals_and_colors_are_read() {
        let mut vhgt = 12.5f32.to_le_bytes().to_vec();
        vhgt.extend((0..LAND_VERTS).map(|i| u8::try_from(i % 7).unwrap()));
        vhgt.extend([0; 3]);
        let mut vnml = vec![0u8; LAND_VERTS * 3];
        vnml[3..6].copy_from_slice(&[0xFF, 2, 127]);
        let land = parse(&[
            synth::field(b"DATA", &0x1Fu32.to_le_bytes()),
            synth::field(b"VNML", &vnml),
            synth::field(b"VHGT", &vhgt),
            synth::field(b"VCLR", &[200; LAND_VERTS * 3]),
        ])
        .unwrap();
        assert_eq!(land.flags, 0x1F);
        let h = land.heights.unwrap();
        assert_eq!(h.offset, 12.5);
        assert_eq!((h.deltas.len(), h.deltas[8]), (LAND_VERTS, 1));
        assert_eq!(land.normals.unwrap()[1], [-1, 2, 127]);
        assert_eq!(land.colors.unwrap()[1088], [200; 3]);
    }

    #[test]
    fn layers_go_to_their_quadrants_and_vtxt_follows_its_atxt() {
        let land = parse(&[
            txt(b"BTXT", 0x10, 2, 0),
            txt(b"BTXT", 0, 3, 0),
            txt(b"ATXT", 0x20, 1, 0),
            vtxt(&[(0, 0.5), (288, 1.0)]),
            // ATXT без VTXT: слой без отсчётов, следующий ATXT его не теряет.
            txt(b"ATXT", 0x30, 1, 1),
            txt(b"ATXT", 0x40, 0, 0),
            vtxt(&[(17, 0.25)]),
        ])
        .unwrap();
        assert_eq!(land.quadrants[2].base, Some(0x10));
        assert_eq!(land.quadrants[3].base, None, "FormID 0 — умолчание");
        let q1 = &land.quadrants[1].layers;
        assert_eq!(q1.len(), 2);
        assert_eq!(
            (q1[0].texture, &q1[0].opacity[..]),
            (0x20, &[(0, 0.5), (288, 1.0)][..])
        );
        assert_eq!(
            (q1[1].texture, q1[1].layer, q1[1].opacity.len()),
            (0x30, 1, 0)
        );
        assert_eq!(land.quadrants[0].layers[0].opacity, [(17, 0.25)]);
    }

    #[test]
    fn broken_texture_fields_are_errors() {
        assert!(matches!(
            parse(&[txt(b"BTXT", 1, 4, 0)]),
            Err(Error::BadValue { .. })
        ));
        assert!(matches!(
            parse(&[vtxt(&[(1, 1.0)])]),
            Err(Error::BadValue { .. })
        ));
        assert!(matches!(
            parse(&[txt(b"ATXT", 1, 0, 0), vtxt(&[(289, 1.0)])]),
            Err(Error::BadValue { .. })
        ));
        assert!(matches!(
            parse(&[synth::field(b"VHGT", &[0; 100])]),
            Err(Error::FieldSize { got: 100, .. })
        ));
    }
}
