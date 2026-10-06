# DUNGEON.DAT

The world: 44 maps (areas), the squares in them, and every door,
teleporter, text plate, actuator, creature and item placed in them. The
layout is the original Dungeon Master's, with DM2's own record sizes and
per-map fields.

- Python reader: `tools/dungeon.py` (`summary`, `map N`, `things TYPE`)
- Rust parser: `crates/dm2-formats/src/dungeon.rs`

Confidence tags used below:
- **code**: read directly from SKULL.EXE.
- **data**: consistent with every record in the file but not yet traced in code.
- **DM1**: assumed from the first game's format and not yet confirmed.

## Loader (SKULL.EXE)

| Address | Role |
|---------|------|
| 0x37058 | Opens `DUNGENB.DAT` if a flag (0x803F8) is set, otherwise `DUNGEON.DAT` (error 0x32 if neither opens), then calls the loader |
| 0x370D2 | New game or load game entry point. It tries `SKSAVEn.DAT`, then `.BAK`, then `DUNGEON.FTL`. A save file starts with the same dungeon block, followed by game state. |
| 0x36909 | Main parser (`load_dungeon(new_game)`); see the steps below |
| 0x34683 | Reads n bytes from the open file (n = 0 is a no-op) |
| 0x1C724 | Makes a map current: caches its descriptor, column pointers and width/height |
| 0x1CA96 | Square byte at (x, y) on the current map, with edge handling across neighbouring maps |
| 0x1D319 | Square to object-list index (counts bit-4 squares above it in the column) |
| 0x1D391 | Square to first thing (0xFFFE if none) |
| 0x1D2DA | Thing reference to record pointer |
| 0x1DFA0 (approx.) | Text and scroll display: unpacks text things |

Load sequence in 0x36909:
1. Read 8 bytes. If the first word is 0x8104, the file is a compressed
   dungeon, which takes a different path not covered here (this release
   is uncompressed).
2. Seek back to the block start (0 for DUNGEON.DAT; a save file has a
   0x2A-byte preamble first) and read the 44-byte header.
3. Read the map descriptors, then the column table, the object list, the
   text block, the 16 thing arrays and the map data, in that order. Each
   is sized from the header.
4. Build the per-map column pointer tables and a depth-to-maps index
   (63 buckets, keyed on descriptor word 8 bits 0-5).
5. For a new game (`new_game != 0`): add 300 spare `0xFFFF` slots to the
   object list, and add the spare record counts from the table at 0x71818
   to each thing array. The spare records are initialised with
   `next = 0xFFFF` (free). Creature records also get byte 5 set to 0xFF.

The trailing checksum is not checked in 0x36909; whether something else
checks it is still open.

## File layout

All integers are little-endian u16 unless noted.

| Section | Size | Notes |
|---------|------|-------|
| Header | 44 | see below |
| Map descriptors | 16 × map count | |
| Column table | 2 × (sum of map widths) | 725 words |
| Object list | 2 × object-list words | 2,360 words: 1,592 used, 768 spare |
| Text | 2 × text words | 28 words |
| Thing arrays | Σ size[t] × count[t], t = 0..15 | |
| Map data | map-data size | 12,615 bytes |
| Checksum | 2 | sum of all preceding bytes, mod 65,536 |

Total: 39,435 + 2 = 39,437 bytes, which matches the file exactly
(verified; the checksum is 0x323A).

### Header (code)

| Offset | Field |
|--------|-------|
| 0 | Random seed (0 here). The word 0x8104 at this position marks a compressed file. |
| 2 | Map data size in bytes |
| 4 | u8 map count (44); byte 5 is unused |
| 6 | Text size in words |
| 8 | Party start: x = bits 0-4, y = bits 5-9, facing = bits 10-11. Always on map 0. |
| 10 | Object list size in words |
| 12 | 16 × u16 thing counts, indexed by thing type |

### Map descriptor (16 bytes)

| Offset | Bits | Field | Source |
|--------|------|-------|--------|
| 0 | u16 | Offset of the map's squares in the map data | code |
| 2 | bit 7 | `door_type0` (+14 bits 8-11) is valid | code |
| 2 | bit 8 | `door_type1` (+14 bits 12-15) is valid | code |
| 2 | bits 0-4, 6 | Each one suppresses a specific optional piece of the map graphics set (category 8) when building the graphics cache | code |
| 2 | bit 5 | Also load category-24 graphics for this map | code |
| 4 | bits 0, 2 | Map state flags, changed at runtime by map-transition code | code (meaning TODO) |
| 6 | u8 | X origin within its depth layer, used to line up adjacent maps | code |
| 7 | u8 | Y origin | code |
| 8 | 0-5 | Depth / layer. Maps on the same layer are grouped; for example the outdoor areas are layer 6. | code |
| 8 | 6-10 | width − 1 | code |
| 8 | 11-15 | height − 1 | code |
| 10 | 0-3 | Number of wall ornaments in the map's list | code |
| 10 | 4-7 | Always 0 here (random wall ornament count in DM1) | DM1 |
| 10 | 8-11 | Number of floor ornaments in the map's list | code |
| 10 | 12-15 | Always 0 here | |
| 12 | 0-3 | Number of door ornaments in the map's list | code |
| 12 | 4-7 | Number of creature types in the map's list | code |
| 12 | 12-15 | Read by game code and multiplied by 2; values 0-10. Probably difficulty or experience scaling. | code (meaning TODO) |
| 14 | 0-3 | Unknown | |
| 14 | 4-7 | Map graphics set: the index used for category 8 (walls/floor/ceiling) and for its numeric attributes (8, set, 11, 0x65/0x66/0x69/0x6B) | code |
| 14 | 8-11 | Door type 0 (category 14 graphics index) | code |
| 14 | 12-15 | Door type 1 | code |

