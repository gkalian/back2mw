//! Имена текстур в выходных мешах и список файлов для копирования, T1.6.
//!
//! В NIF Morrowind текстура задаётся голым именем файла из `Textures\`: каталога нет
//! (проверено на всех NIF ванили: 22 021 ссылка, 0 с разделителем, `docs/ru/formats/tes3.md`).
//! Текстуры Oblivion лежат в дереве каталогов, поэтому при выкладывании в один каталог
//! пути приходится сплющивать, а имена — разводить так, чтобы два разных файла не
//! слились в один.
//!
//! **Схема** (часть совместимости вывода, как схема ID в `transform::ids`): путь
//! относительно `textures\`, нижний регистр, `\` → `_`, префикс `ob_`, расширение как в
//! источнике. `textures\architecture\anvil\anvilwall01.dds` →
//! `ob_architecture_anvil_anvilwall01.dds`.
//!
//! Схема **не инъективна**: `a_b\c.dds` и `a\b_c.dds` дают одно имя. Поэтому реестр
//! [`TextureRegistry`] запоминает, какому исходному пути принадлежит имя, и второй
//! другой путь с тем же именем — ошибка, а не перезапись. Суффиксов для разведения
//! коллизий не придумываем: такая схема стала бы частью совместимости с уже собранными
//! мешами.
//!
//! Расширение остаётся из источника (у Oblivion `.dds`). Что ванильный движок ищет `.dds`
//! при `.tga` в NIF — установлено на ванили, но игрой не проверялось.

use std::collections::HashMap;
use std::fmt;

/// Префикс имён конвертированных текстур: с него не начинается ни одно имя ванили.
pub const PREFIX: &str = "ob_";

/// Один файл к копированию: откуда взять в BSA источника и как назвать в `Textures\`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextureCopy {
    /// Путь в архиве источника, нижний регистр, с `textures\` в начале.
    pub source_path: String,
    /// Имя файла в `Textures\` Morrowind.
    pub target_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureError {
    /// Имя или путь не ASCII: в NIF и в файловой системе Morrowind им делать нечего.
    NotAscii { path: String },
    /// От пути ничего не осталось (пусто или один `textures\`).
    Empty { path: String },
    /// В пути есть сегмент `..`.
    ParentSegment { path: String },
    /// Два разных исходных пути дали одно имя.
    Collision {
        name: String,
        first: String,
        second: String,
    },
    /// Карта нормалей (`*_n.dds`) в слоте, который переносится: копировать её не
    /// положено, а ссылку без файла оставлять нельзя.
    NormalMap { path: String },
}

impl fmt::Display for TextureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAscii { path } => lang::trw!(
                f,
                "путь текстуры {path:?} не ASCII",
                "texture path {path:?} is not ASCII"
            ),
            Self::Empty { path } => lang::trw!(
                f,
                "путь текстуры {path:?} пуст",
                "texture path {path:?} is empty"
            ),
            Self::ParentSegment { path } => lang::trw!(
                f,
                "путь текстуры {path:?} содержит «..»",
                "texture path {path:?} contains \"..\""
            ),
            Self::Collision {
                name,
                first,
                second,
            } => lang::trw!(
                f,
                "имя {name} получилось у двух разных текстур: {first} и {second}",
                "two different textures got the name {name}: {first} and {second}"
            ),
            Self::NormalMap { path } => {
                lang::trw!(
                    f,
                    "текстура {path:?} — карта нормалей в переносимом слоте",
                    "texture {path:?} is a normal map in a converted slot"
                )
            }
        }
    }
}

impl std::error::Error for TextureError {}

/// Приводит путь источника к виду `(путь в архиве, имя в Textures\)`. Чистая функция.
pub fn target_of(source: &str) -> Result<TextureCopy, TextureError> {
    if !source.is_ascii() {
        return Err(TextureError::NotAscii {
            path: source.to_owned(),
        });
    }
    let lower = source.to_ascii_lowercase();
    let mut segments: Vec<&str> = lower.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
    if segments.contains(&"..") {
        return Err(TextureError::ParentSegment {
            path: source.to_owned(),
        });
    }
    if segments.first() == Some(&"textures") {
        segments.remove(0);
    }
    if segments.is_empty() {
        return Err(TextureError::Empty {
            path: source.to_owned(),
        });
    }
    if is_normal_map(&lower) {
        return Err(TextureError::NormalMap {
            path: source.to_owned(),
        });
    }
    Ok(TextureCopy {
        source_path: format!("textures\\{}", segments.join("\\")),
        target_name: format!("{PREFIX}{}", segments.join("_")),
    })
}

/// Карта нормалей по соглашению об именах Oblivion: `<имя>_n.<расширение>`.
pub(crate) fn is_normal_map(lower: &str) -> bool {
    let stem = lower.rsplit_once('.').map_or(lower, |(stem, _)| stem);
    stem.ends_with("_n")
}

/// Реестр выданных имён. Один на весь прогон: коллизия между текстурами **разных**
/// мешей — тоже коллизия.
#[derive(Debug, Clone, Default)]
pub struct TextureRegistry {
    /// Имя в `Textures\` → путь в архиве.
    by_name: HashMap<String, String>,
}

impl TextureRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Имя для текстуры источника. Повторный запрос того же пути — то же имя.
    pub fn register(&mut self, source: &str) -> Result<TextureCopy, TextureError> {
        let copy = target_of(source)?;
        match self.by_name.get(&copy.target_name) {
            Some(existing) if *existing == copy.source_path => {}
            Some(existing) => {
                return Err(TextureError::Collision {
                    name: copy.target_name,
                    first: existing.clone(),
                    second: copy.source_path,
                });
            }
            None => {
                self.by_name
                    .insert(copy.target_name.clone(), copy.source_path.clone());
            }
        }
        Ok(copy)
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Все зарегистрированные текстуры по имени файла: порядок не зависит от порядка
    /// регистрации.
    pub fn copies(&self) -> Vec<TextureCopy> {
        let mut out: Vec<TextureCopy> = self
            .by_name
            .iter()
            .map(|(name, source)| TextureCopy {
                source_path: source.clone(),
                target_name: name.clone(),
            })
            .collect();
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests;
