//! FaceGen Oblivion (фаза 8): морфы геометрии `.egm`, текстуры `.egt` и выражения `.tri`.
//!
//! Раскладки сверены на `Oblivion - Meshes.bsa` 2026-09-27:
//! размеры файлов сходятся с формулой до байта, и ридер требует того же — файл обязан
//! кончиться ровно на последнем байте разобранного. Непонятные части `.tri` (помеченные вершины и
//! поверхности, четырёхугольники) — ошибка, если их число не ноль: пропускать их, не зная
//! раскладки, нельзя.

use std::fmt;

pub const EGM_MAGIC: &[u8; 8] = b"FREGM002";
pub const EGT_MAGIC: &[u8; 8] = b"FREGT003";
pub const TRI_MAGIC: &[u8; 8] = b"FRTRI003";

/// Заголовок всех трёх форматов — 64 байта.
const HEADER_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaceGenError {
    /// Данные кончились раньше, чем требует раскладка.
    Truncated { at: usize, need: usize },
    /// Не та сигнатура.
    BadMagic {
        expected: &'static str,
        found: String,
    },
    /// Раскладка не сходится: лишние байты, неизвестный вариант, числа вне смысла.
    Structure { at: usize, what: String },
}

impl fmt::Display for FaceGenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { at, need } => {
                lang::trw!(
                    f,
                    "смещение {at}: данные кончились, нужно ещё {need} байт",
                    "offset {at}: out of data, {need} more bytes needed"
                )
            }
            Self::BadMagic { expected, found } => {
                lang::trw!(
                    f,
                    "сигнатура {found:?}, а ждали {expected}",
                    "signature {found:?}, expected {expected}"
                )
            }
            Self::Structure { at, what } => {
                lang::trw!(f, "смещение {at}: {what}", "offset {at}: {what}")
            }
        }
    }
}

impl std::error::Error for FaceGenError {}

type Result<T> = std::result::Result<T, FaceGenError>;

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.data.len());
        let Some(end) = end else {
            return Err(FaceGenError::Truncated {
                at: self.at,
                need: n - (self.data.len() - self.at),
            });
        };
        let out = &self.data[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 байта"),
        ))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().expect("4 байта"),
        ))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(
            self.take(4)?.try_into().expect("4 байта"),
        ))
    }

    /// Число из файла как `usize` — для счётчиков.
    fn count(&mut self) -> Result<usize> {
        let at = self.at;
        usize::try_from(self.u32()?).map_err(|_| FaceGenError::Structure {
            at,
            what: lang::tr(
                "счётчик не влезает в usize",
                "the counter does not fit into usize",
            )
            .into(),
        })
    }

    fn magic(&mut self, expected: &'static [u8; 8]) -> Result<()> {
        let found = self.take(8)?;
        if found != expected {
            return Err(FaceGenError::BadMagic {
                expected: std::str::from_utf8(expected).expect("ASCII"),
                found: String::from_utf8_lossy(found).into_owned(),
            });
        }
        Ok(())
    }

    fn i16x3(&mut self, n: usize) -> Result<Vec<[i16; 3]>> {
        let raw = self.take(n.checked_mul(6).ok_or_else(|| self.overflow())?)?;
        Ok(raw
            .chunks_exact(6)
            .map(|c| [0, 2, 4].map(|k| i16::from_le_bytes([c[k], c[k + 1]])))
            .collect())
    }

    fn f32x3(&mut self, n: usize) -> Result<Vec<[f32; 3]>> {
        let raw = self.take(n.checked_mul(12).ok_or_else(|| self.overflow())?)?;
        Ok(raw
            .chunks_exact(12)
            .map(|c| {
                [0, 4, 8].map(|k| f32::from_le_bytes(c[k..k + 4].try_into().expect("4 байта")))
            })
            .collect())
    }

    fn overflow(&self) -> FaceGenError {
        FaceGenError::Structure {
            at: self.at,
            what: lang::tr(
                "размер массива переполняет usize",
                "the array size overflows usize",
            )
            .into(),
        }
    }

    /// Файл обязан кончиться здесь.
    fn end(&self) -> Result<()> {
        if self.at != self.data.len() {
            return Err(FaceGenError::Structure {
                at: self.at,
                what: lang::tr!(
                    "лишние {} байт в конце",
                    "{} extra bytes at the end",
                    self.data.len() - self.at
                ),
            });
        }
        Ok(())
    }
}

