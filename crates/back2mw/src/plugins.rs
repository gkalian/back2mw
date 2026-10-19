//! Список плагинов Oblivion из папки `Data`: оригинальные файлы и моды.
//!
//! **Порядок загрузки.** Настоящий порядок игры задают `Plugins.txt` и даты файлов; здесь порядок
//! свой и детерминированный: `Oblivion.esm`, остальные `.esm` по имени, затем оригинальные
//! `.esp` (в порядке [`OFFICIAL`]), затем моды по имени. Индекс в этом порядке — старший байт
//! абсолютного `FormID`, от которого зависят строковые ID в плагине Morrowind. Значит ID стабильны,
//! пока набор **установленных** плагинов не меняется (не набор отмеченных): установили новый мод —
//! индексы плагинов после него сдвинулись. Это ограничение, а не свойство схемы ID.
//! Индекс `Oblivion.esm` всегда 0, поэтому его ID не зависят ни от чего.

use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use tes4_read::read_header;

/// Оригинальные файлы Oblivion (нижний регистр) в порядке загрузки.
pub const OFFICIAL: [&str; 12] = [
    "oblivion.esm",
    "dlcshiveringisles.esp",
    "knights.esp",
    "dlcbattlehorncastle.esp",
    "dlcfrostcrag.esp",
    "dlchorsearmor.esp",
    "dlcmehrunesrazor.esp",
    "dlcorrery.esp",
    "dlcspelltomes.esp",
    "dlcthievesden.esp",
    "dlcvilelair.esp",
    "update.esp",
];

/// Сколько байт от начала файла читаем для заголовка: запись `TES4` целиком.
const HEADER_PREFIX: u64 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Esm,
    Esp,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Esm => "ESM",
            Self::Esp => "ESP",
        })
    }
}

/// Один установленный плагин.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    /// Имя файла как на диске.
    pub name: String,
    pub kind: Kind,
    pub size: u64,
    /// Оригинальный файл Oblivion, а не мод.
    pub official: bool,
    /// Мастера по заголовку; пусто, если заголовок не прочитался.
    pub masters: Vec<String>,
    /// Заголовок не разобрался: плагин показывается, но конвертировать его нельзя.
    pub error: Option<String>,
    /// Позиция в порядке загрузки (0 — `Oblivion.esm`).
    pub load_index: usize,
}

impl PluginInfo {
    pub fn is_oblivion_master(&self) -> bool {
        self.name.eq_ignore_ascii_case("Oblivion.esm")
    }
}

fn official_rank(name: &str) -> Option<usize> {
    OFFICIAL.iter().position(|o| o.eq_ignore_ascii_case(name))
}

/// Плагины `Data`: `*.esm` и `*.esp`, в порядке загрузки (см. заголовок модуля).
pub fn list(data_dir: &Path) -> std::io::Result<Vec<PluginInfo>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(data_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        let kind = if lower.ends_with(".esm") {
            Kind::Esm
        } else if lower.ends_with(".esp") {
            Kind::Esp
        } else {
            continue;
        };
        let size = entry.metadata()?.len();
        let (masters, error) = match read_masters(&entry.path()) {
            Ok(m) => (m, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        out.push(PluginInfo {
            official: official_rank(&name).is_some(),
            name,
            kind,
            size,
            masters,
            error,
            load_index: 0,
        });
    }
    out.sort_by_key(|p| {
        let rank = official_rank(&p.name);
        (
            !p.is_oblivion_master(),
            p.kind != Kind::Esm,
            rank.is_none(),
            rank.unwrap_or(0),
            p.name.to_ascii_lowercase(),
        )
    });
    for (i, p) in out.iter_mut().enumerate() {
        p.load_index = i;
    }
    Ok(out)
}

fn read_masters(path: &Path) -> Result<Vec<String>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(HEADER_PREFIX).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    read_header(&bytes)
        .map(|h| h.masters)
        .map_err(|e| e.to_string())
}

/// Позиция мастера в порядке загрузки, `None` — мастер не установлен.
pub fn index_of(plugins: &[PluginInfo], name: &str) -> Option<usize> {
    plugins
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .map(|p| p.load_index)
}

/// Абсолютный `FormID` записи плагина `plugin`: старший байт файла — индекс в его списке
/// мастеров (или сам плагин, если индекс равен числу мастеров) → позиция в порядке загрузки.
/// `None`, если нужный мастер не установлен или позиция не влезает в байт.
pub fn absolute_form_id(
    plugins: &[PluginInfo],
    plugin: &PluginInfo,
    file_form_id: u32,
) -> Option<u32> {
    let slot = usize::try_from(file_form_id >> 24).ok()?;
    let owner = match plugin.masters.get(slot) {
        Some(master) => index_of(plugins, master)?,
        None => plugin.load_index,
    };
    let owner = u32::try_from(owner).ok().filter(|&i| i <= 0xFF)?;
    Some((owner << 24) | (file_form_id & 0x00FF_FFFF))
}

#[cfg(test)]
mod tests;
