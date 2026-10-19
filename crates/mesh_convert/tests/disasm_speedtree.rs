//! T2.4: инструмент сверки фактов о SpeedTree с кодом `Oblivion.exe` (`docs/ru/formats/spt.md`).
//!
//! Не проверка, а печать: `BACK2MW_OBLIVION_EXE` — путь к exe, `BACK2MW_DISASM` — список через
//! запятую: `0x7a6cd0` (функция до `ret`, не больше 400 команд), `0x7a6cd0+120` (столько
//! команд), `f32@0xa8c694`, `f64@0xa89c50`, `bytes@0xa8c740+64`. Exe только читается как
//! данные — ничего не запускается.
//!
//! ```text
//! BACK2MW_OBLIVION_EXE='D:\...\Oblivion.exe' BACK2MW_DISASM=0x7a6cd0 \
//!   cargo test -p mesh_convert --test disasm_speedtree -- --ignored --nocapture
//! ```

use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter, Mnemonic};

/// Раздел PE: виртуальный адрес, размер, смещение в файле.
struct Section {
    va: u32,
    size: u32,
    raw: u32,
    raw_size: u32,
}

struct Image {
    bytes: Vec<u8>,
    base: u32,
    sections: Vec<Section>,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

impl Image {
    fn load(path: &str) -> Self {
        let bytes = std::fs::read(path).expect("Oblivion.exe");
        let pe = u32_at(&bytes, 0x3c) as usize;
        assert_eq!(&bytes[pe..pe + 4], b"PE\0\0", "не PE");
        let count = usize::from(u16_at(&bytes, pe + 6));
        let optional = usize::from(u16_at(&bytes, pe + 20));
        let base = u32_at(&bytes, pe + 24 + 28);
        let table = pe + 24 + optional;
        let sections = (0..count)
            .map(|i| {
                let s = table + i * 40;
                Section {
                    size: u32_at(&bytes, s + 8),
                    va: u32_at(&bytes, s + 12),
                    raw_size: u32_at(&bytes, s + 16),
                    raw: u32_at(&bytes, s + 20),
                }
            })
            .collect();
        Self {
            bytes,
            base,
            sections,
        }
    }

    /// VA всех вхождений `needle` в данных разделов.
    fn find(&self, needle: &[u8]) -> Vec<u32> {
        let mut out = Vec::new();
        for s in &self.sections {
            let raw = &self.bytes[s.raw as usize..(s.raw + s.raw_size) as usize];
            for (i, w) in raw.windows(needle.len()).enumerate() {
                if w == needle {
                    out.push(self.base + s.va + u32::try_from(i).expect("образ меньше 4 ГБ"));
                }
            }
        }
        out
    }