/// Морф геометрии: смещение вершины `i` — `scale · deltas[i]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Morph {
    pub scale: f32,
    pub deltas: Vec<[i16; 3]>,
}

/// `.egm`: заголовок (сигнатура, число вершин, симметричных и асимметричных морфов, версия базиса,
/// 40 байт нулей) и морфы — сначала симметричные, по порядку `FGGS`, затем асимметричные (`FGGA`).
///
/// Вершины — это вершины `.tri` того же меша: сначала вершины меша (порядок NIF и `.tri` совпадает у
/// всех 85 пар архива), затем, если у меша есть статические морфы `.tri`, — их вершины. Замер
/// 2026-09-27 (`mesh_convert/tests/real_faces.rs`): у 131 `.egm` вершин столько же, сколько в NIF,
/// у 9 — NIF плюс статические (`HeadHuman`: 1 275 + 461 = 1 736), иначе — у одного
/// (`earswoodelf_50`: 125 против 150, статических морфов нет).
#[derive(Debug, Clone, PartialEq)]
pub struct Egm {
    pub vertices: usize,
    pub basis_version: u32,
    pub symmetric: Vec<Morph>,
    pub asymmetric: Vec<Morph>,
}

impl Egm {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(data);
        c.magic(EGM_MAGIC)?;
        let vertices = c.count()?;
        let sym = c.count()?;
        let asym = c.count()?;
        let basis_version = c.u32()?;
        c.take(HEADER_LEN - c.at)?;
        let mut read = |n: usize| -> Result<Vec<Morph>> {
            (0..n)
                .map(|_| {
                    Ok(Morph {
                        scale: c.f32()?,
                        deltas: c.i16x3(vertices)?,
                    })
                })
                .collect()
        };
        let symmetric = read(sym)?;
        let asymmetric = read(asym)?;
        c.end()?;
        Ok(Self {
            vertices,
            basis_version,
            symmetric,
            asymmetric,
        })
    }

    /// Вершины лица: `base[i] + Σ coef · scale · delta[i]` по симметричным (`FGGS`) и
    /// асимметричным (`FGGA`) морфам. Коэффициентов меньше, чем морфов, — недостающие 0. `base` —
    /// первые вершины файла (вершины меша или меш вместе с вершинами статических морфов); вершин
    /// в `base` больше, чем в файле, — ошибка (файл не от этого меша).
    pub fn apply(&self, base: &[[f32; 3]], sym: &[f32], asym: &[f32]) -> Result<Vec<[f32; 3]>> {
        if base.len() > self.vertices {
            return Err(FaceGenError::Structure {
                at: 0,
                what: lang::tr!(
                    "у меша {} вершин, а у .egm {}",
                    "the mesh has {} vertices, the .egm has {}",
                    base.len(),
                    self.vertices
                ),
            });
        }
        let mut out = base.to_vec();
        for (morphs, coefs) in [(&self.symmetric, sym), (&self.asymmetric, asym)] {
            for (m, &k) in morphs.iter().zip(coefs) {
                let w = k * m.scale;
                if w == 0.0 {
                    continue;
                }
                for (v, d) in out.iter_mut().zip(&m.deltas) {
                    for a in 0..3 {
                        v[a] += w * f32::from(d[a]);
                    }
                }
            }
        }
        Ok(out)
    }
}

/// Морф текстуры: `scale` и 3 канала × строки × столбцы байтов со знаком, **плоскостями**: сначала
/// весь первый канал, затем второй, третий. Замер на `HeadHuman.egt` (2026-09-28): соседние байты
/// различаются в среднем на 0.5–0.6, байты через 3 — на 1.4–1.7 (вперемешку было бы наоборот), и
/// средние по плоскостям разные (63, 81, 101), а по «перемешанным» каналам — равны. Что первая
/// плоскость — красный, а не синий, — не проверено (проверка оттенка в игре).
#[derive(Debug, Clone, PartialEq)]
pub struct TextureMorph {
    pub scale: f32,
    pub data: Vec<i8>,
}

