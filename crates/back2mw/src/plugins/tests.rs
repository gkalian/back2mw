use super::*;
use std::fs;
use std::path::PathBuf;
use tes4_read::{GameFormat, synth};

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("back2mw_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn plugin(&self, name: &str, esm: bool, masters: &[&str]) {
        let mut fields = synth::header_fields(0);
        for m in masters {
            fields.extend_from_slice(&synth::field(b"MAST", format!("{m}\0").as_bytes()));
            fields.extend_from_slice(&synth::field(b"DATA", &0u64.to_le_bytes()));
        }
        let mut bytes = synth::plugin(GameFormat::Tes4, &fields, &[]);
        if esm {
            bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        }
        fs::write(self.0.join(name), bytes).unwrap();
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sample(t: &Temp) -> Vec<PluginInfo> {
    t.plugin("Oblivion.esm", true, &[]);
    t.plugin("Update.esp", false, &["Oblivion.esm"]);
    t.plugin("Knights.esp", false, &["Oblivion.esm"]);
    t.plugin("ZMod.esp", false, &["Oblivion.esm"]);
    t.plugin("AMod.esp", false, &["Oblivion.esm", "ZMod.esp"]);
    t.plugin("Bigger.esm", true, &["Oblivion.esm"]);
    fs::write(t.0.join("Oblivion - Meshes.bsa"), b"not a plugin").unwrap();
    fs::write(t.0.join("readme.txt"), b"x").unwrap();
    list(&t.0).unwrap()
}

#[test]
fn lists_only_plugins_in_the_documented_order() {
    let t = Temp::new("pl_order");
    let names: Vec<String> = sample(&t).into_iter().map(|p| p.name).collect();
    assert_eq!(
        names,
        [
            "Oblivion.esm",
            "Bigger.esm",
            "Knights.esp",
            "Update.esp",
            "AMod.esp",
            "ZMod.esp"
        ]
    );
}

#[test]
fn kind_official_flag_and_load_index_are_filled_in() {
    let t = Temp::new("pl_fields");
    let list = sample(&t);
    assert_eq!(list[0].kind, Kind::Esm);
    assert!(list[0].official && list[0].is_oblivion_master());
    assert_eq!(list[0].load_index, 0);
    let amod = list.iter().find(|p| p.name == "AMod.esp").unwrap();
    assert!(!amod.official);
    assert_eq!(amod.kind, Kind::Esp);
    assert_eq!(amod.masters, ["Oblivion.esm", "ZMod.esp"]);
    assert_eq!(amod.load_index, 4);
    assert!(list.iter().all(|p| p.error.is_none()));
    assert!(list[0].size > 0);
}

#[test]
fn a_broken_plugin_is_listed_with_its_error() {
    let t = Temp::new("pl_broken");
    fs::write(
        t.0.join("Broken.esp"),
        b"garbage that is not a plugin at all",
    )
    .unwrap();
    let list = list(&t.0).unwrap();
    assert!(list[0].error.is_some());
    assert!(list[0].masters.is_empty());
}

#[test]
fn absolute_form_ids_follow_the_master_list() {
    let t = Temp::new("pl_abs");
    let list = sample(&t);
    let by = |n: &str| list.iter().find(|p| p.name == n).unwrap().clone();
    let (base, amod) = (by("Oblivion.esm"), by("AMod.esp"));

    // Oblivion.esm: index 0, FormID does not change — the IDs depend on nothing.
    assert_eq!(
        absolute_form_id(&list, &base, 0x0001_5F3A),
        Some(0x0001_5F3A)
    );
    // AMod: slot 0 is Oblivion.esm, slot 1 is ZMod.esp (index 5), slot 2 is AMod itself (index 4).
    assert_eq!(
        absolute_form_id(&list, &amod, 0x0000_0123),
        Some(0x0000_0123)
    );
    assert_eq!(
        absolute_form_id(&list, &amod, 0x0100_0456),
        Some(0x0500_0456)
    );
    assert_eq!(
        absolute_form_id(&list, &amod, 0x0200_0789),
        Some(0x0400_0789)
    );
}

#[test]
fn a_missing_master_gives_no_id() {
    let t = Temp::new("pl_missing_master");
    t.plugin("Oblivion.esm", true, &[]);
    t.plugin("Orphan.esp", false, &["Oblivion.esm", "NotInstalled.esp"]);
    let list = list(&t.0).unwrap();
    let orphan = list.iter().find(|p| p.name == "Orphan.esp").unwrap();
    assert_eq!(absolute_form_id(&list, orphan, 0x0100_0001), None);
    assert_eq!(absolute_form_id(&list, orphan, 0x0000_0001), Some(1));
}
