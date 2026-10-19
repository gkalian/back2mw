# SCPT — scripts

**Status:** ⛔ not converted · **Morrowind:** only service scripts from templates · **Code:**
`tes3_out::Script`

## In Oblivion

A script is text plus compiled byte code, event blocks (`Begin GameMode`, `OnActivate`, `OnAdd`,
`OnEquip`, `OnDeath`…), its own set of functions. Scripts are attached to objects, quests, effects
(`SEFF`) and dialogue results. `Oblivion.esm` has 2,393 scripts.

## In Morrowind

MWScript: **the engine runs byte code** (`SCDT`); the text is only for the Construction Set. A script is
one implicit block executed every frame while the object is loaded; events are checks inside it
(`OnActivate`, `OnPCAdd`…). The set of functions is different.

## Current state

There is no script translator. The converter writes **only its own service scripts** from templates,
each checked byte for byte against vanilla scripts of the same kind:

| Template | Purpose |
|---|---|
| `LoopSound` | a sound loop on an activator and on an invisible sound marker (like vanilla `Sound_*`) |
| `Locals` | a script of `short` variables only: the NPC markers `ob_npc`, `obf_…` for dialogue filters |

Dialogue results (`AddTopic`, `Choice`, `Goodbye`) are MWScript text that the engine compiles itself.

## What is lost

All of Oblivion's script logic: levers, traps, quest objects, NPC behavior, script effects of
enchantments and potions, dialogue results.

## Plans

Converting scripts needs our own MWScript compiler into byte code — there is an oracle for it: all 632
scripts of `Morrowind.esm` store both the text and the byte code. The scope (all scripts or only object
ones: doors, levers, traps) will be decided after a census of the commands Oblivion scripts actually
call.