/// `.egt`: сигнатура, строки, столбцы, симметричных и асимметричных морфов, версия, нули до 64
/// байт; морфы по порядку `FGTS`.
#[derive(Debug, Clone, PartialEq)]
pub struct Egt {
    pub rows: usize,
    pub columns: usize,
    pub basis_version: u32,
    pub symmetric: Vec<TextureMorph>,
    pub asymmetric: Vec<TextureMorph>,
}

impl Egt {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(data);
        c.magic(EGT_MAGIC)?;
        let rows = c.count()?;
        let columns = c.count()?;
        let sym = c.count()?;
        let asym = c.count()?;
        let basis_version = c.u32()?;
        c.take(HEADER_LEN - c.at)?;
        let size = rows
            .checked_mul(columns)
            .and_then(|n| n.checked_mul(3))
            .ok_or_else(|| c.overflow())?;
        let mut read = |n: usize| -> Result<Vec<TextureMorph>> {
            (0..n)
                .map(|_| {
                    Ok(TextureMorph {
                        scale: c.f32()?,
                        data: c
                            .take(size)?
                            .iter()
                            .map(|&b| i8::from_le_bytes([b]))
                            .collect(),
                    })
                })
                .collect()
        };
        let symmetric = read(sym)?;
        let asymmetric = read(asym)?;
        c.end()?;
        Ok(Self {
            rows,
            columns,
            basis_version,
            symmetric,
            asymmetric,
        })
    }

    /// Карта детализации лица (T8.8): `128 + ½ · Σ coef · scale · морф` по каналам RGB, строки
    /// сверху вниз (в файле — снизу вверх), `columns × rows`. `coefs` — `FGTS` NPC **плюс** `FGTS`
    /// расы: у NPC хранится добавка к лицу расы.
    ///
    /// Сверено 2026-09-30 с готовыми картами `textures\faces\oblivion.esm\<FormID>_0.dds` архива
    /// текстур на всех 1 977 NPC, у которых они есть (разовый тест по архивам): наклон через 128 —
    /// 0.503, корреляция по NPC — медиана 0.961 (10 % хуже 0.914), средняя ошибка при ½ — 2.2
    /// уровня (шум сжатия DXT). С абсолютными коэффициентами NPC (без расы) корреляция 0.56, с
    /// разностью — 0.17, без переворота строк — 0.35. Лицо в игре — текстура расы, умноженная на
    /// карту (`face_texture::compose`).
    pub fn detail(&self, coefs: &[f32]) -> Vec<[u8; 3]> {
        let plane = self.rows * self.columns;
        let mut sum = vec![[0.0f32; 3]; plane];
        for (m, &k) in self.symmetric.iter().zip(coefs) {
            let w = k * m.scale;
            for (c, channel) in m.data.chunks_exact(plane).enumerate().take(3) {
                for (s, &v) in sum.iter_mut().zip(channel) {
                    s[c] += w * f32::from(v);
                }
            }
        }
        let mut out = vec![[0u8; 3]; plane];
        for row in 0..self.rows {
            let from = (self.rows - 1 - row) * self.columns;
            for col in 0..self.columns {
                out[row * self.columns + col] = sum[from + col].map(|s| {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "зажато в 0…255 и округлено"
                    )]
                    let level = (128.0 + 0.5 * s).round().clamp(0.0, 255.0) as u8;
                    level
                });
            }
        }
        out
    }
}

/// Разностный морф `.tri` (эмоции, фонемы): смещение каждой вершины — `scale · deltas[i]`.
#[derive(Debug, Clone, PartialEq)]
pub struct TriDiffMorph {
    pub name: String,
    pub scale: f32,
    pub deltas: Vec<[i16; 3]>,
}

/// Статический морф `.tri` (моргание, взгляд): новые позиции вершин `indices`.
#[derive(Debug, Clone, PartialEq)]
pub struct TriStatMorph {
    pub name: String,
    pub indices: Vec<u32>,
    pub positions: Vec<[f32; 3]>,
}

