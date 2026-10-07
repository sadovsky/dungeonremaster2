# Audio

Tool: `tools/audio.py` (outputs to `re/audio/`, gitignored). Sound effects
and music both live in GRAPHICS.DAT; nothing on the CD image is needed for
in-game audio.

## Sound effects (GRAPHICS.DAT type 2)

- 292 index keys point at 107 distinct entries (about 124 s of audio in total).
- Entry layout: a 6-byte header, then raw samples.
  - u16 sample rate: 11127 Hz in every entry. That is roughly the
    Sound Blaster time-constant rate near 11 kHz.
  - 4 more bytes, always `08 01 00 00` (probably 8 bits per sample and
    mono; the game never reads them).
  - Samples are **unsigned 8-bit mono PCM**.
- The header is only honoured when bit 0x20 of the archive flags word
  (key 0,0,11,0) is set. It is set in this release (flags = 0x7B). With the
  bit clear, the game would skip only 2 bytes and play at a fixed 5500 Hz.
  The reimplementation can assume the 6-byte header.
- On first use the game flips the top bit of every sample in place,
  converting it to signed for the HMI SOS mixer. A tag word just before
  the buffer records that the conversion has been done. This doesn't
  affect a reimplementation; just play the samples as unsigned 8-bit.

### Keys

Sounds use the same (category, index, sub) addressing as images, so a
sound belongs to the thing that makes it:

| Category | Subs used | Likely meaning |
|----------|-----------|----------------|
| 3 (global / wall writing) | 0, 1, 129, 136, 137, 139 | Global interface and world sounds (doors, buttons, etc.) |
| 15 (creatures) | 0–18 per creature, plus 132 and 141 | Per-creature sounds (attack, movement, death ...); the creature index selects the set |
| 16–21 (item classes) | 133, 134 (weapons also 0, 1) | Shared item actions, probably hit/impact and drop/use |
| 22 (champions) | 0, 1, 130, 131, 135, 138 | Champion sounds (pain, hunger ...) |
| 8, 9, 10, 13, 14, 23, 24 | Various, mostly ≥128 | Map set, wall and floor ornaments, doors, missiles, environment |

Subs at or above 128 look like action codes shared across categories.
Their exact meanings still need to be traced from the call sites.

### Engine side (SKULL.EXE)

| Address | Role |
|---------|------|
| 0x15A91 | Allocate the sound tables: a slot table (16 bytes per loaded sample), a 7-byte key table, a 20-entry pending queue and a 6-entry interface queue |
| 0x161D9 | Register a sound key (category, index, sub) as wanted for the current map; duplicates are ignored |
| 0x1623B | Load every registered sound that is not yet loaded: look up (cat, idx, 2, sub), take the rate from the header, store the data pointer and length, convert to signed |
| 0x163C2 | Release all registered sounds (map change) |
| 0x15C10 | Find a registered key and return its 1-based slot |
| 0x15CA9 | **Play sound**: `(cat, idx, sub, ?, volume, x, y, mode)` |

Behaviour of the play function:
- The source map position (x, y) is made relative to the party and
  rotated into the party's facing (four cases, one per direction). The
  result gives left/right and front/back offsets used for panning.
- Volume falls off with distance (sum of the absolute offsets). Beyond a
  per-source audible range the sound is dropped. When an option flag is
  set, all volumes are halved.
- If the same sample is already queued from the same relative position
  this tick, the request is dropped.
- Mode: 0 means the sound plays at the party's position; 1 means it plays
  at the given (x, y), and for a different map level the level offset is
  added in; a negative mode routes it through the separate 6-entry
  interface queue.
- At most 20 positional sounds can be pending per tick.
- Volume 200 and 0x80 are common literal volumes; 0x18,0,0x89 is a
  frequent interface sound.
- Only registered keys play: 0x15C10 searches the per-map table that
  0x161D9 fills while the resource manager loads a map's entries, and a
  key that isn't registered returns 0 and plays nothing. Which keys a map
  registers isn't traced yet.
- Positional sounds more than one square away are also dropped when the
  line-of-sight test 0x2FACC fails. The remake doesn't model this test.

### Sound events (callers of 0x15CA9)

