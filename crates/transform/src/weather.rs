//! Weather: Oblivion `WTHR` → one of Morrowind's ten types, region and climate chance lists → the
//! Morrowind region's `WEAT`.
//!
//! In Morrowind weather is not a record but a type with parameters in `Morrowind.ini`; a region
//! holds only the chances for each type. So it is not the weather itself that is carried over but
//! its type. Which weather gets which type is content, it lives in `data/weather.tsv`; weather from
//! mods that is not there is classified by the `WTHR` class.

use std::collections::HashMap;
use std::sync::OnceLock;

use tes3_out::WeatherChances;
use tes4_read::{WEATHER_CLOUDY, WEATHER_PLEASANT, WEATHER_RAINY, WEATHER_SNOW, Weather};

/// `data/weather.tsv`, built into the binary.
const TABLE: &str = include_str!("../../../data/weather.tsv");

/// Morrowind weather type in `WEAT` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MwWeather {
    Clear,
    Cloudy,
    Foggy,
    Overcast,
    Rain,
    Thunder,
    Ash,
    Blight,
    Snow,
    Blizzard,
}

impl MwWeather {
    const ALL: [Self; 10] = [
        Self::Clear,
        Self::Cloudy,
        Self::Foggy,
        Self::Overcast,
        Self::Rain,
        Self::Thunder,
        Self::Ash,
        Self::Blight,
        Self::Snow,
        Self::Blizzard,
    ];

    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "clear" => Self::Clear,
            "cloudy" => Self::Cloudy,
            "foggy" => Self::Foggy,
            "overcast" => Self::Overcast,
            "rain" => Self::Rain,
            "thunder" => Self::Thunder,
            "ash" => Self::Ash,
            "blight" => Self::Blight,
            "snow" => Self::Snow,
            "blizzard" => Self::Blizzard,
            _ => return None,
        })
    }
}

/// Table parsing: `EDID` (case-insensitive) → type.
pub fn parse_table(text: &str) -> Result<HashMap<String, MwWeather>, String> {
    let mut out = HashMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(edid), Some(kind)) = (cols.next(), cols.next()) else {
            return Err(lang::tr!(
                "строка {}: нужно две колонки",
                "line {}: two columns are required",
                n + 1
            ));
        };
        let kind = MwWeather::parse(kind).ok_or_else(|| {
            lang::tr!(
                "строка {}: неизвестный тип погоды {kind:?}",
                "line {}: unknown weather type {kind:?}",
                n + 1
            )
        })?;
        if kind == MwWeather::Blight {
            return Err(lang::tr!(
                "строка {}: blight заражает игрока — в таблице не используется",
                "line {}: blight infects the player — not used in the table",
                n + 1
            ));
        }
        if out.insert(edid.to_ascii_lowercase(), kind).is_some() {
            return Err(lang::tr!(
                "строка {}: {edid} повторяется",
                "line {}: {edid} is repeated",
                n + 1
            ));
        }
    }
    Ok(out)
}

fn table() -> &'static HashMap<String, MwWeather> {
    static CELL: OnceLock<HashMap<String, MwWeather>> = OnceLock::new();
    CELL.get_or_init(|| {
        parse_table(TABLE).expect("data/weather.tsv is built into the binary and checked by a test")
    })
}

/// The Morrowind weather type and where it came from: `true` — from the table, `false` — by class.
#[must_use]
pub fn classify(weather: &Weather) -> (MwWeather, bool) {
    if let Some(&kind) = table().get(&weather.editor_id.to_ascii_lowercase()) {
        return (kind, true);
    }
    let class = weather.classification;
    let kind = if class & WEATHER_SNOW != 0 {
        MwWeather::Snow
    } else if class & WEATHER_RAINY != 0 {
        MwWeather::Rain
    } else if class & WEATHER_CLOUDY != 0 {
        MwWeather::Cloudy
    } else if class & WEATHER_PLEASANT != 0 {
        MwWeather::Clear
    } else {
        MwWeather::Overcast
    };
    (kind, false)
}

