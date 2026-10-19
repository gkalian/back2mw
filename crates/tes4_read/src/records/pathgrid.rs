use crate::container::Tag;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// Сетка путей ячейки Oblivion.
///
/// Раскладка — замер по всем 8 228 `PGRD` `Oblivion.esm` (2026-09-27, `docs/ru/formats/tes4.md`,
/// «Сетки путей»): `DATA` — число точек (u16); `PGRP` — точки по 16 байт; `PGRR` — номера соседей
/// (u16) подряд, у каждой точки столько, сколько сказано в её `PGRP`, 0xFFFF — пустой слот
/// ([`EMPTY_LINK`]); `PGRI` — связи с точками
/// соседних ячеек по 16 байт. `PGAG` (битовая маска) и `PGRL` (связи точек с объектами) у
/// Morrowind аналога не имеют — не читаются.
#[derive(Debug, Clone, PartialEq)]
pub struct PathGrid {
    /// Точки в мировых координатах (у интерьера — в координатах ячейки).
    pub points: Vec<[f32; 3]>,
    /// Соседи каждой точки: `edges[i]` — номера точек этой же сетки.
    pub edges: Vec<Vec<u16>>,
    /// Связи с точками соседних ячеек.
    pub external: Vec<ExternalEdge>,
}

/// `PGRI`: своя точка и мировые координаты точки в соседней ячейке.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExternalEdge {
    pub point: u16,
    pub target: [f32; 3],
}

/// Пустой слот в `PGRR`: связи нет. Так OpenMW (`components/esm4/loadpgrd.cpp`: `endNode == -1`
/// пропускается); в `Oblivion.esm` это 92 115 из 2 887 675 слотов в 4 351 из 8 228 `PGRD`, других
/// номеров вне сетки нет, а без пустых слотов все связи записаны у обоих концов (2026-09-27).
pub const EMPTY_LINK: u16 = 0xFFFF;

fn size_error(field: Tag, expected: usize, got: usize) -> Error {
    Error::FieldSize {
        record: "PGRD",
        field,
        expected,
        got,
    }
}

fn bad_value(field: Tag, detail: String) -> Error {
    Error::BadValue {
        record: "PGRD",
        field,
        detail,
    }
}

fn f32_at(raw: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(raw[at..at + 4].try_into().expect("4 байта"))
}

fn u16_at(raw: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(raw[at..at + 2].try_into().expect("2 байта"))
}

impl FromFields for PathGrid {
    const SIG: Tag = *b"PGRD";
    const NAME: &'static str = "PGRD";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let count = usize::from(u16::from_le_bytes(fields.fixed::<2>(*b"DATA")?));

        let pgrp = fields.first(*b"PGRP").unwrap_or_default();
        if pgrp.len() != count * 16 {
            return Err(size_error(*b"PGRP", count * 16, pgrp.len()));
        }
        let points: Vec<[f32; 3]> = pgrp
            .chunks_exact(16)
            .map(|c| [f32_at(c, 0), f32_at(c, 4), f32_at(c, 8)])
            .collect();
        let degrees: Vec<usize> = pgrp.chunks_exact(16).map(|c| usize::from(c[12])).collect();

        let pgrr = fields.first(*b"PGRR").unwrap_or_default();
        let total: usize = degrees.iter().sum();
        if pgrr.len() != total * 2 {
            return Err(size_error(*b"PGRR", total * 2, pgrr.len()));
        }
        let mut neighbours = pgrr.chunks_exact(2).map(|c| u16_at(c, 0));
        let mut edges = Vec::with_capacity(count);
        for degree in degrees {
            let list: Vec<u16> = neighbours
                .by_ref()
                .take(degree)
                .filter(|&n| n != EMPTY_LINK)
                .collect();
            if let Some(bad) = list.iter().find(|&&n| usize::from(n) >= count) {
                return Err(bad_value(
                    *b"PGRR",
                    lang::tr!(
                        "сосед {bad} при {count} точках",
                        "neighbour {bad} with {count} points"
                    ),
                ));
            }
            edges.push(list);
        }

