back2mw — converts Oblivion content into a mod for the original Morrowind
==========================================================================

This is an early alpha: most of the output has not been played through in the original game yet.
Known problem: creatures with their own model neither deal nor take hits.

Getting started
---------------
No installation needed: put the folder anywhere (preferably not in Program Files) and run
back2mw-gui.exe. The window opens in English; the RU / EN switch in the top right corner changes the
language of the window, the log and the error messages.

1. The program looks for Morrowind and Oblivion in the standard Steam and GOG folders. If it does not
   find them, press "Browse…" and point to the game folder (its root or Data / Data Files).
2. Tick the Oblivion plugins to convert (original files and mods).
3. Optionally pick a world (worldspace) to convert its lands, interiors, NPCs and dialogue, and
   "Pack files into BSA" to put each plugin's assets into its own archive.
4. Press "Convert". The result is written into Morrowind's Data Files together with the manifest
   back2mw-install.tsv; "Remove from game…" (or back2mw.exe uninstall --target-data "<Data Files>")
   rolls it back.

Each Oblivion plugin becomes a Morrowind plugin with the same name (Mod1.esp -> Mod1.esp). Enable the
plugins in the Morrowind launcher. If you built BSAs, add them to Morrowind.ini, section [Archives]
(e.g. Archive 1=Mod1.bsa) — the program never changes that file. Sounds are always installed as loose
files: the original engine does not read them from archives.

How the player reaches converted lands is up to you: a separate mod or the console (coc "<cell>").

What is converted
-----------------
Statics, trees, world objects, weapons, armor and clothing, items, magic, leveled lists; with a world —
landscape, cities, interiors, regions, path grids, NPCs with baked faces, sounds and dialogue with
voices; creatures placed in the world — on Morrowind donor models or with their own model and
animation from Oblivion; permanent particle effects (fire, smoke). Oblivion scripts, event-triggered
particles and animation inside meshes (other than creatures) are not converted yet.

Full documentation, including what is lost for every record type:
  https://gkalian.github.io/back2mw/      (English and Russian)
  https://github.com/gkalian/back2mw      (source code, releases)

Validation (optional)
---------------------
Set the paths to esmtool.exe and niftest.exe from OpenMW in config.toml (see config.example.toml) to
have the result checked by them.

Console version: back2mw.exe --help (add --lang ru for Russian). It reads the same config.toml as the
window, including the language and the world offset.