| Caller | Event | Sound (cat, idx, sub) | Volume |
|--------|-------|------------------------|--------|
| 0x3023F | Creature animation frame with a sound field other than 0x7F | (15, type, field) | 0x80 |
| 0x31348 | Creature hurt but alive (see below) | (15, type, 9 or 10) | |
| 0x414A5 | A champion's blow lands on a creature | (15, type, 0x8D) | 200 |
| 0x2B35D | Creature transform: changed / not changed | (3, 0, 0x81 / 0x8B) | 200 |
| 0x22A68 | Empty-hand press on the square ahead when its flag 0x40 is set | (3, 0, 0x88) | 0x80 |
| 0x39B3F | Eating or drinking | (9, 0x5B, 0xFB) | 200 |
| 0x4A34A, 0x58FC2 | Teleporting | (0x18, 0, 0x89) / (3, 0, 0x89) | 0x80 |
| 0x4CDCC | Floor sensors | (10, ornament, 0x88) | |
| 0x57476, 0x58304 | Wall actuator sounds | (3, map + 1, record word 1 >> 3) | 200 |
| 0x5A073 | Thunder, 1-15 ticks after the flash | (0x17, map set, 0) | 0x40 |
| 0x15B4A | Volume control click | (3, 0, 0x8B) | 200 |
| 0x160DB | Plays the delayed-sound queue that mode ≥ 2 requests fill | | |

0x3F422/0x3F49D, listed earlier as sound functions, fetch and release
images in the drawing code; they aren't sound calls.

**Pain cry (0x31348).** When owed damage leaves a creature alive and its
type flag 0x01 is clear and its AI class has flag 0x8000, the game draws
a random number; one time in eight the creature cries out at once.
Otherwise, if the blow exceeds 3% of the type's info byte 2 or 5% of its
remaining hit points, a coin flip (only when its status word has bit 3)
and then a one-in-four roll decide it. The cry picks sub 9 or 10 with a
further random bit. These draws happen whether or not a sound plays.

## Music (GRAPHICS.DAT type 3)

- 29 songs, key (category 4, index *n*, type 3, sub 0), n = 0..28; each
  is an HMI HMP file (`HMIMIDIP013195` signature).
- **Song selection**: `DATA/SONGLIST.DAT` is a byte table indexed by the
  party's **current map number**, holding song numbers. It has 46 valid
  entries (the dungeon has 44 maps), padded with 0xFF to 63 bytes, and
  the game reads at most 63. Song 0 means silence (the loader returns
  early when the song is 0).
- **Music update (0x10AF6), once per game tick** from the main loop
  (0x24691) with the party's map. If the map's song differs from the last
  one chosen, it starts at once when nothing is playing or a fade is
  already running; otherwise a fade counter is set to 127. Each tick the
  counter becomes the music volume and steps down by one, so the fade
  lasts 126 ticks (about 17 s); at 1 the waiting song starts (0x1095D)
  at volume 127. A song 0 leaves the music at the faded volume.
- **Nothing else changes songs.** The title loop (0x386F5) makes no music
  calls, so the title screen is silent after the intro movie. Pausing
  stops the main loop but not the sound driver, so music keeps playing
  while paused. The options volume control (0x10736, via 0x15B4A) sets
  the music level 0-7 (5 at startup); 0 stops the music and raising it
  again restarts the last song.
- The game loads a song with a plain copy of the entry into a
  preallocated buffer, then hands it to the HMI SOS MIDI driver.
- `TEST.HMP` in the game directory is the setup program's test tune, not
  used in game.

### HMP format (variant `HMIMIDIP013195`)

| Offset | Field |
|--------|-------|
| 0x00 | `HMIMIDIP` plus a 6-character version date (`013195`), padded |
| 0x20 | u32 file size |
| 0x30 | u32 track count (including track 0) |
| 0x34 | u32: 192 in every file. **Not** the timing base. |
| 0x38 | u32 **tick rate in Hz**: 120 in every file. Verified: the last event tick divided by the song-length field gives about 120 for all 29 songs. |
| 0x3C | u32 song length in seconds |
| 0x40 | Per-channel tables (priority, device mapping); not needed for playback |
| 0x388 | First track chunk (0x308 in the older, undated `HMIMIDIP` variant) |

Each track chunk has a 12-byte header (u32 track index, u32 chunk length
including the header, u32 MIDI channel), followed by event data.

How events differ from Standard MIDI:
- **Delta times** are variable-length but reversed: 7-bit groups,
  least-significant first, and the byte with bit 7 **set** is the *last*
  one. MIDI uses the opposite convention.
- Everything else (running status, channel messages, `FF` meta with a
  1-byte length, `FF 2F 00` end of track) follows MIDI. Track 0 carries
  no tempo event; timing comes only from the header tick rate.

