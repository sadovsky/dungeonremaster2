# Save games (SKSAVEn.DAT)

Verified from the writer and reader code, and implemented in
`crates/dm2-engine/src/save.rs`. No save written by the DOS game has been
checked yet: everything below, and whether the DOS game accepts the
remake's files, still needs confirming against a real SKSAVE made in
DOSBox (see "Open questions").

## Files

| Name | Use |
|------|-----|
| `SKSAVEn.DAT` | Save slot *n* (digit at 0x7F998 = '0' + slot) |
| `SKSAVEn.BAK` | Previous contents of the slot. Saving renames the old file to .BAK first; loading falls back to it when the .DAT can't be opened. |
| `DUNGEON.FTL` | Last-resort fallback for loading: a save-format snapshot of the dungeon. Used when neither slot file opens and no game has been loaded yet. |

The names come from the pointer table at 0x73000 (`.Z022` and `.Z023`
are path and slot substitutions, see `13-text.md`).

## Code

| Address | Role |
|---------|------|
| 0x3502B | Save (command 0x8C): dialogs, slot choice (0x3780F), file handling, then the sections below |
| 0x370D2 | Load: slot choice (0x376B6), tries .DAT, then .BAK, then DUNGEON.FTL |
| 0x346AA / 0x34683 | Write / read a raw block |
| 0x343BE / 0x34536 | Write / read a bit-packed block (below) |
| 0x343AF | Reset the bit buffer |
| 0x344C0 | Flush the last partial byte |
| 0x3449B | Write one bit |
| 0x3491A / 0x3573A | Write / read a thing chain |
| 0x34E73 / 0x35B97 | Write / read the per-square dynamic state |
| 0x34DFE | Write the things held by timers |
| 0x34797 | Write creature and missile cross-references |
| 0x36909 | Dungeon section parser, shared with DUNGEON.DAT; argument 0 = loading a save, so no spare slots are added |
| 0x3426D | Before saving: deactivate every active creature slot (0x3085A) |
| 0x55FDC / 0x55DBD / 0x56030 | Before saving: compact the timer array, rebuild the heap, rebuild the free list |
| 0x55F4F | Before saving: refresh stored timer indices (champion +0x2E for type 0x0C, missile word 3 for types 0x1D/0x1E) |
| 0x34236 / 0x34106 | After saving: walk every map and reactivate its creatures |

## Layout

### 1. Header (42 bytes, raw)

| Offset | Size | Field |
|--------|------|-------|
| 0 | u16 | Format marker, written as 1 |
| 2 | 40 | Save name, NUL-terminated (copied from the name the player typed, 0x7F8FE) |

The save reads the slot's existing header first, so an overwritten slot
keeps any bytes the writer does not set.

### 2. Dungeon snapshot (raw)

The same sections as DUNGEON.DAT, in the same order, without the trailing
checksum:
- 44-byte header
- map descriptors
- column table
- object list
- text
- 16 thing arrays
- map data

See `03-dungeon-dat.md`. The counts in the header are the current ones,
which include the free slots added when the game started.

On load, 0x36909 rebuilds the static structure (maps, actuators, text)
from this. The thing lists are then rebuilt from part 4, which overrides
the dynamic fields.

### 3. Global state (bit-packed)

All remaining data goes through a bit packer:
- Each structure has a mask table with one byte of mask per byte of data.
- For every data byte, only the bits set in its mask are written,
  highest bit first.
- The bits go into one continuous stream, flushed 8 bits at a time.
- A byte whose mask is 0 takes no space at all.

The reader (0x34536) leaves masked-out bits as they already are in the
destination (it starts from the existing byte), so unsaved fields keep
whatever the loader had there. The final partial byte is rotated so the
pending bits sit at the top; the low bits are zero.

The mask tables are in the data object:

| Block | Size × count | Mask | Contents |
|-------|-------------|------|----------|
| Globals | 60 × 1 | 0x75316 | Built from scattered globals; see the next table |
| Flags | 8 × 1 | 0x75312 (all bits) | 0x7F100: global flag bytes |
| Global bytes | 1 × 64 | 0x75312 | 0x7F0C0: 64 byte variables (dungeon scripting) |
| Global words | 2 × 64 | 0x75312 | 0x7F108: 64 word variables |
| Champions | 263 × champion count | 0x75352 | The champion records (base 0x7FBD0, stride 0x107) |
| Misc | 6 × 1 | 0x75459 | 0x7FFEC (only bytes 3-4 saved) |
| Timers | 12 × timer count | 0x7545F | Timer queue entries (0x760E8); entries past the count are cleared on load |

