# Save games (SKSAVEn.DAT)

Verified from the writer and reader code. No real save file has been
checked yet, so byte offsets inside the bit-packed part should be
confirmed against one before relying on them.

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

The reader (0x34536) zero-fills masked-out bits, so fields that aren't
saved come back as 0. The mask tables are in the data object:

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

The mask leaves gaps (for example only the low 3 bits at 0x08 and the low
5 bits at 0x0A and 0x0C), so the stream is much shorter than 60 bytes.

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

**Thing chains** (0x3491A): for each thing until 0xFFFE or 0xFFFF:
- For types 4 and above: a 1 bit (more follows), the type (4 bits), and,
  unless it is a creature or the caller doesn't want it, the cell
  (2 bits). Types 0-3 (doors, teleporters, text, actuators) stay where
  the snapshot put them.
- The record, bit-packed with a per-type mask (pointer table at 0x754D7).
  The `next` link is never saved (its mask is 0); lists are rebuilt from
  the order written. Masks, as the bits kept in each word after `next`:

| Type | Mask (record words 1-3) | Notes |
|------|-------------------------|-------|
| 0 door | word 1: 0x3E00 | Only the state bits |
| 2 text | word 1: 0x0001 | The visible flag |
| 3 actuator | byte 4: bits 0 and 2 | For actuator types 0x1B, 0x1D, 0x27, 0x2C, 0x2D, 0x30, 0x32 and 0x41, an extra 9-bit value (word 1 >> 7) |
| 4 creature | words 3-6 mostly, plus 0x0780 of word 7 | Possessions follow as a nested chain. An alternative mask (0x7548B) applies to some creatures (0x1F9A3). |
| 5 weapon | 0x3DFF | Kind, flags and charges |
| 6 clothing | 0x1FFF | |
| 7 scroll | 0xFFFF | |
| 8 potion | 0xFFFF | |
| 9 container | word 2: 0xE000, word 3: 0x0400 | Then the 2 state bits of byte 4, then the contents chain. Some containers (0x1FA8F) save a single "has contents" bit instead and are handled by cross-reference. |
| 10 misc | 0xC0FF | Inside money containers the mask switches to 0xFF7F so the stack count is kept |
| 14 missile | words 2-3: 0xFFFF, 0x03FF | Then the carried thing as a chain |
| 15 cloud | 0xFFFF | Clouds tied to a timer of type 0x19 write a 1 bit and the timer index (10 bits) instead |

## Loading sequence (0x370D2)

1. Read the 42-byte header and keep its first word (0x7F928).
2. Parse the dungeon snapshot with 0x36909(0).
3. Read the bit-packed blocks into the globals, champions and timers.
4. 0x35B97: rebuild the inventories and leader hand, clear every square's
   thing list, read each square's bits and things back in, and link
   things through their `next` words. Fix-ups follow (0x35B0E).
5. Post-processing (0x36EF6, 0x55F4F, 0x594A1), then 0x4AE20 places the
   party on (x, y, map). The .BAK rename happens if the .BAK was used.

## Open questions

- Confirm all offsets against a real save written by the game (DOSBox).
- The meaning of the globals at 0x716A0, 0x7F19C, 0x7F270 and the
  environment bytes.
- The exact champion mask fields (to be aligned with `06-champions.md`).
- When DUNGEON.FTL exists (a shipped "start of game" save?).
