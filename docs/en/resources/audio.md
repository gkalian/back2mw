# Sounds and voices

**Code:** `mesh_convert::audio`, `mesh_convert::audio::mp3`

ffmpeg is not needed: the formats of both games match.

## Sounds (`.wav`)

| | Oblivion | Morrowind |
|---|---|---|
| Format | 16-bit PCM; 44,100, 22,050, 32,000, 11,025, 16,000 Hz | 16-bit PCM; mostly 22,050 Hz |
| Channels | mono and stereo (233 of 2,093) | mono (stereo — 1 file) |
| Where | `Oblivion - Sounds.bsa` | `Sound\Fx`, `Sound\Cr` — **from disk only** |

**Conversion:** mono is copied byte for byte, stereo is mixed to mono by averaging the channels, the
sample rate is unchanged. The file is `Sound\ob\<name>_<hash>.wav`. Sounds are always installed as loose
files, even when building BSAs: the original engine does not read sounds from archives.

## Voices (`.mp3`)

| | Oblivion | Morrowind |
|---|---|---|
| Format | mp3 44,100 Hz mono 64 kbit/s, CBR | the same |
| Path | `sound\voice\<plugin>\<race>\<m\|f>\<quest>_<topic>_<FormID>_<N>.mp3` | `Sound\Vo\…`, the path is in the line's `SNAM` |
| Per line | a file for **each response** | **one file** |
| Lips | a `.lip` per file | the mouth moves with loudness |

**Conversion:** the responses of a line are joined **by mp3 frames** without re-encoding (ID3 tags are
cut off, the format is checked in every frame). The voice folder comes from the "voice race" (the
race's `VNAM`: a Breton woman uses Imperial, Dark and Wood Elves use High Elf, Orcs use Nord, Khajiit use
Argonian). A generic line is duplicated per race + sex pair, each with its own file. The file is
`Sound\Vo\ob\<hash>.mp3`.

## What is lost

- `.lip` lip sync.
- Frequency shift and randomness, `SOUN` flags — see [Sounds](../types/soun.md).
