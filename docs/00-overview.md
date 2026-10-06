# Overview of the distribution

Source: GOG-style archive `Dungeon_Master_II_-_The_Legend_of_Skullkeep_1994.zip`
extracted to `original/` (gitignored). Paths below are relative to
`original/dumast2/`.

## Launch chain

`DM2.BAT` runs `EREGCARD` (registration card, irrelevant), then
`IBMIOP SKULL.EXE +VS`. `IBMIOP.EXE` is a small real-mode launcher that
starts the protected-mode game. The `+VS` switch still needs to be
identified (suspected: VGA/sound selection).

The wrapper `dosbox.conf` at the archive root mounts `cd/DM2.cue` as drive D.

## Files

| File | Kind | Notes |
|------|------|-------|
| `SKULL.EXE` | LE (DOS/4GW), Watcom C/C++32, 1994 | The game. See `01-executable.md`. |
| `DOS4GW.EXE` | DOS extender | Not needed by the reimplementation. |
| `FTL`, `SPLASH`, `INTRO`, `END`, `CREDITS` | 16-bit MZ executables | Logo, intro, ending and credits sequences; each probably carries its own embedded animation data. To be examined separately. |
| `DATA/GRAPHICS.DAT` | archive, 8.6 MB | Signature word `0x8005`, then entry count `5624`. Holds graphics, sounds and probably text. |
| `DATA/DUNGEON.DAT` | 39 KB | The dungeon. Possibly compressed (the original DM used a compressed variant). |
| `DATA/SONGLIST.DAT` | 63 bytes | Byte list (values 0x00-0x1C), padded with 0xFF. Probably a map from a level or area to a music track. |
| `SETUP.EXE`, `SETUP.INI`, `SKULL.CFG` | Sound setup | HMI SOS driver configuration (digital + FM MIDI). |
| `HMI*.386`, `*.BNK`, `TEST.HMP`, `TEST.RAW` | HMI sound system | FM instrument banks (`MELODIC.BNK`, `DRUM.BNK`) matter for music playback. |
| `cd/DM2.img` (+cue/ccd/sub) | CD image, 36 MB | Contents not yet examined. |

## Data file names referenced by SKULL.EXE

A pointer table in the data object (around 0x73000) lists the file names.
Each starts with a `.Znnn` token, which the runtime replaces with a
directory prefix:

- `DUNGEON.DAT`, `DUNGENB.DAT` (second dungeon variant, missing from this
  release; purpose unknown)
- `GRAPHICS.DAT`, `GRAPHIC2.DAT` (second graphics file, also missing;
  maybe optional)
- `SKSAVE*.DAT` (save games)

`DATA\SONGLIST.DAT` is referenced by a literal path, without the token.
