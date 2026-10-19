# DIAL, INFO, QUST — dialogue

**Status:** 🟡 partly · **Morrowind:** `DIAL`/`INFO` · **Code:** `transform::dialogue`,
`tes3_out::dialogue`, `data/conditions.tsv`, `data/voice_topics.tsv` · **Flag:** only with `--worldspace`

## In Oblivion

A `DIAL` is a topic of one of the types: Topic (an ordinary topic), Conversation (NPC-to-NPC talk and
calls), Combat, Persuasion, Detection, Service, Misc. An `INFO` is a line: **several responses** (each
with its own voice file and emotion), `CTDA` conditions (function, operator, number or global variable,
"or" flag), add topics, choices (`TCLT` — jump to another topic), a result script. A `QUST` is a quest:
stages, journal, conditions on all its lines, "start game enabled" flag. Voice is mp3 in
`Sound\Voice\<plugin>\<race>\<sex>\` with a `.lip` for each file.

`Oblivion.esm` has 3,817 topics, 19,278 lines, 390 quests.

## In Morrowind

`DIAL` — topic (name = ID), voice, greeting (`Greeting 0…9`), persuasion, journal. `INFO` — **a single
text** of up to 512 characters, one sound file, filter fields (NPC, race, class, faction and rank,
cell, sex, disposition) and **at most 6** `SCVR` conditions; there is no "or" — vanilla makes copies of
the line. The result is MWScript text that the engine compiles itself. Voice and persuasion topics are
vanilla ones; the engine asks for them by name.

## How back2mw converts it

**What is selected:**

- lines of quests that **start with the game** and have no conditions on the quest stage
  (`GetStage`, `GetStageDone`, `GetQuestVariable`, `GetQuestRunning`);
- a line with a condition Morrowind has no counterpart for is **dropped entirely**;
- `GetPlayerInSEWorld` is evaluated against the converted world, `GetIsPlayableRace` → "Not Race"
  filters;
- "or" → copies of the `INFO` (at most 8);
- Conversation is not converted, except the `HELLO` call → the `Hello` voice.

**The speaker is computed**, not translated field by field: the unchanging conditions on the speaker
(`GetIsID`, race, class, faction, sex) are checked against the converted NPCs, and the resulting set is
written as an `ONAM` field (one NPC), as shared fields + `Not ID` on the extra ones, or as a copy per NPC
(up to 8). A faction that is not the NPC's main one is expressed with a marker `Local obf_… = 0`. Generic
lines get a `Local ob_npc = 0` filter: only converted NPCs say them, not vanilla ones.

| Oblivion | Morrowind | Rule |
|---|---|---|
| Topic | topic | name — `FULL`; a topic named like a vanilla one is prepended to it |
| `GREETING` | start of `Greeting 0` | converted NPCs greet the Oblivion way |
| voice topics (Combat, Detection, Service…) | vanilla Voice and Persuasion topics | by the table below; no choices or goodbye |
| responses of a line | one `NAME` | joined; longer than 512 — continued through `Choice "Continue"` |
| add topics | `AddTopic` in the result | |
| `TCLT` choice | `Choice "<topic>" n` | lines of the choice topic — copies with a `Choice = n` filter |
| result script | — | dropped |
| voice of the responses | one `.mp3` | mp3 frames joined without re-encoding; `Vo\ob\<hash>.mp3` |
| a generic voiced line | a copy per race + sex pair | each pair has its own voice file |
| `[… Sound]` note on a voice line | empty text | a wordless shout |

Result for `SEWorld`: 42 topics, 1,116 `INFO`s, 744 of them voiced, 285 voice files.

### Voice topics

--8<-- "voice_topics.md"

## What is lost

- **Quests**: stages, journal, lines of quests that do not start with the game.
- **NPC-to-NPC conversations**.
- The **result script** of lines (giving items, changing stages, AI).
- Conditions with no counterpart: `IsGuard`, `GetScriptVariable`, `GetRandomPercent`, trespassing and so
  on — such a line disappears entirely.
- Lines that need more than 6 conditions or more than 8 copies.
- Response emotions, `.lip`.
- Oblivion's `BOAST*` and `JOKE*` persuasion (Morrowind has no counterpart).
- Vanilla greetings (bounty, vampirism) do not reach converted NPCs.

!!! warning "Not verified in the game"
    Whether the original engine plays sound on topics and greetings (in vanilla only voice is voiced) —
    this decides whether topics get voiced; that a block of lines lands at the start of a vanilla topic;
    `Not …` filters with `= 1`; that vanilla NPCs do not pass the `Local ob_npc = 0` filter.