The 60-byte globals record:

| Offset | Size | Source | Meaning (working) |
|--------|------|--------|-------------------|
| 0x00 | u32 | 0x7F22C | Game tick (also copied to 0x7F218 on load) |
| 0x04 | u32 | 0x70518 | Random seed or state |
| 0x08 | u16 | 0x7F276 | Number of champions |
| 0x0A | u16 | 0x7F26A (high) | Party x |
| 0x0C | u16 | 0x7F26E | Party y |
| 0x0E | u16 | 0x7F252 (high) | Party facing |
| 0x10 | u16 | 0x7F260 (high) | Current map |
| 0x12 | u16 | 0x759A2 (high) | Leader index |
| 0x14 | u16 | 0x8042A | Timer count |
| 0x16 | u32 | 0x716A0 | Unknown |
| 0x1A | u32 | 0x7F19C | Unknown |
| 0x1E | u16 | 0x7F26A (low) | Movement state |
| 0x20 | u16 | 0x7F25A | Movement state |
| 0x22 | u16 | 0x7F270 | Unknown |
| 0x28 | u16 | 0x7169C / 0x7169E | Two 4-bit values, packed |
| 0x2A-0x3B | | 0x80470-0x80480, 0x80430 | Environment state (weather or sky, category 23); see `04-rendering.md` when written |

The mask leaves gaps, so the stream is much shorter than 60 bytes. As read
from the table:
- tick: low 24 bits only;
- random state: low 16 bits only, so the DOS format cannot restore the
  generator exactly (bits 16-23 feed later outputs);
- champion count 3 bits, x and y 5 bits, facing 2 bits, map 6 bits,
  leader 2 bits, timer count 9 bits;
- 0x16 and 0x1A are kept in full. They are the tick of the last creature
  attack on the party (0x716A0) and of the party's last move (0x7F19C),
  see `06-champions.md`.

The timer mask keeps the tick (24 bits), the map (6 bits), the type
(7 bits) and bytes 5-9, but **not** the word at +10. The misc mask keeps
only bytes 3 and 4 of 0x7FFEC, i.e. 0x7FFEF and 0x7FFF0 (the two party
counters in `06-champions.md`).

### 4. Dynamic objects (bit-packed, same stream)

1. **Timer-held things** (0x34DFE): for every timer of type 0x3C or 0x3D,
   the thing chain it references.
2. **Inventories:** for each champion, its 30 slots, then the leader-hand
   item, each as a thing chain (0x3491A).
3. **Squares** (0x34E73): for each map, column and row:
   - Changeable square bits, depending on the element type:

     | Element | Bits saved |
     |---------|------------|
     | Door | 3 (open state) |
     | Pit | 1 (bit 3, open or closed) |
     | Teleporter | 1 (bit 3, unless certain conditions apply) |
     | Trick wall | 1 (bit 2) |
     | Others | none |

   - Then the square's thing list. A square with no list (bit 4 clear)
     writes just the end marker.
4. **Cross-references** (0x34797): for each creature and missile written,
   its index in the rebuilt arrays (10 bits).
5. **Flush:** the final partial byte is padded with zero bits.

**Thing chains** (0x3491A, arguments: first thing, "write cells",
"whole list"): for each thing until 0xFFFE or 0xFFFF:
- For types 4 and above: a 1 bit, the type (4 bits), and, when cells are
  wanted and the thing isn't a creature, the cell (2 bits). Types 0-3
  (doors, teleporters, text, actuators) get no leading bit; they stay
  where the snapshot put them.
- Fields written before the record: an actuator of one of the listed
  types writes word 1 >> 7 (9 bits); a creature writes its type byte
  (+4, 7 bits); a container writes bits 1-2 of word 2.
