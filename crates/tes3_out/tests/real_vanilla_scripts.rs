//! The `crate::scripts` templates against vanilla (T10.3): the bytecode we write matches the `SCDT`
//! of vanilla scripts of the same kind byte for byte. The synthetic layout test is in
//! `src/scripts/tests.rs`; here is the check the synthetic test cannot give (vanilla bytes are not
//! put into the repository).
//!
//! * Loop — every script of `Morrowind.esm` whose text is exactly our template with volume and
//!   pitch 1.0 (`Sound_Cave_Drip`, `Sound_Machinery`, …) is rebuilt with the same ID and sound.
//! * Variables — every script made only of `short` declarations (`nohello`, `noflee`, …).
//!
//! ```text
//! BACK2MW_GAME_DATA='<directory with Morrowind.esm>' \
//!   cargo test -p tes3_out --test real_vanilla_scripts -- --ignored --nocapture
//! ```

mod common;

use tes3::esp::{Plugin, TES3Object};
use tes3_out::{PluginBuilder, Script};

/// Our script, written and read back: `(SCVR, SCDT)`.
fn ours(script: &Script) -> (Vec<u8>, Vec<u8>, u32) {
    let mut builder = PluginBuilder::new("back2mw", "");
    builder.add_script(script).unwrap();
    let mut plugin = Plugin::new();
    plugin.load_bytes(&builder.to_bytes().unwrap()).unwrap();
    plugin
        .objects
        .into_iter()
        .find_map(|o| match o {
            TES3Object::Script(s) => Some((s.variables, s.bytecode, s.header.num_shorts)),
            _ => None,
        })
        .expect("the plugin has one SCPT")
}

/// Text without comments, empty lines and case — one line at a time.
fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| {
            l.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// `PlayLoopSound3DVP "<sound>"[,] 1.0[,] 1.0` → the sound.
fn loop_sound_of(text: &str) -> Option<String> {
    let l = lines(text);
    let [begin, if_cell, if_playing, play, endif1, endif2, end] = l.as_slice() else {
        return None;
    };
    let quoted = |s: &str| s.split('"').nth(1).map(str::to_owned);
    let sound = quoted(play)?;
    let tail: String = play
        .rsplit('"')
        .next()?
        .chars()
        .filter(|c| !matches!(c, ',' | ' '))
        .collect();
    (begin.starts_with("begin ")
        && if_cell.replace(' ', "") == "if(cellchanged==0)"
        && if_playing.replace(' ', "")
            == format!("if(getsoundplaying\"{}\"==0)", sound.replace(' ', ""))
        && play.starts_with("playloopsound3dvp ")
        && tail == "1.01.0"
        && endif1 == "endif"
        && endif2 == "endif"
        && end.starts_with("end"))
    .then_some(sound)
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn templates_match_vanilla_bytecode() {
    let plugin = Plugin::from_path(common::game_file("Morrowind.esm")).unwrap();
    let (mut loops, mut locals) = (Vec::new(), Vec::new());
    for o in &plugin.objects {
        let TES3Object::Script(s) = o else { continue };
        if let Some(text_sound) = loop_sound_of(&s.text) {
            // The sound is taken from the bytecode: the case in the text may differ.
            let at = s
                .bytecode
                .windows(2)
                .position(|w| w == [0x2f, 0x11])
                .unwrap();
            let len = usize::from(s.bytecode[at + 2]);
            let sound = String::from_utf8(s.bytecode[at + 3..at + 3 + len].to_vec()).unwrap();
            assert!(sound.eq_ignore_ascii_case(&text_sound), "{}", s.id);
            let (vars, code, _) = ours(&Script::LoopSound {
                id: s.id.clone(),
                sound,
            });
            assert!(vars.is_empty());
            assert_eq!(code, s.bytecode, "SCDT {} did not match", s.id);
            loops.push(s.id.clone());
            continue;
        }
        let l = lines(&s.text);
        if l.len() >= 3
            && l[0].starts_with("begin ")
            && l.last().is_some_and(|e| e.starts_with("end"))
            && l[1..l.len() - 1].iter().all(|v| v.starts_with("short "))
        {
            let shorts: Vec<String> = s
                .variables
                .split(|&b| b == 0)
                .filter(|v| !v.is_empty())
                .map(|v| String::from_utf8(v.to_vec()).unwrap())
                .collect();
            let (vars, code, count) = ours(&Script::Locals {
                id: s.id.clone(),
                shorts: shorts.clone(),
            });
            assert_eq!(code, s.bytecode, "SCDT {}", s.id);
            assert_eq!(vars, s.variables, "SCVR {}", s.id);
            assert_eq!(count, s.header.num_shorts, "{}", s.id);
            locals.push(s.id.clone());
        }
    }
    println!("loops: {} — {}", loops.len(), loops.join(", "));
    println!("variables: {} — {}", locals.len(), locals.join(", "));
    // Control: the check did not pass on an empty set.
    assert!(loops.len() >= 10, "{loops:?}");
    assert!(locals.len() >= 10, "{locals:?}");
}