/// `.tri`: заголовок (сигнатура; вершин, треугольников, четырёхугольников, помеченных вершин и
/// поверхностей, текстурных координат, флаг расширения, разностных и статических морфов, вершин
/// статических морфов; 16 байт нулей), вершины (свои и статических морфов подряд), треугольники,
/// текстурные координаты и их треугольники, морфы. Вершины `.tri` — без дублей на швах UV, их
/// столько же и в том же порядке, что у NIF (все 85 пар архива, 2026-09-27); у `.egm` к ним
/// добавлены вершины статических морфов (см. [`Egm`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Tri {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[i32; 3]>,
    pub diff_morphs: Vec<TriDiffMorph>,
    pub stat_morphs: Vec<TriStatMorph>,
}

impl Tri {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(data);
        c.magic(TRI_MAGIC)?;
        let vertices = c.count()?;
        let triangles = c.count()?;
        let quads = c.count()?;
        let labelled_vertices = c.count()?;
        let labelled_surface = c.count()?;
        let texcoords = c.count()?;
        let at = c.at;
        let extension = c.u32()?;
        let diff = c.count()?;
        let stat = c.count()?;
        let stat_vertices = c.count()?;
        c.take(HEADER_LEN - c.at)?;
        if quads + labelled_vertices + labelled_surface != 0 {
            return Err(FaceGenError::Structure {
                at: 12,
                what: lang::tr!(
                    "четырёхугольников {quads}, помеченных вершин {labelled_vertices} и \
                     поверхностей {labelled_surface}: раскладка не известна",
                    "{quads} quads, {labelled_vertices} labelled vertices and \
                     {labelled_surface} labelled surfaces: unknown layout"
                ),
            });
        }
        if extension != 1 {
            return Err(FaceGenError::Structure {
                at,
                what: lang::tr!(
                    "флаг расширения {extension}, известен только 1 (морфы с именами)",
                    "extension flag {extension}, only 1 is known (named morphs)"
                ),
            });
        }
        let all = c.f32x3(vertices + stat_vertices)?;
        let tris = (0..triangles)
            .map(|_| Ok([c.i32()?, c.i32()?, c.i32()?]))
            .collect::<Result<Vec<_>>>()?;
        if texcoords > 0 {
            c.take(texcoords.checked_mul(8).ok_or_else(|| c.overflow())?)?;
            c.take(triangles.checked_mul(12).ok_or_else(|| c.overflow())?)?;
        }
        let name = |c: &mut Cursor<'_>| -> Result<String> {
            let len = c.count()?;
            let raw = c.take(len)?;
            Ok(String::from_utf8_lossy(raw.strip_suffix(&[0]).unwrap_or(raw)).into_owned())
        };
        let diff_morphs = (0..diff)
            .map(|_| {
                Ok(TriDiffMorph {
                    name: name(&mut c)?,
                    scale: c.f32()?,
                    deltas: c.i16x3(vertices)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut next = vertices;
        let mut stat_morphs = Vec::with_capacity(stat);
        for _ in 0..stat {
            let name = name(&mut c)?;
            let n = c.count()?;
            let indices = (0..n).map(|_| c.u32()).collect::<Result<Vec<_>>>()?;
            let Some(positions) = all.get(next..next + n) else {
                return Err(FaceGenError::Structure {
                    at: c.at,
                    what: lang::tr!(
                        "вершин статических морфов {stat_vertices}, а нужно больше",
                        "static morph vertices {stat_vertices}, more are needed"
                    ),
                });
            };
            next += n;
            if let Some(bad) = indices
                .iter()
                .find(|&&i| usize::try_from(i).is_ok_and(|i| i >= vertices))
            {
                return Err(FaceGenError::Structure {
                    at: c.at,
                    what: lang::tr!(
                        "морф {name}: вершина {bad} при {vertices} вершинах",
                        "morph {name}: vertex {bad} with {vertices} vertices"
                    ),
                });
            }
            stat_morphs.push(TriStatMorph {
                name,
                indices,
                positions: positions.to_vec(),
            });
        }
        c.end()?;
        Ok(Self {
            vertices: all[..vertices].to_vec(),
            triangles: tris,
            diff_morphs,
            stat_morphs,
        })
    }
}

#[cfg(test)]
mod tests;
