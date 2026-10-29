//! Oblivion FaceGen (phase 8): `.egm` geometry morphs, `.egt` textures and `.tri` expressions.
//!
//! The layouts were checked on `Oblivion - Meshes.bsa` on 2026-09-27: file sizes match the formula
//! to the byte, and the reader requires the same — a file must end exactly at the last parsed byte.
//! Unclear parts of `.tri` (labelled vertices and surfaces, quads) are an error if their count is
//! not zero: skipping them without knowing the layout is not possible.

use std::fmt;

pub const EGM_MAGIC: &[u8; 8] = b"FREGM002";
pub const EGT_MAGIC: &[u8; 8] = b"FREGT003";
pub const TRI_MAGIC: &[u8; 8] = b"FRTRI003";

/// The header of all three formats is 64 bytes.
const HEADER_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaceGenError {
    /// The data ended earlier than the layout requires.
    Truncated { at: usize, need: usize },
    /// Wrong signature.
    BadMagic {
        expected: &'static str,
        found: String,
    },
    /// The layout does not add up: extra bytes, an unknown variant, numbers out of range.
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
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    /// A number from the file as `usize` — for counts.
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
                [0, 4, 8].map(|k| f32::from_le_bytes(c[k..k + 4].try_into().expect("4 bytes")))
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

    /// The file must end here.
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

/// A geometry morph: vertex `i` is offset by `scale · deltas[i]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Morph {
    pub scale: f32,
    pub deltas: Vec<[i16; 3]>,
}

/// `.egm`: the header (signature, vertex count, counts of symmetric and asymmetric morphs, basis
/// version, 40 zero bytes) and the morphs — first the symmetric ones, in `FGGS` order, then the
/// asymmetric ones (`FGGA`).
///
/// The vertices are the `.tri` vertices of the same mesh: first the mesh vertices (the NIF and
/// `.tri` order matches in all 85 pairs of the archive), then, if the mesh has static `.tri`
/// morphs, their vertices. Measured 2026-09-27 (`mesh_convert/tests/real_faces.rs`): 131 `.egm`
/// files have as many vertices as the NIF, 9 have the NIF plus the static ones (`HeadHuman`: 1,275 +
/// 461 = 1,736), one differs (`earswoodelf_50`: 125 versus 150, no static morphs).
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

    /// Face vertices: `base[i] + Σ coef · scale · delta[i]` over the symmetric (`FGGS`) and
    /// asymmetric (`FGGA`) morphs. If there are fewer coefficients than morphs, the missing ones
    /// are 0. `base` is the first vertices of the file (the mesh vertices or the mesh together with
    /// the static morph vertices); more vertices in `base` than in the file is an error (the file
    /// is not for this mesh).
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

/// A texture morph: `scale` and 3 channels × rows × columns of signed bytes, **in planes**: first
/// the whole first channel, then the second, the third. Measured on `HeadHuman.egt` (2026-09-28):
/// neighboring bytes differ by 0.5–0.6 on average, bytes 3 apart by 1.4–1.7 (interleaved it would
/// be the other way round), and the plane means differ (63, 81, 101), while the "interleaved"
/// channel means are equal. That the first plane is red rather than blue is not verified (a hue
/// check in the game).
#[derive(Debug, Clone, PartialEq)]
pub struct TextureMorph {
    pub scale: f32,
    pub data: Vec<i8>,
}

/// `.egt`: signature, rows, columns, counts of symmetric and asymmetric morphs, version, zeros up
/// to 64 bytes; the morphs in `FGTS` order.
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

    /// Face detail map (T8.8): `128 + ½ · Σ coef · scale · morph` per RGB channel, rows top to
    /// bottom (bottom to top in the file), `columns × rows`. `coefs` is the NPC's `FGTS` **plus**
    /// the race's `FGTS`: an NPC stores an addition to the race face.
    ///
    /// Checked 2026-09-30 against the ready-made maps `textures\faces\oblivion.esm\<FormID>_0.dds`
    /// of the texture archive on all 1,977 NPCs that have them (a one-off test over the archives):
    /// the slope through 128 is 0.503, the per-NPC correlation median is 0.961 (10% are worse than
    /// 0.914), the mean error with ½ is 2.2 levels (DXT compression noise). With the NPC's absolute
    /// coefficients (without the race) the correlation is 0.56, with the difference 0.17, without
    /// flipping rows 0.35. The face in the game is the race texture multiplied by the map
    /// (`face_texture::compose`).
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
                        reason = "clamped to 0…255 and rounded"
                    )]
                    let level = (128.0 + 0.5 * s).round().clamp(0.0, 255.0) as u8;
                    level
                });
            }
        }
        out
    }
}

/// A `.tri` difference morph (emotions, phonemes): each vertex is offset by `scale · deltas[i]`.
#[derive(Debug, Clone, PartialEq)]
pub struct TriDiffMorph {
    pub name: String,
    pub scale: f32,
    pub deltas: Vec<[i16; 3]>,
}

/// A `.tri` static morph (blinking, gaze): new positions of the vertices `indices`.
#[derive(Debug, Clone, PartialEq)]
pub struct TriStatMorph {
    pub name: String,
    pub indices: Vec<u32>,
    pub positions: Vec<[f32; 3]>,
}

/// `.tri`: the header (signature; counts of vertices, triangles, quads, labelled vertices and
/// surfaces, texture coordinates, the extension flag, counts of difference and static morphs,
/// static morph vertices; 16 zero bytes), the vertices (its own and the static morphs' in a row),
/// triangles, texture coordinates and their triangles, morphs. `.tri` vertices have no duplicates
/// on UV seams, there are as many of them and in the same order as in the NIF (all 85 pairs of the
/// archive, 2026-09-27); in `.egm` the static morph vertices are added to them (see [`Egm`]).
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