/// Chances by type, normalized to a sum of 100: the Construction Set requires exactly 100, while in
/// Oblivion the sum can be anything (`SENSSouthweather` has 80). The largest remainder method; on
/// equal remainders the type earlier in `WEAT` order wins. Negative and zero chances are ignored.
/// `None` means there are no chances.
#[must_use]
pub fn chances(list: impl IntoIterator<Item = (MwWeather, u64)>) -> Option<WeatherChances> {
    let mut by_kind = [0u64; 10];
    for (kind, chance) in list {
        by_kind[kind as usize] += chance;
    }
    let total: u64 = by_kind.iter().sum();
    if total == 0 {
        return None;
    }
    let mut share = [0u64; 10];
    let mut rest: Vec<(u64, usize)> = Vec::new();
    for (i, &c) in by_kind.iter().enumerate() {
        share[i] = c * 100 / total;
        rest.push((c * 100 % total, i));
    }
    let missing = 100 - share.iter().sum::<u64>();
    // The larger remainder first; on a tie the smaller type number.
    rest.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for &(_, i) in rest
        .iter()
        .take(usize::try_from(missing).expect("no more than 10"))
    {
        share[i] += 1;
    }
    let s = share.map(|v| u8::try_from(v).expect("a share is no more than 100"));
    debug_assert_eq!(MwWeather::ALL.len(), s.len());
    Some(WeatherChances {
        clear: s[0],
        cloudy: s[1],
        foggy: s[2],
        overcast: s[3],
        rain: s[4],
        thunder: s[5],
        ash: s[6],
        blight: s[7],
        snow: s[8],
        blizzard: s[9],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weather(edid: &str, class: u8) -> Weather {
        Weather {
            editor_id: edid.to_owned(),
            classification: class,
        }
    }

    #[test]
    fn embedded_table_parses_and_never_uses_blight() {
        let t = parse_table(TABLE).unwrap();
        assert!(t.len() >= 37, "all 37 WTHR of Oblivion.esm: {}", t.len());
        assert!(t.values().all(|&k| k != MwWeather::Blight));
        assert!(parse_table("X\tblight\n").is_err());
        assert!(parse_table("X\tsunny\n").is_err());
        assert!(
            parse_table("X\tclear\nx\train\n").is_err(),
            "a case-insensitive repeat"
        );
    }

    #[test]
    fn table_wins_over_class_and_class_is_the_fallback() {
        // Fog has the "cloudy" class — the table knows better.
        assert_eq!(
            classify(&weather("SEFog", WEATHER_CLOUDY)),
            (MwWeather::Foggy, true)
        );
        assert_eq!(
            classify(&weather("ModStorm", WEATHER_RAINY | WEATHER_CLOUDY)),
            (MwWeather::Rain, false)
        );
        assert_eq!(
            classify(&weather("ModSnow", WEATHER_SNOW)),
            (MwWeather::Snow, false)
        );
        assert_eq!(classify(&weather("ModX", 0)), (MwWeather::Overcast, false));
    }

    #[test]
    fn chances_sum_to_a_hundred() {
        // SENSSouthweather: fog 40, overcast 20, rain 10, thunder 10 — the sum is 80.
        let c = chances([
            (MwWeather::Foggy, 40),
            (MwWeather::Overcast, 20),
            (MwWeather::Rain, 10),
            (MwWeather::Thunder, 10),
        ])
        .unwrap();
        assert_eq!((c.foggy, c.overcast, c.rain, c.thunder), (50, 25, 13, 12));
        // Three equal shares: the remainder goes to the first in WEAT order.
        let c = chances([
            (MwWeather::Rain, 1),
            (MwWeather::Clear, 1),
            (MwWeather::Cloudy, 1),
        ])
        .unwrap();
        assert_eq!((c.clear, c.cloudy, c.rain), (34, 33, 33));
        // Two WTHR of the same type add up.
        let c = chances([(MwWeather::Clear, 30), (MwWeather::Clear, 70)]).unwrap();
        assert_eq!(c.clear, 100);
        assert_eq!(chances([]), None);
    }
}
