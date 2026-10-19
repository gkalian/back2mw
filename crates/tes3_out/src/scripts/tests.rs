use super::*;

fn loop_sound(id: &str, sound: &str) -> Script {
    Script::LoopSound {
        id: id.to_owned(),
        sound: sound.to_owned(),
    }
}

/// Байт-код петли, выписанный по инструкциям (раскладка — в заголовке модуля); сверка с ванилью —
/// `tests/real_vanilla_scripts.rs`.
#[test]
fn loop_sound_bytecode_follows_the_vanilla_layout() {
    let c = compile(&loop_sound("obls_Wind_2", "ob_Wind_2")).unwrap();
    let mut want: Vec<u8> = vec![0x06, 0x01, 3, 9];
    want.extend_from_slice(b" X\x1f\x10 == 0");
    want.extend_from_slice(&[0x06, 0x01, 1, 12 + 9]);
    want.extend_from_slice(b" X\x88\x11 b\x09ob_Wind_2 == 0");
    want.extend_from_slice(&[0x2f, 0x11, 9]);
    want.extend_from_slice(b"ob_Wind_2");
    want.extend_from_slice(&[0, 0, 0x80, 0x3f, 0, 0, 0x80, 0x3f]);
    want.extend_from_slice(&[0x09, 0x01, 0x09, 0x01, 0x01, 0x01]);
    assert_eq!(c.bytecode, want);
    assert_eq!(
        c.bytecode.len(),
        46 + 2 * 9,
        "длина у ванили — 46 + 2·длина имени"
    );
    assert!(c.variables.is_empty());
    assert_eq!(c.shorts, 0);
    assert!(c.text.starts_with("Begin obls_Wind_2\r\n"), "{}", c.text);
    assert!(
        c.text.contains("PlayLoopSound3DVP \"ob_Wind_2\", 1.0, 1.0"),
        "{}",
        c.text
    );

    // Контроль: другое имя меняет обе длины и оба вхождения, остальное — нет.
    let other = compile(&loop_sound("obls_Wind_2", "ob_W_3")).unwrap();
    assert_eq!(other.bytecode.len(), 46 + 2 * 6);
    assert_eq!(other.bytecode[..16], want[..16]);
    assert_eq!(other.bytecode[16], 12 + 6);
}

#[test]
fn locals_are_declared_in_scvr_and_the_bytecode_is_a_bare_end() {
    let c = compile(&Script::Locals {
        id: "ob_npc_marker".to_owned(),
        shorts: vec!["ob_npc".to_owned()],
    })
    .unwrap();
    assert_eq!(c.bytecode, [0x01, 0x01]);
    assert_eq!(c.variables, b"ob_npc\0");
    assert_eq!(c.shorts, 1);
    assert_eq!(c.text, "Begin ob_npc_marker\r\n\r\nshort ob_npc\r\n\r\nEnd");
}

#[test]
fn the_record_header_counts_what_the_template_wrote() {
    let TES3Object::Script(s) = script(&loop_sound("obls_A_1", "ob_A_1")).unwrap() else {
        panic!("не SCPT");
    };
    assert_eq!(s.header.bytecode_length as usize, s.bytecode.len());
    assert_eq!(s.header.variables_length, 0);
    let TES3Object::Script(s) = script(&Script::Locals {
        id: "ob_m".to_owned(),
        shorts: vec!["a".to_owned(), "bc".to_owned()],
    })
    .unwrap() else {
        panic!("не SCPT");
    };
    assert_eq!((s.header.num_shorts, s.header.variables_length), (2, 5));
}

#[test]
fn names_that_would_break_the_script_are_rejected() {
    assert!(compile(&loop_sound("obls_A_1", "a\"b")).is_err());
    assert!(compile(&loop_sound("obls_A_1", "")).is_err());
    assert!(compile(&loop_sound("obls_A_1", &"x".repeat(32))).is_err());
    assert!(compile(&loop_sound(&"x".repeat(32), "ob_A_1")).is_err());
    assert!(compile(&loop_sound("obls_A_1", "звук")).is_err());
    assert!(
        compile(&Script::Locals {
            id: "ob_m".to_owned(),
            shorts: vec!["a b".to_owned()],
        })
        .is_err()
    );
    // Контроль: предел — ровно 31.
    assert!(compile(&loop_sound(&"x".repeat(31), &"y".repeat(31))).is_ok());
}
