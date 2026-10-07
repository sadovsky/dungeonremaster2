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

Behaviour of the play function, in order (ported in `audio/sfx.rs`):
- **Map check.** A request with mode 1 or more is accepted only when the
  current map is the party's map (or the linked seam map 0x7F278, not
  modelled). Modes below 1 always pass.
- **Queue check.** At most 20 positional requests per tick; at most 6 in
  the interface queue (negative mode).
- **Registration.** Only keys registered for the party's map play (see
  "Sound registration" below); 0x15C10 returns 0 for any other key.
- **Sleep.** While the party sleeps (0x7F234) the volume is halved.
- **Delay.** Mode 2 or more goes to the delayed queue (below) instead.
- **Position.** The source square is made relative to the party and
  rotated into the facing (four cases, one per direction): right and
  forward offsets. A source on another map has its position corrected by
  the two maps' origins first.
- **Duplicates.** If the same sample is already queued from the same
  relative position this tick, the request is dropped.
- **Reachability.** When the source is more than one square away
  (|right| + |forward| > 1), the party's sound-distance grid decides: if
  it doesn't reach the source square the request is dropped; if the path
  is longer than the straight distance, both offsets are scaled up to the
  path length (rounded to nearest).
- **Mode.** 0 plays at once, 1 joins the pending positional queue,
  negative joins the interface queue.

The driver turns a queued request into a voice (0x10877):
- volume out of 255 = (volume × 256 / (right² + forward² + 8)) / 32, so a
  sound at the party's square plays at its volume byte and falls off with
  the square of the distance (one third at 4 squares, one ninth at 8);
- the pan word (0x8000 = centre) comes from two tables: a sound straight
  ahead or behind is centred and one exactly to the side takes the end
  entries; otherwise r = (|right| << 11) / |forward| is compared against
  eight descending thresholds (0x704DE, the last 0) and the first one not
  above r gives index *i*, which picks entry *i* of the sixteen-word pan
  table (0x704EE) for sounds to the left and entry 15 − *i* for the right.
  The remake reads both tables from the user's SKULL.EXE;
- when voices run short, higher priority (the play function's fourth
  argument) and then louder voices win (0x1084D).

Volume 200 and 0x80 are the common literal volumes; (0x18, 0, 0x89) is a
frequent interface sound.

**Modes used by the callers:** eating and the volume click use mode 0;
the party's own teleport sound uses −1 (interface queue); thunder uses a
computed delay; every other caller uses 1.

### Sound-distance grid (0x2F8F5, read by 0x2FACC)

A per-square byte map of the party's map (and of the seam map, when
there is one): steps from the party plus one, found by a breadth-first
search of up to 8 steps through the creature planner (0x3188A), with
squares that block movement marked by bit 7. 0x2FACC reads it for a
square: a blocking square (a wall carrying an actuator, a closed door)
takes the smallest value of its four neighbours; 0 means not reached and
the sound is dropped. The grid is rebuilt whenever its flag (0x7F38C bit
2) is set: on loading a game, when the party moves, when a door steps or
is destroyed, when a trick wall changes, and on map change. Bit 1 of the
same flag requests a light rebuild instead.

The remake builds the grid from the movement blocking rule each tick
that has sound requests (`SoundGrid`).

### Sound registration (0x3AB31 → 0x3D826 → 0x161D9)

While loading a map, 0x3AB31 builds a list of key patterns (category,
index, type, sub, each possibly "any", plus sub ranges) for everything
the map needs. 0x3D826 walks the archive index against that list and,
for every matching sound entry (type 2, or a pattern with type "any"),
registers the key; 0x1623B then loads the registered samples, and
0x163C2 releases them all on the next map change. The sound-relevant
patterns are:

| Category | Indexes registered |
|----------|--------------------|
| 1, 7, 0x10, 0x15, 0x18 | all |
| 3 | 0 (global) and map number + 1 (the map's wall actuator sounds) |
| 8 | 0xFE and the map's graphics set |
| 0x17 | the map's environment set |
| 9, 0x0A | 0xFE and the map's wall / floor ornaments |
| 0x0B, 0x0E | the map's door ornaments and door types |
| 0x0D | 0, 0x2F, 0x7E, 0x9F |
| 0x16 | 0xFE, the recruited champions, and portrait actuators (0x7E) on the map |
| 0x1A | 0x80, 0x81 |
| 0x0F | creature types, see below |

Creature types get a flag byte. Bit 0 (all of the type's keys) is set
for types in the map's creature list, types whose attribute 6 is set,
and types made by a creature generator (wall actuator 0x2E) on the map.
Bit 1 is set for types that live on a map linked to this one by a
map-edge actuator (0x27). A type with bit 0 registers everything; a type
with only bit 1 registers just subs 0xFA–0xFD (its animation tables), so
its sounds never play. Two option flags (0x803FA, 0x803FB) widen the
lists; both are assumed clear.

The remake builds the set in `audio/registry.rs`, taking categories
0x0B and 0x0E whole rather than narrowing them to the map's door types.

### Delayed playback (0x160DB, event 0x15)

A request with mode 2 or more is stored in a free slot of an 8-entry
table (category, index, sub, priority, volume, map, x, y) and timeline
event 0x15 is scheduled for `mode − 1` ticks later, with the slot number
in the event's x/y bytes. When the event runs, the slot is replayed as an
ordinary mode-1 request if its map is still the party's map, and the slot
is freed either way. With all 8 slots in use the request is dropped. The
remake keeps the table on the game state (`sound_queue.rs`).

**Thunder** (0x5A073) uses it: sound (0x17, map set, 0) at the party's
square, volume 0x40, delayed by `0x4C − rain / pattern multiplier` ticks
while it rains, or `random(10) + 5` with no rain, clamped to 1–15. The
remake uses the rain formula but a fixed 9 for the no-rain case, without
the random draw; that draw belongs with the rest of the game RNG's draw
order.

### End of the game

Neither death of the last champion nor the end-game actuator changes the
song. The ending routine (0x2005B) releases every registered sound, then
0x10D29 shuts both the digital and the MIDI driver down (0x1043C) and
leaves with an ending code for the launcher, which runs the ending
movie. The remake stops the music and all effects once the game is over.

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

**Home-square signal (0x31348).** Whenever owed damage is applied to a
creature whose type flag 0x01 is clear and whose AI class lacks flag
0x04, a clear action is sent to the creature's home square (slot +0x0C)
for the next tick (0x4BBE4), whether or not the creature survives.

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
- **Voice allocation and volume curve:** HMI's driver isn't reversed. The
  FM volume curve is fitted to the recordings (see "Mixer levels").
  Music pan routes a voice left, right or to both sides, as OPL3 does.
- **Drum pitch:** channel 9 plays DRUM.BNK patch *note* at that note's pitch.
- **Loop controllers:** 110/111 are treated as loop start/end, with the
  110 value as a repeat count (0 or 127 means forever). Finished songs restart.
- **Song numbering:** SONGLIST values are used directly as the song index,
  with 0 as silence.
- **Music update:** `Audio::music_tick` reproduces 0x10AF6 (126-tick
  fade, counter as volume), driven once per game tick by the frontend and
  by `dm2 --replay --audio`. The title screen is silent, as in the
  original.
- **Sound effects:** the play function's rules are ported: the map check,
  per-map registration, the sound-distance grid with path stretching, the
  driver's distance attenuation and the halved volume while asleep.
  `Effect::SoundAt` carries the original's volume and mode where they
  differ from 200 and 1. The pan table is ported. Not modelled: the seam
  map 0x7F278, voice priority, and the no-rain random draw in the thunder
  delay.
- **Mixer levels:** see "Mixer levels" below.
- **Live check:** `DM2_AUDIO_DUMP=path.wav` writes everything the live
  game plays to a WAV file.

## Mixer levels

**Traced (code):**
- **Options levels** (0x10736): the volume control holds a level 0-7 per
  path. Digital: level *l* becomes the 15-bit master volume
  `l<<12 | l<<9 | l<<6 | l<<3 | l` (0x7FFF at 7), handed to the HMI digital
  driver. Music: level *l* becomes the MIDI master byte `l*2 | l<<4`
  (90 at 5, 126 at 7); level 0 stops the music and raising it restarts the
  last song.
- **Defaults:** digital level 7 (the high word of 0x7169A, applied when the
  digital driver starts) and music level 5 (set at 0x1041D), so effects
  play at full scale and music at master 90.
- **Song start:** each song starts at song volume 0x7F (0x62012); the fade
  then lowers that volume one step per tick (see "Music update").
- **Request volume:** the driver turns a request's volume byte into a
  0-255 voice volume with the distance law above; the remake plays samples
  at voice volume / 255 times the digital master.

**Fitted to the original's recordings (DOSBox, SB16 + OPL3):**
- **FM loudness law:** a note's amplitude goes as
  `(velocity / 127)^1.0 x (volume x expression / 127^2)^1.4`, applied as
  extra operator attenuation of −20 log10(amplitude) in 0.75 dB total-level
  steps. The previous linear total-level scaling crushed softly played
  songs: map 0's song (lead channel velocity about 66) came out 9 dB too
  quiet while a louder song matched.
- **Fade law:** during a fade the song volume scales amplitude as
  `(volume / 127)^2.5`, which reproduces the original's 8 dB drop over the
  first six seconds of the stairs segment.
- **Overall music level:** scaled so map 0's song matches the original
  (cave and inventory segments within 0.5 dB). A different options level
  scales music relative to level 5 with the same exponent as the fade;
  this is an assumption.
- **Voice count:** limiting the synth to 9 voices made no measurable
  difference, so 18 are kept.

**Comparison** (remake replay against the original's recordings, RMS over
the windows where the original has signal; before = linear total-level
curve, linear fade, effects at 0.8):

| Segment | Before | After |
|---|---|---|
| cave (map 0) | −9.2 dB | +0.4 dB |
| inventory (map 0) | −9.4 dB | +0.3 dB |
| door (map 2) | −16.4 dB | +3.1 dB |
| stairs (map 8) | −0.6 dB | +4.9 dB |
| outdoor (map 1) | +15.8 dB | +17.7 dB |

**Open:**
- **No effects in the recordings:** none of the five DOSBox recordings
  contains a sound-effect transient (the routes include walking, turning,
  a door, stairs and a creature in view), so the original's digital output
  apparently did not reach them, most likely because the digital path did
  not start under that DOSBox configuration. The effects level therefore
  comes from the traced driver levels (unity at level 7), not from the
  recordings, and the outdoor segment, where creature sounds dominate the
  remake, cannot be compared until the original is recorded with working
  digital sound.
- **Stairs offset:** before its fade the stairs segment stays about 4.7 dB
  louder than the original under every curve tried and with 9 or 18
  voices, so it is not the loudness law. Candidates: the music state the
  original restores after loading a save, or song 15's patches.
- **Assumed, not verified:** that DOSBox mixes the SB digital and OPL
  outputs at equal level (so digital full scale equals the calibrated FM
  scale), and that HMI's digital master volume scales samples linearly.
