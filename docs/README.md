# Dungeon Master II reverse-engineering notes

Notes on the DOS release of *Dungeon Master II: The Legend of Skullkeep*,
written to support a clean reimplementation in Rust (`crates/`). The new
engine reads the original data files from the user's own copy at runtime;
no game data, decompiler output or original text is stored in this repo.

## Layout

| File | Topic | Status |
|------|-------|--------|
| [00-overview.md](00-overview.md) | Distribution files, executables, toolchain | started |
| [01-executable.md](01-executable.md) | SKULL.EXE LE layout, unpacking, address map | started |
| [02-graphics-dat.md](02-graphics-dat.md) | GRAPHICS.DAT archive: index, categories/types, image/sound formats | started |
| [03-dungeon-dat.md](03-dungeon-dat.md) | DUNGEON.DAT: maps, things, creatures, text | started |
| [04-rendering.md](04-rendering.md) | Layout table, viewport traversal, draw requests, blitter, light, fonts, display driver | started |
| [05-timeline.md](05-timeline.md) | Main loop, RNG, timeline/event queue, movement, doors, actuators, map transitions | started |
| [06-champions.md](06-champions.md) | Champion record, RNG, skills/levels, regen, poison, recruit, death | started |
| [07-combat-magic.md](07-combat-magic.md) | Action codes, melee, champion damage, missiles, explosions, runes/spells | started |
| [08-creatures-ai.md](08-creatures-ai.md) | Creature info records, slots, AI programs/interpreter, movement rules, damage, animation | started |
| [09-items.md](09-items.md) | Item identity, attributes, slots, charges, containers, eating and potions | started |
| [10-ui-input.md](10-ui-input.md) | Rectangle layout, mouse zones, keys, commands, leader hand, dialogs | started |
| [11-audio.md](11-audio.md) | Digital SFX, HMP music, SONGLIST.DAT, FM banks | done (first pass) |
| [12-savegame.md](12-savegame.md) | SKSAVE format: raw dungeon snapshot plus bit-packed state | started |
| [13-text.md](13-text.md) | Text entries: obfuscation, escapes, embedded mini-languages | started |
| [14-cutscenes.md](14-cutscenes.md) | MVE cutscenes, CD image contents, other files | done (first pass) |

## Workflow

1. `python3 tools/le_unpack.py original/dumast2/SKULL.EXE re/skull`
   produces the relocated objects plus `layout.json`.
2. Ghidra headless (see `01-executable.md`) imports `re/skull/flat.bin` at
   0x10000 and exports a decompilation listing to `re/` (gitignored).
3. Findings are written up here in our own words and implemented in
   `crates/dm2-formats` (file formats) and `crates/dm2-engine` (game logic).
