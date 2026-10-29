use super::*;
use crate::container::{Record, RecordHeader};
use crate::synth;

fn record(fields: &[Vec<u8>]) -> Record {
    Record {
        header: RecordHeader {
            tag: *b"CREA",
            flags: 0,
            form_id: 0x0001_A6F7,
            vcs1: 0,
            form_version: 0,
            vcs2: 0,
        },
        data: fields.concat(),
    }
}

fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Required fields: `ACBS`, `AIDT`, `DATA`, `RNAM`, `BNAM`, `TNAM`, `WNAM` — as in the robbed Mania
/// guard (`SEUncleLeo`) in `Oblivion.esm`.
fn required() -> Vec<Vec<u8>> {
    vec![
        synth::field(b"EDID", b"SEUncleLeo\0"),
        synth::field(
            b"ACBS",
            &[72, 2, 1, 0, 0, 0, 165, 0, 0, 0, 2, 0, 1, 0, 0, 0],
        ),
        synth::field(b"AIDT", &[5, 30, 80, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        synth::field(
            b"DATA",
            &[
                2, 40, 5, 5, 2, 0, 80, 0, 0, 0, 10, 0, 50, 30, 35, 20, 12, 60, 10, 25,
            ],
        ),
        synth::field(b"RNAM", &[64]),
        synth::field(b"BNAM", &1.0f32.to_le_bytes()),
        synth::field(b"TNAM", &1.5f32.to_le_bytes()),
        synth::field(b"WNAM", &2.0f32.to_le_bytes()),
    ]
}

#[test]
fn reads_stats_flags_and_scalars() {
    let c = Creature::from_record(&record(&required())).unwrap();
    assert_eq!(c.editor_id, "SEUncleLeo");
    assert_eq!(c.flags, 0x1_0248);
    assert_eq!(c.flags & CREATURE_WALKS, CREATURE_WALKS);
    assert_eq!((c.base_spell, c.fatigue, c.level), (0, 165, 2));
    assert_eq!((c.calc_min, c.calc_max), (1, 0));
    assert_eq!(
        c.stats,
        CreatureStats {
            kind: CREATURE_TYPE_UNDEAD,
            combat: 40,
            magic: 5,
            stealth: 5,
            soul: 2,
            health: 80,
            attack_damage: 10,
            attributes: [50, 30, 35, 20, 12, 60, 10, 25],
        }
    );
    assert_eq!(c.ai.aggression, 5);
    assert_eq!(c.attack_reach, 64);
    assert_eq!(
        (c.base_scale, c.turning_speed, c.foot_weight),
        (1.0, 1.5, 2.0)
    );
    assert!(c.model.is_none() && c.body_parts.is_empty() && c.sounds.is_empty());
}

#[test]
fn reads_model_parts_lists_and_sound_groups() {
    let mut fields = required();
    fields.extend([
        synth::field(b"FULL", b"Uncle Leo\0"),
        synth::field(b"MODL", b"Creatures/Zombie/Skeleton.NIF\0"),
        synth::field(b"NIFZ", b"ZombieBody.NIF\0ZombieHead01.NIF\0"),
        synth::field(b"KFFZ", b"SpecialIdle_Leo.kf\0"),
        synth::field(b"SNAM", &[0x10, 0, 0, 0, 3, 0xAA, 0xBB, 0xCC]),
        synth::field(b"CNTO", &[0x20, 0, 0, 0, 5, 0, 0, 0]),
        synth::field(b"SPLO", &u32s(&[0x30])),
        synth::field(b"PKID", &u32s(&[0x40])),
        synth::field(b"PKID", &u32s(&[0x41])),
        synth::field(b"SCRI", &u32s(&[0x50])),
        synth::field(b"INAM", &u32s(&[0x60])),
        synth::field(b"CSCR", &u32s(&[0x70])),
        synth::field(b"CSDT", &u32s(&[0])),
        synth::field(b"CSDI", &u32s(&[0x80])),
        synth::field(b"CSDC", &[50]),
        synth::field(b"CSDT", &u32s(&[6])),
        synth::field(b"CSDI", &u32s(&[0x81])),
        synth::field(b"CSDI", &u32s(&[0x82])),
        synth::field(b"CSDC", &[25]),
        synth::field(b"NAM0", b"Blood/Spray.dds\0"),
    ]);
    let c = Creature::from_record(&record(&fields)).unwrap();
    assert_eq!(c.name.as_deref(), Some("Uncle Leo"));
    assert_eq!(c.model.as_deref(), Some("Creatures/Zombie/Skeleton.NIF"));
    assert_eq!(c.body_parts, ["ZombieBody.NIF", "ZombieHead01.NIF"]);
    assert_eq!(c.animations, ["SpecialIdle_Leo.kf"]);
    assert_eq!(c.factions, [(0x10, 3)]);
    assert_eq!(c.items, [(0x20, 5)]);
    assert_eq!(
        (c.spells.as_slice(), c.packages.as_slice()),
        (&[0x30][..], &[0x40, 0x41][..])
    );
    assert_eq!(
        (c.script, c.death_item, c.sound_source),
        (Some(0x50), Some(0x60), Some(0x70))
    );
    // The `CSDC` chance belongs to the last sound before it; a sound without its own `CSDC` is
    // 100%.
    assert_eq!(
        c.sounds,
        [
            CreatureSound {
                kind: 0,
                sound: 0x80,
                chance: 50
            },
            CreatureSound {
                kind: 6,
                sound: 0x81,
                chance: 100
            },
            CreatureSound {
                kind: 6,
                sound: 0x82,
                chance: 25
            },
        ]
    );
    assert_eq!(c.blood_spray.as_deref(), Some("Blood/Spray.dds"));
}

#[test]
fn bad_sizes_and_sound_before_type_are_errors() {
    let mut short_data = required();
    short_data[3] = synth::field(b"DATA", &[0; 18]);
    assert!(Creature::from_record(&record(&short_data)).is_err());

    let mut orphan = required();
    orphan.push(synth::field(b"CSDI", &u32s(&[0x80])));
    assert!(matches!(
        Creature::from_record(&record(&orphan)),
        Err(Error::BadValue { field, .. }) if field == *b"CSDI"
    ));
}
