//! Map of the carried-over world: an optional MWSE-Lua add-on (`--lua-map`).
//!
//! The Morrowind engine itself cannot show a carried-over worldspace on the world map: there is one
//! map per game and it ends earlier (in vanilla at cell x = 28, with Morrowind Code Patch at x =
//! 51; `docs/en/formats/tes3.md`, "Map window"). So next to the plugin we put:
//!
//! ```text
//! output/Textures/ob_menus_map_world_<sheet>.dds    the painted map sheet from Oblivion's `WRLD`
//! output/MWSE/mods/back2mw/map/worlds/<EDID>.json   where this sheet lies in the Morrowind world
//! output/MWSE/mods/back2mw/map/main.lua             the shared mod: reads all descriptions
//! ```
//!
//! The sheet is copied as is: Morrowind reads Oblivion DDS (`SEWorld` has DXT5 1024×1024 without
//! mipmaps). The engine draws only power-of-two textures in the interface; the sheets of
//! `Oblivion.esm` are like that, and for any other sheet the run writes a note.
//!
//! There is one mod for all back2mw plugins, and it is written identically by every build: a second
//! installation finds the same file in its place and does not treat it as foreign (`install`).
//! Without MWSE the files just sit there — the engine does not read the `MWSE` folder. The project
//! targets the game without MWSE, so all this is written only on a flag.
//!
//! The description is JSON because MWSE reads it without a parser of its own (`json.loadfile`). It
//! is written by hand: there are few fields, and a dependency for them is not needed.

use std::fmt::Write as _;

use transform::world_map::SheetRect;

/// The mod folder relative to `Data Files`.
pub const MOD_DIR: &str = "MWSE/mods/back2mw/map";
/// The mod itself.
pub const SCRIPT_FILE: &str = "MWSE/mods/back2mw/map/main.lua";
/// The mod text: the same for all builds of this converter version.
pub const SCRIPT: &str = include_str!("world_map/main.lua");
/// Description format number; the mod reads only this one.
pub const FORMAT: u32 = 1;

/// Description of one world's map sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Description {
    /// `EDID` worldspace.
    pub world: String,
    /// World name for the mod's log (`FULL`).
    pub name: Option<String>,
    /// The Morrowind plugin the world belongs to.
    pub plugin: String,
    /// Sheet name in `Textures\`.
    pub texture: String,
    /// Size of the sheet file in pixels: the mod fits the sheet into the window with these
    /// proportions.
    pub width: u32,
    pub height: u32,
    /// Morrowind cells the world occupies: (min x, min y, max x, max y). While the player is in
    /// them, the mod shows the sheet.
    pub cells: (i32, i32, i32, i32),
    /// Which rectangle of the world the sheet file covers.
    pub sheet: SheetRect,
}

/// DDS size in pixels (width, height) from the header; `None` means not a DDS.
pub fn dds_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let field = |at: usize| {
        let raw = bytes.get(at..at + 4)?;
        Some(u32::from_le_bytes(
            raw.try_into().expect("the slice is exactly 4 bytes"),
        ))
    };
    if bytes.get(..4)? != b"DDS " {
        return None;
    }
    // Header: height, then width.
    Some((field(16)?, field(12)?))
}

/// Description path relative to `Data Files` (`/`).
pub fn description_file(world: &str) -> String {
    format!("{MOD_DIR}/worlds/{world}.json")
}

/// A JSON string: quotes, backslashes and control characters are escaped.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => {
                write!(out, "\\u{:04x}", u32::from(c)).expect("writing to a String does not fail");
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Description {
    /// Description text. Keys in a fixed order, numbers as Rust prints them: the same input gives
    /// the same bytes.
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