        let mut external = Vec::new();
        for raw in fields.all(*b"PGRI") {
            if !raw.len().is_multiple_of(16) {
                return Err(size_error(*b"PGRI", raw.len() / 16 * 16 + 16, raw.len()));
            }
            for c in raw.chunks_exact(16) {
                let point = u16_at(c, 0);
                if usize::from(point) >= count {
                    return Err(bad_value(
                        *b"PGRI",
                        lang::tr!(
                            "точка {point} при {count} точках",
                            "point {point} with {count} points"
                        ),
                    ));
                }
                external.push(ExternalEdge {
                    point,
                    target: [f32_at(c, 4), f32_at(c, 8), f32_at(c, 12)],
                });
            }
        }

        Ok(Self {
            points,
            edges,
            external,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    fn parse(fields: &[Vec<u8>]) -> Result<PathGrid> {
        let data = fields.concat();
        PathGrid::from_fields(&Fields::parse(&data, "PGRD")?)
    }

    fn pgrp(points: &[([f32; 3], u8)]) -> Vec<u8> {
        let mut d = Vec::new();
        for (xyz, degree) in points {
            for v in xyz {
                d.extend(v.to_le_bytes());
            }
            // Три байта заполнения — не нули, как в реальных данных: читать их нельзя.
            d.extend([*degree, 0x98, 0x43, 0x07]);
        }
        synth::field(b"PGRP", &d)
    }

    fn u16s(tag: &Tag, values: &[u16]) -> Vec<u8> {
        let d: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        synth::field(tag, &d)
    }

    fn pgri(edges: &[(u16, [f32; 3])]) -> Vec<u8> {
        let mut d = Vec::new();
        for (point, xyz) in edges {
            d.extend(point.to_le_bytes());
            d.extend([0xAA, 0xBB]);
            for v in xyz {
                d.extend(v.to_le_bytes());
            }
        }
        synth::field(b"PGRI", &d)
    }

    #[test]
    fn points_neighbours_and_external_edges_are_read() {
        let grid = parse(&[
            u16s(b"DATA", &[3]),
            pgrp(&[
                ([1.0, 2.0, 3.0], 2),
                ([4.5, 5.0, 6.0], 0),
                ([7.0, 8.0, -9.0], 2),
            ]),
            synth::field(b"PGAG", &[0xFF]),
            // Пустой слот 0xFFFF у третьей точки выпадает.
            u16s(b"PGRR", &[1, 2, EMPTY_LINK, 0]),
            pgri(&[(2, [4096.0, 8192.0, 10.0])]),
            synth::field(b"PGRL", &[0; 8]),
        ])
        .unwrap();
        assert_eq!(grid.points[1], [4.5, 5.0, 6.0]);
        assert_eq!(grid.points[2][2], -9.0);
        assert_eq!(grid.edges, [vec![1, 2], vec![], vec![0]]);
        assert_eq!(
            grid.external,
            [ExternalEdge {
                point: 2,
                target: [4096.0, 8192.0, 10.0]
            }]
        );
    }

    #[test]
    fn grid_without_connections_has_no_pgrr() {
        let grid = parse(&[u16s(b"DATA", &[1]), pgrp(&[([0.0; 3], 0)])]).unwrap();
        assert_eq!(grid.edges, [Vec::<u16>::new()]);
        assert!(grid.external.is_empty());
    }

    #[test]
    fn inconsistent_sizes_and_indices_are_errors() {
        // Точек в DATA больше, чем в PGRP.
        assert!(matches!(
            parse(&[u16s(b"DATA", &[2]), pgrp(&[([0.0; 3], 0)])]),
            Err(Error::FieldSize { got: 16, .. })
        ));
        // Связей в PGRP две, а в PGRR одна.
        assert!(matches!(
            parse(&[
                u16s(b"DATA", &[1]),
                pgrp(&[([0.0; 3], 2)]),
                u16s(b"PGRR", &[0])
            ]),
            Err(Error::FieldSize { got: 2, .. })
        ));
        // Сосед за пределами сетки.
        assert!(matches!(
            parse(&[
                u16s(b"DATA", &[1]),
                pgrp(&[([0.0; 3], 1)]),
                u16s(b"PGRR", &[1])
            ]),
            Err(Error::BadValue { .. })
        ));
        assert!(matches!(
            parse(&[
                u16s(b"DATA", &[1]),
                pgrp(&[([0.0; 3], 0)]),
                pgri(&[(1, [0.0; 3])])
            ]),
            Err(Error::BadValue { .. })
        ));
        assert!(matches!(
            parse(&[pgrp(&[([0.0; 3], 0)])]),
            Err(Error::MissingField { .. })
        ));
    }
}
