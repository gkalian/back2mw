//! Карта перенесённого мира: необязательное дополнение на MWSE-Lua (`--lua-map`).
//!
//! Сам движок Morrowind показать перенесённый worldspace на карте мира не может: она одна на
//! игру и кончается раньше (у ванили — на ячейке x = 28, с патчем Morrowind Code Patch — на
//! x = 51; `docs/ru/formats/tes3.md`, «Окно карты»). Поэтому рядом с плагином кладутся:
//!
//! ```text
//! output/Textures/ob_menus_map_world_<лист>.dds     рисованный лист карты из `WRLD` Oblivion
//! output/MWSE/mods/back2mw/map/worlds/<EDID>.json   где этот лист лежит в мире Morrowind
//! output/MWSE/mods/back2mw/map/main.lua             общий мод: читает все описания
//! ```
//!
//! Лист копируется как есть: Morrowind читает DDS Oblivion (у `SEWorld` — DXT5 1024×1024 без
//! мипов). Движок рисует в интерфейсе только текстуры со сторонами — степенями двойки; у листов
//! `Oblivion.esm` они такие, а про другой лист прогон пишет замечание.
//!
//! Мод один на все плагины back2mw и пишется одинаковым при каждой сборке: вторая установка
//! находит на его месте тот же файл и не считает его чужим (`install`). Без MWSE файлы просто
//! лежат — движок папку `MWSE` не читает. Проект рассчитан на игру без MWSE, поэтому всё это
//! пишется только по флагу.
//!
//! Описание — JSON, потому что MWSE читает его без своего разбора (`json.loadfile`). Пишется
//! руками: полей мало, а зависимость ради них не нужна.

use std::fmt::Write as _;

use transform::world_map::SheetRect;

/// Папка мода относительно `Data Files`.
pub const MOD_DIR: &str = "MWSE/mods/back2mw/map";
/// Сам мод.
pub const SCRIPT_FILE: &str = "MWSE/mods/back2mw/map/main.lua";
/// Текст мода: один и тот же у всех сборок этой версии конвертера.
pub const SCRIPT: &str = include_str!("world_map/main.lua");
/// Номер формата описания; мод читает только его.
pub const FORMAT: u32 = 1;

/// Описание листа карты одного мира.
#[derive(Debug, Clone, PartialEq)]
pub struct Description {
    /// `EDID` worldspace.
    pub world: String,
    /// Имя мира для журнала мода (`FULL`).
    pub name: Option<String>,
    /// Плагин Morrowind, которому принадлежит мир.
    pub plugin: String,
    /// Имя листа в `Textures\`.
    pub texture: String,
    /// Размер файла листа в пикселях: мод вписывает лист в окно с этими пропорциями.
    pub width: u32,
    pub height: u32,
    /// Ячейки Morrowind, в которых стоит мир: (min x, min y, max x, max y). Пока игрок в них,
    /// мод показывает лист.
    pub cells: (i32, i32, i32, i32),
    /// Какой прямоугольник мира покрывает файл листа.
    pub sheet: SheetRect,
}

/// Размер DDS в пикселях (ширина, высота) из заголовка; `None` — не DDS.
pub fn dds_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let field = |at: usize| {
        let raw = bytes.get(at..at + 4)?;
        Some(u32::from_le_bytes(
            raw.try_into().expect("срез ровно 4 байта"),
        ))
    };
    if bytes.get(..4)? != b"DDS " {
        return None;
    }
    // Заголовок: высота, затем ширина.
    Some((field(16)?, field(12)?))
}

/// Путь описания относительно `Data Files` (`/`).
pub fn description_file(world: &str) -> String {
    format!("{MOD_DIR}/worlds/{world}.json")
}

/// Строка JSON: кавычки, обратная косая и управляющие символы экранированы.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => {
                write!(out, "\\u{:04x}", u32::from(c)).expect("запись в String не падает");
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Description {
    /// Текст описания. Ключи в постоянном порядке, числа — как их печатает Rust: тот же вход
    /// даёт те же байты.
    pub fn to_json(&self) -> String {
        let (min_x, min_y, max_x, max_y) = self.cells;
        let s = &self.sheet;
        let name = self
            .name
            .as_deref()
            .map_or_else(|| "null".to_owned(), quoted);
        format!(
            "{{\n  \"format\": {FORMAT},\n  \"world\": {},\n  \"name\": {name},\n  \"plugin\": {},\n  \"texture\": {},\n  \"width\": {},\n  \"height\": {},\n  \"cells\": {{ \"minX\": {min_x}, \"minY\": {min_y}, \"maxX\": {max_x}, \"maxY\": {max_y} }},\n  \"sheet\": {{ \"left\": {}, \"bottom\": {}, \"right\": {}, \"top\": {} }}\n}}\n",
            quoted(&self.world),
            quoted(&self.plugin),
            quoted(&format!("Textures\\{}", self.texture)),
            self.width,
            self.height,
            s.left,
            s.bottom,
            s.right,
            s.top,
        )
    }
}

#[cfg(test)]
mod tests;
