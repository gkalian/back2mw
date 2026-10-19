use super::*;

/// Кадр MPEG-1 Layer III 64 кбит/с 44 100 Гц: 208 байт, с дополнением — 209. `fill` — байт тела.
fn frame(padding: bool, mono: bool, fill: u8) -> Vec<u8> {
    let mut f = vec![
        0xFF,
        0xFB,
        0x50 | if padding { 0x02 } else { 0 },
        if mono { 0xC0 } else { 0x00 },
    ];
    f.resize(208 + usize::from(padding), fill);
    f
}

fn id3v2(body: usize) -> Vec<u8> {
    let mut t = b"ID3\x03\x00\x00".to_vec();
    // Размер 7-битными байтами.
    let b = u32::try_from(body).unwrap();
    t.extend([21, 14, 7, 0].map(|s| u8::try_from((b >> s) & 0x7F).unwrap()));
    t.resize(10 + body, 0);
    t
}

#[test]
fn frames_are_kept_and_tags_are_cut() {
    let a = frame(false, true, 1);
    let b = frame(true, true, 2);
    let mut file = id3v2(300);
    file.extend(&a);
    file.extend(&b);
    let mut v1 = b"TAG".to_vec();
    v1.resize(128, b' ');
    file.extend(&v1);
    let frames = voice_frames(&file).unwrap();
    assert_eq!(frames, [a.as_slice(), b.as_slice()].concat());
    // Контроль: без тегов — файл как есть.
    let plain = [a.clone(), b.clone()].concat();
    assert_eq!(voice_frames(&plain).unwrap(), plain.as_slice());
}

#[test]
fn xing_frame_is_dropped() {
    let mut xing = frame(false, true, 0);
    xing[4 + SIDE_INFO_MONO..4 + SIDE_INFO_MONO + 4].copy_from_slice(b"Info");
    let a = frame(false, true, 7);
    let file = [xing, a.clone()].concat();
    assert_eq!(voice_frames(&file).unwrap(), a.as_slice());
    // Только служебный кадр — звука нет.
    assert_eq!(voice_frames(&file[..208]), Err(Mp3Error::Empty));
}

#[test]
fn join_puts_frames_of_all_responses_in_order() {
    let a = frame(false, true, 1);
    let b = frame(true, true, 2);
    let mut tagged = id3v2(20);
    tagged.extend(&b);
    let joined = join_voice([a.as_slice(), tagged.as_slice()]).unwrap();
    assert_eq!(joined, [a, b].concat());
    assert_eq!(join_voice([]), Err(Mp3Error::Empty));
}

#[test]
fn other_formats_and_garbage_are_errors() {
    let stereo = frame(false, false, 0);
    assert!(matches!(
        voice_frames(&stereo),
        Err(Mp3Error::Format { mono: false, .. })
    ));
    let mut fast = frame(false, true, 0);
    fast[2] = 0x90; // 128 кбит/с
    assert!(matches!(
        voice_frames(&fast),
        Err(Mp3Error::Format {
            bitrate_kbps: 128,
            ..
        })
    ));
    // Мусор после кадров (как у `testtoddquest`) и обрезанный кадр.
    let mut tail = frame(false, true, 0);
    tail.extend([0u8; 30]);
    assert_eq!(voice_frames(&tail), Err(Mp3Error::NotFrame { offset: 208 }));
    let cut = &frame(false, true, 0)[..100];
    assert_eq!(voice_frames(cut), Err(Mp3Error::Truncated { offset: 0 }));
    assert_eq!(voice_frames(b"RIFF"), Err(Mp3Error::NotFrame { offset: 0 }));
}
