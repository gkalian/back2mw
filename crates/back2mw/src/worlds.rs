//! Worldspace в плагинах Oblivion — для выбора в интерфейсе.
//!
//! Показываются только worldspace без родителя (`WNAM`): дочерние — города, они вливаются в
//! родителя сами (`transform::exterior::merge_children`) и отдельно не переносятся. Список
//! читается без разбора ячеек (`tes4_read::list_worldspaces`), поэтому `Oblivion.esm` на 250 МБ
//! занимает доли секунды на чтение файла.

use std::path::Path;

/// Worldspace, который можно перенести.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldInfo {
    /// Плагин, где он лежит.
    pub plugin: String,
    pub editor_id: String,
    /// `FULL`, если есть.
    pub name: Option<String>,
    /// `EDID` дочерних worldspace (городов), которые вольются в него.
    pub cities: Vec<String>,
}

impl WorldInfo {
    /// Подпись в списке: «SEWorld — Shivering Isles (Oblivion.esm), городов: 5».
    pub fn label(&self) -> String {
        let mut out = self.editor_id.clone();
        if let Some(name) = &self.name {
            out.push_str(" — ");
            out.push_str(name);
        }
        out.push_str(&format!(" ({})", self.plugin));
        if !self.cities.is_empty() {
            out.push_str(&lang::tr!(
                ", городов: {}",
                ", cities: {}",
                self.cities.len()
            ));
        }
        out
    }
}

/// Worldspace плагина `plugin` из папки `data_dir` без родителя, в порядке файла.
pub fn list(data_dir: &Path, plugin: &str) -> Result<Vec<WorldInfo>, String> {
    let path = data_dir.join(plugin);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let all = tes4_read::list_worldspaces(&bytes).map_err(|e| format!("{plugin}: {e}"))?;
    Ok(all
        .iter()
        .filter(|(_, w)| w.parent.is_none())
        .map(|(form_id, w)| WorldInfo {
            plugin: plugin.to_owned(),
            editor_id: w.editor_id.clone(),
            name: w.name.clone(),
            cities: all
                .iter()
                .filter(|(_, c)| c.parent == Some(*form_id))
                .map(|(_, c)| c.editor_id.clone())
                .collect(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tes4_read::{GameFormat::Tes4 as F, synth};

    fn world(id: u32, edid: &str, full: Option<&str>, parent: Option<u32>) -> Vec<u8> {
        let mut fields = synth::field(b"EDID", format!("{edid}\0").as_bytes());
        if let Some(full) = full {
            fields.extend(synth::field(b"FULL", format!("{full}\0").as_bytes()));
        }
        if let Some(p) = parent {
            fields.extend(synth::field(b"WNAM", &p.to_le_bytes()));
        }
        [
            synth::record(F, b"WRLD", id, 0, &fields),
            // Дети worldspace — их обход пропускает, не заглядывая внутрь.
            synth::group(
                F,
                &id.to_le_bytes(),
                1,
                &synth::record(F, b"CELL", id + 1, 0, &[]),
            ),
        ]
        .concat()
    }

    #[test]
    fn only_worlds_without_parent_are_listed_with_their_cities() {
        let dir = std::env::temp_dir().join(format!("back2mw_worlds_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let top = synth::group(
            F,
            b"WRLD",
            0,
            &[
                world(0x10, "SEWorld", Some("Shivering Isles"), None),
                world(0x20, "SENSBliss", Some("New Sheoth"), Some(0x10)),
                world(0x30, "SEManiaGarden", None, None),
            ]
            .concat(),
        );
        let other = synth::group(F, b"STAT", 0, &synth::record(F, b"STAT", 0x40, 0, &[]));
        let bytes = synth::plugin(F, &synth::header_fields(0), &[other, top]);
        std::fs::write(dir.join("Test.esm"), bytes).unwrap();

        let worlds = list(&dir, "Test.esm").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(worlds.len(), 2);
        assert_eq!(worlds[0].cities, ["SENSBliss"]);
        assert_eq!(
            worlds[0].label(),
            "SEWorld — Shivering Isles (Test.esm), cities: 1"
        );
        assert_eq!(
            lang::with(lang::Lang::Ru, || worlds[0].label()),
            "SEWorld — Shivering Isles (Test.esm), городов: 1"
        );
        assert_eq!(worlds[1].label(), "SEManiaGarden (Test.esm)");
        assert!(list(&dir, "Missing.esp").is_err());
    }
}