    /// Байты начиная с VA (до конца раздела).
    fn at(&self, va: u32) -> &[u8] {
        let rva = va - self.base;
        let s = self
            .sections
            .iter()
            .find(|s| rva >= s.va && rva < s.va + s.size.max(s.raw_size))
            .unwrap_or_else(|| panic!("VA {va:#x} вне разделов"));
        let off = (s.raw + (rva - s.va)) as usize;
        &self.bytes[off..(s.raw + s.raw_size) as usize]
    }
}

fn disasm(img: &Image, va: u32, limit: Option<usize>) {
    let code = img.at(va);
    let mut decoder = Decoder::with_ip(32, code, u64::from(va), DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    let mut text = String::new();
    let mut ins = Instruction::default();
    let max = limit.unwrap_or(400);
    for _ in 0..max {
        if !decoder.can_decode() {
            break;
        }
        decoder.decode_out(&mut ins);
        text.clear();
        formatter.format(&ins, &mut text);
        println!("{:08x}  {text}", ins.ip());
        if limit.is_none() && ins.mnemonic() == Mnemonic::Ret {
            break;
        }
    }
}

fn number(s: &str) -> u32 {
    let s = s.trim();
    s.strip_prefix("0x").map_or_else(
        || s.parse().unwrap(),
        |h| u32::from_str_radix(h, 16).unwrap(),
    )
}

#[test]
#[ignore = "инструмент: BACK2MW_OBLIVION_EXE, BACK2MW_DISASM"]
fn disassemble() {
    let exe = std::env::var("BACK2MW_OBLIVION_EXE").expect("BACK2MW_OBLIVION_EXE");
    let img = Image::load(&exe);
    let list = std::env::var("BACK2MW_DISASM").unwrap_or_default();
    for item in list.split(',').filter(|s| !s.trim().is_empty()) {
        println!("== {item}");
        if let Some(addr) = item.strip_prefix("f32@") {
            let b = img.at(number(addr));
            println!("{}", f32::from_le_bytes(b[..4].try_into().unwrap()));
        } else if let Some(addr) = item.strip_prefix("f64@") {
            let b = img.at(number(addr));
            println!("{}", f64::from_le_bytes(b[..8].try_into().unwrap()));
        } else if let Some(text) = item.strip_prefix("findstr@") {
            // `findstr@.?AVTESObjectTREE@@`: VA всех вхождений строки.
            for va in img.find(text.as_bytes()) {
                println!("{va:#x}");
            }
        } else if let Some(value) = item.strip_prefix("finddw@") {
            // `finddw@0xb39e18`: VA всех выровненных по 4 `u32`, равных значению.
            for va in img.find(&number(value).to_le_bytes()) {
                if va % 4 == 0 {
                    println!("{va:#x}");
                }
            }
        } else if let Some(rest) = item.strip_prefix("u32@") {
            // `u32@0x7a7cbc+18`: столько `u32` подряд (таблицы переходов).
            let (addr, count) = rest.split_once('+').unwrap_or((rest, "1"));
            let b = img.at(number(addr));
            let words: Vec<String> = (0..number(count) as usize)
                .map(|i| format!("{:#x}", u32_at(b, i * 4)))
                .collect();
            println!("{}", words.join(" "));
        } else if let Some(rest) = item.strip_prefix("scan@") {
            // `scan@0x783000-0x7b8000=0x1f4`: команды, у которых непосредственный операнд или
            // смещение в памяти равны значению (линейный разбор — на данных может сбиться).
            let (range, value) = rest.split_once('=').unwrap();
            let (from, to) = range.split_once('-').unwrap();
            let (from, to, value) = (number(from), number(to), u64::from(number(value)));
            let code = &img.at(from)[..(to - from) as usize];
            let mut decoder = Decoder::with_ip(32, code, u64::from(from), DecoderOptions::NONE);
            let mut formatter = IntelFormatter::new();
            let mut ins = Instruction::default();
            let mut text = String::new();
            while decoder.can_decode() {
                decoder.decode_out(&mut ins);
                let hit = (0..ins.op_count()).any(|i| {
                    matches!(
                        ins.op_kind(i),
                        iced_x86::OpKind::Immediate8
                            | iced_x86::OpKind::Immediate16
                            | iced_x86::OpKind::Immediate32
                            | iced_x86::OpKind::Immediate8to32
                    ) && ins.immediate(i) == value
                }) || u64::from(ins.memory_displacement32()) == value;
                if hit {
                    text.clear();
                    formatter.format(&ins, &mut text);
                    println!("{:08x}  {text}", ins.ip());
                }
            }
        } else if let Some(rest) = item.strip_prefix("bytes@") {
            let (addr, len) = rest.split_once('+').unwrap_or((rest, "64"));
            let b = &img.at(number(addr))[..number(len) as usize];
            let text: String = b
                .iter()
                .map(|&c| {
                    if c.is_ascii_graphic() || c == b' ' {
                        c as char
                    } else {
                        '.'
                    }
                })
                .collect();
            println!("{text}");
        } else {
            let (addr, count) = match item.split_once('+') {
                Some((a, n)) => (a, Some(number(n) as usize)),
                None => (item, None),
            };
            disasm(&img, number(addr), count);
        }
    }
}