### Map data

For each map, starting at its offset:

1. **Squares**: width × height bytes, **column-major**: square (x, y) is at
   `offset + x*height + y`.
2. **Lists** (one byte per entry) in this order: creature types, wall
   ornaments (category 9 indices), floor ornaments (category 10), door
   ornaments (category 11). The lengths come from the descriptor. The gap
   between consecutive map offsets equals squares plus list lengths for
   all 44 maps (verified).

A square's wall/floor/door ornament index (see thing fields) is a
1-based position in these lists, which the code then maps to the global
graphics index.

### Square byte

| Bits | Meaning |
|------|---------|
| 5-7 | Element type |
| 4 | Square owns a thing list (code) |
| 0-3 | Element-specific flags |

| Element | Name | Flags seen | Notes |
|---------|------|-----------|-------|
| 0 | Wall | none | |
| 1 | Floor | 0, 8 | Bit 1 is tested on floor, teleporter and trickwall squares by creature-movement code (link to an adjacent map?) TODO |
| 2 | Pit | 0, 8 | Bit 3 = open (code: falling only when it is set) |
| 3 | Stairs | 8, 12 | Bit 2 = direction (code: chooses between two transitions). Only 4 squares in the file. |
| 4 | Door | 0, 4, 8, 12 | Bits 0-2 = door state; 4 = closed and 5 = destroyed (code compares against 4 and 5) |
| 5 | Teleporter | 0, 4, 8, 12 | Bit 3 = active (code) |
| 6 | Trick wall | 0, 1 | Bit 2 = open (set/cleared by square actions); bit 0 = illusory, i.e. passable while it looks solid. Either bit makes it passable (0x4AF72). See 05-timeline. (code) |
| 7 | Rock | 0 | Solid filler. Out-of-map lookups also return 0xE0. 4,500 squares. |

Element 7 does not exist in DM1. DM2 uses it as solid space around the
irregular outdoor areas.

### Things

A **thing reference** is a u16 (code, 0x1D2DA):

| Bits | Field |
|------|-------|
| 0-9 | Index into that type's array |
| 10-13 | Thing type |
| 14-15 | Cell / position within the square (quadrant, or the wall side for wall-mounted things) |

Special values: 0xFFFE ends a list; 0xFFFF marks a free slot (both for
object-list entries and for the `next` word of an unused record).

**Linking**: column table entry *c* = index in the object list of the
first thing-bearing square in column *c* (a running total across all
maps). Within a column, the object-list index of square (x, y) is the
column's entry plus the number of thing-bearing squares above y. That
object-list entry is the first thing reference. Every record starts with
a `next` reference word, so a square's things form a singly linked list
ending in 0xFFFE. Creatures (word 2) and containers (word 2) hold a
second list of their possessions or contents.

Record sizes (table at 0x71808) and runtime spares (table at 0x71818):

| Type | Name | Size | Count | Spares |
|------|------|------|-------|--------|
| 0 | Door | 4 | 53 | 0 |
| 1 | Teleporter | 6 | 217 | 0 |
| 2 | Text | 4 | 576 | 0 |
| 3 | Actuator | 8 | 1,020 | 0 |
| 4 | Creature | 16 | 299 | 75 |
| 5 | Weapon | 4 | 173 | 100 |
| 6 | Clothing | 4 | 202 | 60 |
| 7 | Scroll | 4 | 9 | 0 |
| 8 | Potion | 4 | 83 | 12 |
| 9 | Container | 8 | 24 | 5 |
| 10 | Misc item | 4 | 256 | 200 |
| 11-13 | unused | 0 | 0 | 0 |
| 14 | Missile | 8 | 0 | 60 |
| 15 | Cloud | 4 | 0 | 50 |

Missiles and clouds exist only at runtime. In a new game, the counts of
creatures, missiles and clouds (types 4, 14 and 15) set the size of the
active-object pool (0x80426).

#### Field layouts

Word 0 of every record is `next`.

**Door (4)**, word 1:
- bit 9: animation direction (set = opening); bit 10: animation running;
  bit 12: cleared when an action starts a move. These are runtime state
  (code, door handlers 0x568F7 and 0x564C6; see 05-timeline).
- The door's open/closed state is not here but in the door square's bits
  0-2 (0 open, 1-3 part-closed, 4 closed, 5 destroyed).
