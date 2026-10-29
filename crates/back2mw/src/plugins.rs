//! The list of Oblivion plugins in the `Data` folder: original files and mods.
//!
//! **Load order.** The game's real order is set by `Plugins.txt` and file dates; here the order is
//! our own and deterministic: `Oblivion.esm`, the other `.esm` files by name, then the original
//! `.esp` files (in [`OFFICIAL`] order), then mods by name. The index in this order is the high
//! byte of the absolute `FormID`, on which the string IDs in the Morrowind plugin depend. So the
//! IDs are stable as long as the set of **installed** plugins does not change (not the set of
//! ticked ones): install a new mod and the indices of the plugins after it shift. This is a
//! limitation, not a property of the ID scheme. The index of `Oblivion.esm` is always 0, so its IDs
//! depend on nothing.

use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use tes4_read::read_header;

/// Original Oblivion files (lower case) in load order.
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

/// How many bytes from the start of the file we read for the header: the whole `TES4` record.
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

/// One installed plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    /// File name as on disk.
    pub name: String,
    pub kind: Kind,
    pub size: u64,
    /// An original Oblivion file, not a mod.
    pub official: bool,
    /// Masters from the header; empty if the header could not be read.
    pub masters: Vec<String>,
    /// The header did not parse: the plugin is shown but cannot be converted.
    pub error: Option<String>,
    /// Position in the load order (0 is `Oblivion.esm`).
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

/// Plugins in `Data`: `*.esm` and `*.esp`, in load order (see the module header).
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

/// The master's position in the load order; `None` means the master is not installed.
pub fn index_of(plugins: &[PluginInfo], name: &str) -> Option<usize> {
    plugins
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .map(|p| p.load_index)
}

/// Absolute `FormID` of a record of plugin `plugin`: the file's high byte is an index into its
/// master list (or the plugin itself, if the index equals the number of masters) → position in the
/// load order. `None` if the needed master is not installed or the position does not fit into a
/// byte.
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
