# REGN, WTHR, CLMT — regions and weather

**Status:** ✅ converted · **Morrowind:** `REGN` · **Code:** `transform::exterior`, `transform::weather`,
`transform::sounds::region_sounds`

## In Oblivion

**`REGN`** — map color, map name, priority and a set of data: weather (`RDWT`: `WTHR` + chance),
sounds (`RDSD`), objects, grass. A cell can have **several regions** (`XCLR`). **`WTHR`** is a full
weather: sky colors by time of day, fog, clouds, lighting, class (clear, cloudy, rain, snow).
**`CLMT`** is the world's climate: a list of weathers with chances, sun and moons.

## In Morrowind

`REGN` — name, map color, chances of **10 fixed weather types** (clear, cloudy, foggy, overcast,
rain, thunder, ash, blight, snow, blizzard), sounds with chances, sleep creatures. A cell has **one
region**. What each weather looks like is set by `Morrowind.ini`, not by the plugin.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| several regions of a cell | one | with weather, highest priority, then number of cells, then lower `FormID` |
| worldspace climate | `REGN` | for cells without a region with weather |
| `WTHR` | weather type | by the table below, otherwise by class |
| weather chances | sum of 100 | largest remainder |
| name | worldspace name | |
| `RCLR` | map color | |
| `RDSD` sounds | the region's `SNAM` | weights to a sum of 55, loops with the average weight; see [Sounds](soun.md#region-sounds) |

The blight storm is never used: it infects the player.

### Weather table

--8<-- "weather.md"

## What is lost

- **The look of the weather**: Oblivion's sky colors, fog, clouds — in Morrowind they are shared in
  `Morrowind.ini`.
- The climate's sun and moons.
- Weather flags and time of day of region sounds.
- Sleep creatures (no source), the region's objects and grass.

!!! warning "Not verified in the game"
    Long region sound lists (52–56 entries in `SEWorld`, vanilla has at most 27).
