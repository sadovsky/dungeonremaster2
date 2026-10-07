# dungeonremaster2

A from-scratch reimplementation of the 1994 DOS game *Dungeon Master II:
The Legend of Skullkeep*, written in Rust, together with the
reverse-engineering notes it is built from.

**No game data is included.** The engine reads the graphics, dungeon,
sounds, music and rule tables from your own copy of the original game at
runtime. Nothing from the original files, executable or text is stored
in this repository.

## Requirements

- Rust (stable) and Cargo.
- Your own copy of the DOS release, with `SKULL.EXE` and a `DATA/`
  directory containing `GRAPHICS.DAT`, `DUNGEON.DAT` and `SONGLIST.DAT`,
  plus `MELODIC.BNK` and `DRUM.BNK` next to `SKULL.EXE` for music.

By default the tools look for the game in `original/dumast2/` (gitignored).
To set that up from an archive of the game:

```
mkdir -p original && unzip YOUR_DM2_ARCHIVE.zip -d original
```

## Running

```
cargo run --release -p dm2                 # uses original/dumast2/DATA
cargo run --release -p dm2 -- /path/to/DATA
```

| Key | Action |
|-----|--------|
| Keypad, or W/A/S/D and cursor keys | Move and turn (Q/E also turn) |
| 1-4 | Open a champion's inventory |
| Mouse | Everything the original's mouse does: arrows, hands, actions, runes, inventory, picking up and throwing items |
| Ctrl+S | Save (slot 0) |
| Esc | Pause / resume |
| Tab | Debug overlay; PageUp/PageDown jump between maps |

Environment variables:

| Variable | Meaning |
|----------|---------|
| `DM2_DATA` | Path to the game's `DATA/` directory |
| `DM2_SAVE_DIR` | Where saves go (default `./saves`; the original install is never written) |
| `DM2_TICK_MS` | Game tick length in milliseconds (default 133.3, see `docs/05`) |
| `DM2_MUTE` | Set to disable audio |
| `DM2_DEMO_CHAMPION` | Show a placeholder champion when nobody is recruited (screenshots) |

Headless screenshots (no window):

```
cargo run --release -p dm2 -- --screenshot out.png [MAP X Y DIR] [--ticks N] [--cmd C]...
cargo run --release -p dm2 -- --screenshot-title out.png
```

Render a music track to WAV for listening checks:

```
cargo run --release -p dm2-engine --example songwav -- SONG [SECONDS]
```

## Layout

| Path | Contents |
|------|----------|
| `docs/` | Reverse-engineering notes: formats, rendering, timeline, champions, combat and magic, creatures, items, UI, audio, saves. Start at `docs/README.md`. |
| `crates/dm2-formats` | Parsers for `GRAPHICS.DAT` (archive, index, images) and `DUNGEON.DAT` |
| `crates/dm2-engine` | The game: state, timeline, mechanics, champions, combat, magic, missiles, creatures and AI, rendering, UI, input, audio, save games |
| `crates/dm2` | The windowed frontend (macroquad, cpal) |
| `tools/` | Python analysis tools: LE unpacker, archive/dungeon dumpers, reference renderers |

## Testing

```
cargo test
```

Tests that need the original data skip themselves when it is absent.

## Status

All of the game's systems are implemented; remaining approximations are
listed in each doc's open-questions or implementation-status section.
Still to be confirmed against the original game: the real-time tick
length, compatibility with save files written by the DOS game, and the
music driver's exact voice allocation and volume curve.

## License

The code, tools and notes in this repository are released under the MIT
License (see `LICENSE`). The license does not cover *Dungeon Master II*
itself: the original game's files, graphics, sounds, music and text remain
the property of their copyright holders and are not part of this
repository. You need your own copy of the game to run the remake.