`tools/audio.py music` converts each song to a Type 1 MIDI file with
division = 120 and tempo = 1,000,000 µs per quarter, so one tick is one
120 Hz HMP tick. The output is structurally verified (every track ends
exactly on its end-of-track event, and the lengths match the headers).

### FM instruments (MELODIC.BNK, DRUM.BNK)

DM2 supports FM (OPL2/OPL3) music only, not General MIDI (README). The
two banks are AdLib `.BNK` files whose 6-byte signature is altered
(`AMLIB-` and `ANLIB-` instead of `ADLIB-`). Otherwise they follow the
standard layout:

| Offset | Field |
|--------|-------|
| 0 | u8 major, u8 minor version (0.0) |
| 2 | Signature (6 bytes) |
| 8 | u16 number of instruments used (128), u16 total (128) |
| 12 | u32 offset of the name table (0x1C), u32 offset of the instrument data (0x61C) |
| name table | 128 × 12 bytes: u16 data index, u8 used flag, 9-byte name |
| data | 128 × 30 bytes: u8 percussive, u8 voice, two 13-byte operator blocks, two wave-select bytes |

128 × 30 + 0x61C = 5404, which is exactly the file size. MELODIC.BNK
maps General MIDI programs 0–127 to OPL patches. DRUM.BNK holds the
percussion patches, presumably selected by note number on MIDI channel 9.
The game loads both at startup (0x10225) before reading SONGLIST.DAT.

### Plan for the reimplementation

Faithful music needs an OPL emulator, such as an `opl3`/Nuked-OPL3 port
in Rust, driven by an HMI-style MIDI-to-OPL voice allocator that uses
these two banks. A simpler first step is to render the converted MIDI
with any soft synth. The voice-allocation rules in HMI's driver aren't
reversed here, because they live in `HMIMDRV.386`, not in SKULL.EXE.

## Other distribution sound files

- `TEST.RAW` (40,320 bytes): the setup program's digital test sample.
- `HMI*.386`: HMI SOS driver modules (detect, digital, MIDI). Not needed.
- `SKULL.CFG`: the chosen digital and MIDI devices (Sound Blaster settings).

## Implementation (crates/dm2-engine/src/audio)

| Module | Role |
|--------|------|
| `bnk.rs` | BNK bank parser (128 patches per bank) |
| `hmp.rs` | HMP parser into per-track event lists |
| `opl.rs` | Two-operator FM voice written from the OPL2 datasheet behaviour (pitch quantised to F-number/block, envelope rates, KSL, feedback, the four waveforms, LFOs) |
| `midi.rs` | MIDI-to-FM driver, 18 voices |
| `music.rs` | Sequencer with HMI loop controllers 110/111, and song selection from SONGLIST.DAT |
| `sfx.rs` | Sample decoding, positional placement and the effect voices |
| `mod.rs` | `Audio` mixer: `render(&mut [f32])` produces interleaved stereo |

The frontend (`crates/dm2/src/sound.rs`) feeds a cpal stream from the
mixer and drains `Effect::Sound` requests after each tick. `DM2_MUTE=1`
disables it. `cargo run --release -p dm2-engine --example songwav -- N`
renders song N to `re/audio/` for listening checks.

Not from the original (tentative):
- **Voice allocation, volume curve and pan:** HMI's driver isn't reversed.
  Pan routes a voice left, right or to both sides, as OPL3 does.
- **Drum pitch:** channel 9 plays DRUM.BNK patch *note* at that note's pitch.
- **Loop controllers:** 110/111 are treated as loop start/end, with the
  110 value as a repeat count (0 or 127 means forever). Finished songs restart.
- **Song numbering:** SONGLIST values are used directly as the song index,
  with 0 as silence.
- **Music update:** `Audio::music_tick` reproduces 0x10AF6 (126-tick
  fade, counter as volume), driven once per game tick by the frontend and
  by `dm2 --replay --audio`. The title screen is silent, as in the
  original.
- **Sound effects:** attenuation is linear with distance over an 8-square
  range, and each level of map difference counts as 2 squares. A request's
  volume scales its gain relative to 200 (`Effect::SoundAt` carries the
  original's value where it differs, e.g. 0x80 for creature frames). Not
  modelled: the per-map registration of playable keys and the
  line-of-sight drop, so the remake can play sounds the original
  wouldn't. At the outdoor spot map 1 (2,9), creature frame sounds made
  the remake's track about 20 dB louder than the original's.
- **Live check:** `DM2_AUDIO_DUMP=path.wav` writes everything the live
  game plays to a WAV file.
