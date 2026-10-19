//! Погода: `WTHR` Oblivion → один из десяти типов Morrowind, списки шансов региона и климата →
//! `WEAT` региона Morrowind.
//!
//! У Morrowind погода — не запись, а тип с параметрами в `Morrowind.ini`; регион держит только
//! шансы на каждый тип. Поэтому переносится не сама погода, а её тип. Какой погоде какой тип —
//! контент, он в `data/weather.tsv`; погоду из модов, которой там нет, определяет класс `WTHR`.

use std::collections::HashMap;
use std::sync::OnceLock;

use tes3_out::WeatherChances;
use tes4_read::{WEATHER_CLOUDY, WEATHER_PLEASANT, WEATHER_RAINY, WEATHER_SNOW, Weather};

/// `data/weather.tsv`, встроенный в сборку.
const TABLE: &str = include_str!("../../../data/weather.tsv");

/// Тип погоды Morrowind в порядке `WEAT`.
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

/// Разбор таблицы: `EDID` (без учёта регистра) → тип.
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
        parse_table(TABLE).expect("data/weather.tsv встроен в сборку и проверен тестом")
    })
}

/// Тип погоды Morrowind и откуда он взят: `true` — из таблицы, `false` — по классу.
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

/// Шансы по типам, сведённые к сумме 100: Construction Set требует ровно 100, а у Oblivion сумма
/// бывает любой (у `SENSSouthweather` — 80). Метод наибольшего остатка, при равенстве остатков —
/// тип раньше в порядке `WEAT`. Отрицательные и нулевые шансы не учитываются. `None` — шансов нет.
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
    // Больший остаток — раньше; при равенстве — меньший номер типа.
    rest.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for &(_, i) in rest
        .iter()
        .take(usize::try_from(missing).expect("не больше 10"))
    {
        share[i] += 1;
    }
    let s = share.map(|v| u8::try_from(v).expect("доля не больше 100"));
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
        assert!(t.len() >= 37, "все 37 WTHR Oblivion.esm: {}", t.len());
        assert!(t.values().all(|&k| k != MwWeather::Blight));
        assert!(parse_table("X\tblight\n").is_err());
        assert!(parse_table("X\tsunny\n").is_err());
        assert!(
            parse_table("X\tclear\nx\train\n").is_err(),
            "повтор без учёта регистра"
        );
    }

    #[test]
    fn table_wins_over_class_and_class_is_the_fallback() {
        // У тумана класс «облачно» — таблица знает лучше.
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
        // SENSSouthweather: туман 40, пасмурно 20, дождь 10, гроза 10 — сумма 80.
        let c = chances([
            (MwWeather::Foggy, 40),
            (MwWeather::Overcast, 20),
            (MwWeather::Rain, 10),
            (MwWeather::Thunder, 10),
        ])
        .unwrap();
        assert_eq!((c.foggy, c.overcast, c.rain, c.thunder), (50, 25, 13, 12));
        // Три равные доли: остаток уходит первому по порядку WEAT.
        let c = chances([
            (MwWeather::Rain, 1),
            (MwWeather::Clear, 1),
            (MwWeather::Cloudy, 1),
        ])
        .unwrap();
        assert_eq!((c.clear, c.cloudy, c.rain), (34, 33, 33));
        // Два WTHR одного типа складываются.
        let c = chances([(MwWeather::Clear, 30), (MwWeather::Clear, 70)]).unwrap();
        assert_eq!(c.clear, 100);
        assert_eq!(chances([]), None);
    }
}