- bit 0 selects descriptor door type 0 or 1 (code, 0x1FE1C); bit 5 makes
  closing check the creature's size class (0x564C6); bit 7 allows magical
  destruction and bit 8 bashing (0x18D9E); bit 13 is driven by actuator
  0x46 (meaning unknown). 33 of 53 doors have only bit 5 set. Bits 1-4
  (ornament in DM1) still to be confirmed against the door renderer.

**Teleporter (6)**, words 1-2:
- word 1 bits 0-4 = destination x, bits 5-9 = destination y; word 2 bits
  8-15 = destination map (code: edge-link lookup 0x1D074).
- word 2 bits 1-2: when both are set, square actions don't toggle the
  teleporter's active bit (code: 0x58EDB). The active bit itself is bit 3
  of the teleporter square.
- Word 1 bits 10-11 rotation, bit 12 absolute rotation, bits 13-14 scope
  mask (party 2, creatures 1/2, other things always pass; scope 1 =
  creatures only), bit 15 audible (code: 0x4A34A, see 05-timeline).
- Word 2 bit 0: the square counts as rock for the adjacent-layer lookup
  (0x1CC7E).
- A teleporter square also serves as a map-edge link. An actuator of type
  0x27 on it disables the link (see 05-timeline).

**Text (2)**, word 1 (code, text display routine):
- bit 0: visible / active
- bits 1-2: mode. 0 = text packed in the dungeon's own text block;
  1 = a message string in GRAPHICS.DAT (category 3, type 5, index =
  bits 3-15), with a special case when bits 11-15 = 14; other modes
  display nothing here.
- bits 3-15: word offset into the text block (mode 0) or message number.

**Actuator (3)**, words 1-3 (code: square-action handlers 0x58304 and
0x57476, and target firing 0x4BC4C; the full type list is in 05-timeline):
- word 1 bits 0-6: actuator type; bits 7-15: data (item kind, creature
  type, counter, delay, map ...).
- word 2:
  - bit 0: busy/latch;
  - bit 2: enabled/state;
  - bits 3-4: action sent (0 set, 1 clear, 2 toggle, 3 follow);
  - bit 5: inverted;
  - bit 6: sound;
  - bits 7-10: delay in ticks.
- word 3:
  - bits 0-3: ornament (DM1-assumed);
  - bits 4-5: target cell;
  - bits 6-10: target x;
  - bits 11-15: target y.
- The earlier reading of "word 3 bits 4-9 = type" was wrong. Those bits are
  the target cell and x.

**Creature (16)**:
| Offset | Field | Source |
|--------|-------|--------|
| 0 | next | code |
| 2 | First possession (thing list; 0xFFFE = none) | code/data |
| 4 | u8 creature type (looked up through the creature info table, 0x1F8D9) | code |
| 5 | u8 active-creature slot; 0xFF = not active; reset on load | code |
| 6 | Hit points (values such as 7, 600, 1000) | data |
| 8, 10, 12 | Mostly 0; occasionally HP-like values. Possibly HP of further group members, or per-creature data. | data, TODO |
| 14 | Bits 8-10 always 4-7 (0x400 in 282 of 299 records); bit 7 rare | data, TODO |

**Weapon (5), clothing (6), misc (10)**, word 1:
- bits 0-6: item kind; bit 7: always set in the file (meaning TODO);
  bits 10-13 for weapons/clothing and 14-15 for misc vary
  (charges/quantity/flags?). TODO.

**Scroll (7)**, word 1 (code): if bits 10-15 are 0, bits 0-9 are the index
of a text thing holding the scroll's content. Otherwise bits 10-15 are a
GRAPHICS.DAT message number used directly.

**Potion (8)**, word 1: bit 15 always set; the rest varies (DM1: bits 0-7
power, bits 8-14 kind). TODO.

**Container (9)**: word 1 = first content thing; word 2 bits 13-14 =
container kind (DM1 agrees); word 3 = 0 or 0xFFFF. TODO.

### Packed text (code)

Each word holds three 5-bit codes, high to low (bits 10-14, 5-9, 0-4):

| Code | Output |
|------|--------|
| 0-25 | A-Z |
| 26 | space |
| 27 | period |
| 28 | line break (or a separator, depending on the caller) |
| 29 | escape: the next code selects a single character from a 32-entry table at 0x71928 (lowercase letters and digits) |
| 30 | escape: the next code selects a short word from a table at 0x71828 (common words) |
| 31 | end of text |

Only 28 words of dungeon text exist. Nearly all of DM2's text lives in
GRAPHICS.DAT, and text things reach it through mode 1.

## Open questions

- Descriptor word 4 flags; word 12 bits 12-15 (difficulty?); word 14 bits 0-3.
- Square bit 1 on floor, teleporter and trickwall squares.
- Exact door, teleporter, actuator, potion and item bitfields (to be done
  with the doc for each system: 07, 08, 09).
- Whether anything verifies the checksum, and how the compressed (0x8104)
  dungeon format works (needed only for other releases).
- `DUNGENB.DAT` (alternate dungeon, chosen by the flag at 0x803F8): what
  sets the flag.