- A whole list ends with a 0 bit. A single-thing chain (an inventory
  slot, the leader's hand, a timer's thing, a missile's payload) writes
  no terminator, except a single 0 bit when the slot is empty (0xFFFF).
- Inside a missile's payload a cloud writes only the low 7 bits of its
  record index, then stops.
- Creatures and containers are numbered in the order written; the
  cross-reference block (0x34797) uses those numbers: a "linked"
  container writes the number of the creature in its word 1, a missile
  inside a creature's possessions the number of the container in its
  word 1 (10 bits each).
- The record, bit-packed with a per-type mask (pointer table at 0x754D7).
  The `next` link is never saved (its mask is 0); lists are rebuilt from
  the order written. Masks, as the bits kept in each word after `next`:

| Type | Mask (record words 1-3) | Notes |
|------|-------------------------|-------|
| 0 door | word 1: 0x3E00 | Only the state bits |
| 2 text | word 1: 0x0001 | The visible flag |
| 3 actuator | byte 4: bits 0 and 2 | For actuator types 0x1B, 0x1D, 0x27, 0x2C, 0x2D, 0x30, 0x32 and 0x41, an extra 9-bit value (word 1 >> 7) |
| 4 creature | words 3-6 mostly, plus 0x0780 of word 7 | Possessions follow as a whole nested chain. Creature types with info flag bit 0 (0x1F9A3) use the alternative mask 0x7548B, and then their possessions' cells are written too. |
| 5 weapon | 0x3DFF | Kind, flags and charges |
| 6 clothing | 0x1FFF | |
| 7 scroll | 0xFFFF | |
| 8 potion | 0xFFFF | |
| 9 container | word 2: 0xE000, word 3: 0x0400 | Then the contents as a whole chain. Containers with (word 2 & 6) == 2 (0x1FA8F) use mask 0x754B3 and write only a "has contents" bit; a set bit registers them for the cross-reference block. |
| 10 misc | 0xC0FF | Inside a money container (state bits 0 and a (20, item, 5, 0x40) entry, 0x1F2AB) the mask switches to 0x754BF (0xFF7F) so the stack count is kept |
| 14 missile | words 2-3: 0xFFFF, 0x03FF | Then the payload as a single chain. A missile among a creature's possessions uses mask 0x754CF and is registered for the cross-reference block instead. |
| 15 cloud | 0xFFFF | Then a 1 bit and the index (10 bits) of the type-0x19 timer holding it, or a 0 bit |

The pointer table at 0x754D7 holds no mask for types 1 (teleporter) and
11-13, so those write nothing at all. Unrelocated, its entries are
offsets from the data object's base.

**Squares** (0x34E73): maps in order, then x, then y (storage order).
Bits saved per element: pit 0x08, door 0x07, trick wall 0x04, a plain
teleporter 0x08. A teleporter that is a map-edge link (0x1D113) saves no
bits, and its thing list is written only on the side whose partner map
has the higher index.

## Loading sequence (0x370D2)

1. Read the 42-byte header and keep its first word (0x7F928).
2. Parse the dungeon snapshot with 0x36909(0).
3. Read the bit-packed blocks into the globals, champions and timers.
4. 0x35B97: rebuild the inventories and leader hand, clear every square's
   thing list, read each square's bits and things back in, and link
   things through their `next` words. Fix-ups follow (0x35B0E).
5. Post-processing (0x36EF6, 0x55F4F, 0x594A1), then 0x4AE20 places the
   party on (x, y, map). The .BAK rename happens if the .BAK was used.

## The remake's files

`save.rs` writes the format above, then appends a trailer with the
engine's exact state (full random state and tick, all timer bytes, full
champion records, move cooldown, pending damage, deferred map change).
It ends with the trailer length (u32) and the tag `DM2R`. The DOS reader
consumes a fixed amount and should ignore it. Without the trailer the
loader uses the original fields only. Like the original, saving prepares
the live game first (creature slots freed, timers compacted), so
continuing after a save matches loading it; this is covered by a test.

The loader takes the dungeon from the snapshot as is and does not parse
the dynamic object part, since the snapshot already holds the current
records and lists. This assumes the DOS snapshot is a plain memory copy,
which is what the writer suggests.

## Open questions

- Confirm all offsets against a real save written by the game (DOSBox):
  in particular the chain terminators, the third argument of the square
  chains (assumed "whole list"), the cloud and cross-reference rules,
  and that the DOS game loads a remake save.
- The edge-link rule compares the partner map with the
  current one; which side counts as "lower" should be checked in a save
  from a dungeon with edge links.
- The leader's hand item (0x7FBB4) is not modelled by the engine yet; it
  is carried through `Legacy` unchanged.
- The meaning of the globals at 0x716A0, 0x7F19C, 0x7F270 and the
  environment bytes.
- The exact champion mask fields (to be aligned with `06-champions.md`).
- When DUNGEON.FTL exists (a shipped "start of game" save?).
