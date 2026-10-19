# SOUN — sounds

**Status:** ✅ converted · **Morrowind:** `SOUN`, placed sounds as activators with a script · **Code:**
`transform::sounds`, `mesh_convert::audio`

## In Oblivion

`SOUN`: a path to a file **or to a folder** (the engine picks a random file from it — 522 of 1,140),
distance without attenuation and audibility limit, frequency shift, attenuation in hundredths of a dB,
flags (random frequency, loop, 2D…), start and stop time. Sounds are referenced by doors, lights,
activators, containers, regions, creatures, scripts and **placements** (a `REFR` to a `SOUN` — a sound
standing in the world: 5,938).

Files are 16-bit PCM `.wav`, mono and stereo (stereo — 233 of 2,093).

## In Morrowind

`SOUN`: a path to **a single file**, volume 0…255, min and max range (× 20 and × 50 units). Sounds
belong to doors (open, close), lights, regions, creatures. **There is no placed sound**: vanilla uses
an invisible activator with a `PlayLoopSound3DVP` script. Files are 16-bit PCM `.wav`, and the engine
reads them **only from disk**, not from BSAs.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| a `SOUN` referenced by an object of the plugin | `SOUN` | others are not written |
| attenuation | volume | 255 · 10^(−attenuation/2000) |
| min, max | min, max | min·5/20, max·100/50 (the engine's multipliers) |
| folder | the first `.wav` by name | regions get variants, see below |
| stereo `.wav` | mono | average of the channels, sample rate as is |
| doors, lights | door `SNAM`/`ANAM`, light `SNAM` | |
| `ACTI.SNAM` (loop) | a loop script on the activator itself | |
| `REFR` to a `SOUN` | a reference to an invisible `ACTI` `obsm_…` | model `EditorMarker.nif`, loop script |

The loop scripts are written from a template checked byte for byte against 12 vanilla `Sound_*` scripts.

### Region sounds {#region-sounds}

In Oblivion the chances of a region's sounds are **weights** (their sum is about 100 %), and loops play
continuously (100 %). In Morrowind each sound has a chance; in vanilla the sum is most often 55.

- weights are normalised to a sum of 55, each at least 1;
- a loop is an ordinary sound with a weight equal to the average weight of the random ones;
- a folder gives no more variants than the sound's chance: `obr<k>_…` for the next files, chance split
  equally;
- chance 0 and folders without files are not converted.

## What is lost

- **Container sounds** (Morrowind has no such field).
- Frequency shift and randomness, flags, start and stop time, 2D.
- **A random file from a folder** — always the first one (regions get several variants).
- Weather flags and time of day of region sounds.
- Water sounds (`WATR`) and script sounds. Creature sounds only for creatures with their own model (`SNDG`, [`CREA`](crea.md)).
- 33 `SOUN`s in `Oblivion.esm` whose file exists nowhere are not written.

!!! warning "Not verified in the game"
    Volume and range; that `EditorMarker.nif` is invisible; whether the engine runs our loop byte code.
