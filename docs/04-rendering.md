# Rendering

How SKULL.EXE builds a frame: the screen and viewport bitmaps, the layout
system that positions every drawn element, the 3D viewport traversal, the
draw-request pipeline (offset, scale, mirror, light, colour key), the
blitters, fonts and palette output.

Tools: `tools/layout.py` (layout table and placement resolver),
`tools/viewport.py` (test renderer for walls, floor and ceiling; writes to
`re/vp/`). Both are verification aids for the Rust port, not the engine.

Status key: **verified** = reproduced with the tools against the real data;
**traced** = read from the code, not yet reproduced; **inferred** = best
reading, needs confirmation.

## 1. Bitmaps and screen

- Every bitmap the renderer touches carries a 6-byte header *before* its
  pixel pointer: bits per pixel (byte at −6, 4 or 8), width (word at −4),
  height (word at −2). Decoded images, the viewport and the screen all use it.
- **Screen back buffer**: 320×200, 8-bit, at the pointer in 0x71550. This is
  the default destination of a draw request.
- **Viewport bitmap**: 224×136, 8-bit (30,464 bytes; size stored at 0x71704
  to 0x71708), pointer in 0x7F240. The 3D view is rendered into it and then
  copied into the screen.
- Bitmaps can also be 4-bit (two pixels per byte, rows rounded to an even
  pixel count); the blitter has 4-bit destination paths (below), used for
  off-screen work such as icons.

### Getting pixels to the display (traced)

SKULL.EXE never touches VGA registers or calls BIOS video. All display and
input I/O goes through **`int 0xFC`**, a service installed by the launcher
`IBMIOP.EXE` (`DM2.BAT` runs `IBMIOP SKULL.EXE +VS`). EAX holds a function
number; parameters are passed in a shared block whose pointer is at
0x8048C. The only direct port access is a wait on the VGA status port
0x3DA (vertical retrace) at 0x13888.

| Fn | Wrapper | Use |
|----|---------|-----|
| 0x0A | 0x13898 | **Set palette**: 1,024 bytes copied to the shared block, i.e. 256 records of (index, R, G, B) with 8-bit components, the same layout as GRAPHICS.DAT (1,0,9,254). The driver does the 8-bit → 6-bit DAC conversion. |
| 0x09 | 0x138D9 | **Present a rectangle** of the screen buffer (8-byte rect in the shared block, flag word 8 or 0x8008). Bracketed by 0x14C55 / 0x14C68, which hide and show the mouse cursor when the rect overlaps it. |
| 0x05 | 0x139E4 | Present variant used after drawing the 1-line text strip from 0x80410 |
| 0x03 | 0x13A87 | Rect operation (8-byte rect) |
| 0x06, 0x0E, 0x0C, 0x0B, 0x07, 0x08, 0x04, 0x0D | 0x138C7, 0x14400-0x14CC8 | Cursor/mouse and input services (not traced; see 10-ui-input) |
| 0x12-0x14, 0x28, 0x01, 0x2E-0x30, 0x29-0x2B, 0x0F | various | Timer, keyboard, sound and startup services (not traced) |

`IBMIOP.EXE` itself has not been disassembled. For the remake the contract
is enough: a 320×200 indexed framebuffer, a 256-entry palette of 8-bit
RGB, and rectangle presents.

## 2. Layout system (verified)

Almost every on-screen position is a **layout id** looked up in the table
stored in GRAPHICS.DAT entry (1,0,4,0). Loader 0x19F3D → parser 0x19D0E;
lookup 0x190E9; placement 0x1936F.

### Table format

| Offset | Content |
|--------|---------|
| 0 | u16 magic 0xFC0D |
| 2 | u16 number of id ranges R |
| 4 | R × (i16 first id, i16 last id) |
| 4 + 4R | for each id of each range in order: 4 × i16 (kind, parent, x, y) |

30 ranges, 2,183 records here. In memory the game repacks each range into
a compact block (constant kind or parent, byte-sized fields where values
allow; flag bits 1, 2, 4, 8, 16 in 0x19D0E/0x19091); this is only a memory
optimisation and changes nothing.

### Record kinds

| Kind | Meaning |
|------|---------|
| 0-8 | **Anchored point** (x, y) in the parent's frame. The kind is the anchor: which point of the placed box sits on (x, y). |
| 1 | Also the "top-left" anchor; when used as a *parent* it acts as a pure translation by (x, y). |
| 9 | **Rectangle**: x and y are its width and height. It has no position of its own; it clips whatever is placed through it. |
| 10-18 | **Attached point**: anchor point (kind − 10) of the parent's placed rectangle, plus (x, y). The object itself is then anchored with kind − 10. |

Anchors (kind or kind − 10): 1 top-left, 5 top-centre, 2 top-right,
8 middle-left, 0 centre, 6 middle-right, 4 bottom-left, 7 bottom-centre,
3 bottom-right. "Centre" offsets use (size + 1) / 2 and "far edge" offsets
use size − 1.

### Placement algorithm (0x1936F)

Inputs: the id, the object size (w, h), an optional anchor override, and
the source bitmap (its size is the default when w or h is 0). If bit 15 of
the id is set, w and h are instead an extra (dx, dy) added to the
reference point, and the size comes from the bitmap.

1. Reference point = the record's (x, y) for kinds 0-8 (0 for 10-18);
   clip = a huge rectangle.
2. Walk up the parent chain until a record with parent 0:
   - Parent is kind 1: translate the point and the clip by its (x, y).
   - Parent is a rectangle R (w × h) and the current record is an anchored
     point: R is placed with the *current record's* anchor at the current
     record's (x, y); the clip is intersected with it. The point itself does
     not move: the record and its clip box share a frame.
   - Current record is kind 10-18: its parent P places rectangle R (P's
     parent) with P's anchor at P's (x, y); the point becomes R's origin +
     anchor point + own (x, y); clip ∩= R; continue from R.
   - Parent is another anchored point: remember that the next rectangle's
     origin must be added to the point (a nested frame).
3. Top-left = reference point minus the anchor offset for the object size.
4. Clip to the accumulated rectangle; return the visible box and how many
   source columns/rows were cut off at the left/top. If the chain passed
   through the viewport rectangle (id 3) during a mid-step frame, a further
   clip (0x7170E) applies (section 5).

`tools/layout.py show ID [W H]` prints the chain and the placement; the
Python `resolve()` is a direct transcription of these rules.

### Notable ids

| Id(s) | What |
|-------|------|
| 1 | Screen rectangle 320×200; 2 is its frame |
| 3 | Viewport rectangle 224×136 (frame of the viewport bitmap); 4 is its frame, 7 is a frame 40 rows down |
| 10 | Right-hand panel, 92×87 at (228, 40) |
| 19 | Message line, 114×14 at (103, 145) |
| 130-133 | Missile/effect size boxes (44×38, 64×61, 96×88, 170×154) |
| 700, 701 | Viewport ceiling and floor |
| 702 + c | Wall face of viewport cell c |
| 3100 + 25c + s | Item positions: cell c, sub-square s (0-24) |
| 4425-4775 | Side-wall ornament positions for side cells (table 0x7179C maps cell → base id) |
| 5000 + 25c + s | Creature positions: cell c, sub-square s |

## 3. Viewport traversal (verified for walls, floor, ceiling)

Entry point **0x54B3F** `(facing, x, y)` draws one view into the viewport
bitmap.

### View cone

23 cells, each a (lateral, forward) offset; lateral is positive to the
party's right. Map coordinates are `pos + forward·step[facing] +
lateral·step[(facing+1)&3]`, steps N (0,−1), E (1,0), S (0,1), W (−1,0)
(0x1C9E9, tables 0x717F8/0x71800).

| Cell | Lat, fwd | Depth | Faces drawn |
|------|----------|-------|-------------|
| 0 | 0, 0 | 0 | — (party square) |
| 1, 2 | ∓1, 0 | 0 | side |
| 3 | 0, 1 | 1 | front |
| 4, 5 | ∓1, 1 | 1 | front + side |
| 6 | 0, 2 | 2 | front |
| 7, 8 | ∓1, 2 | 2 | front + side |
| 9, 10 | ∓2, 2 | 2 | side |
| 11 | 0, 3 | 3 | front |
| 12, 13 | ∓1, 3 | 3 | front + side |
| 14, 15 | ∓2, 3 | 3 | front + side |
| 16 | 0, 4 | 4 | front |
| 17-20 | ∓1, ∓2, 4 | 4 | front |
| 21, 22 | ∓3, 4 | 4 | — |

Tables: offsets 0x75ACC (byte pairs), depth 0x75B11, side 0x75AFA, faces
0x7600E. The left/right partner of each cell (used for mirroring) is
0x75B28: 0,2,1,3,5,4,6,8,7,10,9,11,13,12,15,14.

### Frame sequence

1. Allocate per-frame tables: 23 cell summaries (18 bytes each, 0x80040),
   two 17×21 occlusion grids (0x80044, 0x8004C), two 23-entry bit masks
   (0x80034, 0x80048).
2. Build the summaries for cells 22 down to 0 (0x52BF6, see below).
3. Pick ceiling/floor colour adjustments from map-set attributes 'p'/'q'
   (0x70/0x71) depending on whether the nearest rows are open (0x19AC4).
4. Draw the **ceiling**: image (8, set, 1, 1) at layout 700; then the
   **floor**: image (8, set, 1, 0) at layout 701 (0x4E32A). If the map
   set's images do not cover the 136 rows, the gap between them is filled
   with a solid colour first (0x4E294, gap size in 0x802D0).
5. Compute the wall **parity** (0x54874, mode 0): `(map layer + map x
   origin + map y origin + x + y + facing) & 1`, using the current map
   descriptor bytes 6, 7 and word 8 bits 0-5. Stored in 0x802CC.
6. Cells in this order (0x76025), back to front, outer columns first:
   `19 20 17 18 16 14 15 12 13 11 9 10 7 8 6 4 5 3 1 2` (0x53E9E).
7. Party square and foreground (0x54117), then missiles and effects.

Ceiling/floor mirroring uses the per-set flag word (8, set, 11, 0x65)
(0x802D2): floor bit 0x08 → mirror when parity (or by time when 0x10 is
also set); otherwise bit 0x40 → follow a global toggle (0x7F252 bit 0).
Ceiling bit 0x02 → mirror when *not* parity (by time with 0x04); otherwise
bit 0x20 → the global toggle.

### Cell summary (0x1E908)

Each cell gets 8 words: view type, raw square byte, first drawable thing,
four ornament slots (one per face) and a spare. View types:

| View type | From element | Notes |
|-----------|--------------|-------|
| 0 | wall; closed trick wall | faces get wall ornaments |
| 1 | floor; closed pit; invisible teleporter; open trick wall | floor ornament from set attribute 0x6B (animated if bit 15) or from actuator/text things |
| 2 | open pit | |
| 5 | visible teleporter | adds an effect overlay (0x509E6) |
| 0x10 / 0x11 | door, facing / side-on | door state in slot 3, door thing in slot 4 |
| 0x12 / 0x13 | stairs, two orientations | relative to facing parity; up/down in slot 3 |
| 7 | solid rock | never drawn |

### Walls (0x53C7B) — verified

- Image (8, set, 1, sub) at layout 702 + cell; scale 1:1; colour key =
  the set's default key, attribute (8, set, 11, 100).
- Cells 1-15: normally sub = 34 + cell. Cells with lateral > 0 are always
  drawn **mirrored horizontally**.
- When parity is 1, the game first looks for alternate art at sub
  176 + partner(cell); if absent (always, in this archive) it uses
  34 + partner(cell), i.e. the opposite side's image, and mirrors the centre
  cells too. Result: the two art variants swap sides on every step.
- Cells 16-20 all use sub 50; mirrored when lateral is +1, then XOR parity
  (lateral ±2 never mirror before the XOR).
- Light: depth 0 is passed for walls (their art is pre-shaded per
  position), except in mid-step frames.

### Other cell content (traced)

- **Wall ornaments** (0x4F3DF): category 9 images; position id from
  0x19B91 (`cell·25 + 3100 + slot`, or the side-wall base table 0x7179C);
  colour key from attribute (9, orn, 11, 4); per-ornament anchor slot from
  attribute 5; size scaled by the depth table below, with an aspect
  override from attributes 0x14 / 0x15 for depths 2 and 3; wall writing
  (text things) is drawn with the 8×8 wall font, centred on the face.
- **Creatures** (0x51203 → 0x50DEE): the view of the creature is
  `(party facing − creature facing) & 3` (front if the type has the
  "always front" flag). The animation sequence picks an image sub per view;
  missing views fall back to the opposite view mirrored, then the front,
  then subs 250-253 / 252. Position: layout 5000 + 25·cell + slot, where the
  5×5 sub-square slot is rotated by facing (0x19B0B) and nudged by small
  offsets (table 0x75BC2: 0, 1, 2, 3, 0, −3, −2, −1); side views shift ±7
  scaled pixels. Scale = depth scale × the per-frame byte from the
  creature's table (15, type, 7, 0xFE), divided by 64.
- **Items** (0x518B0) at 3100 + 25·cell + slot; missiles (0x522A7).
- Things inside a cell are drawn in a fixed sub-square order that depends
  on whether the cell is left of, right of or on the centre line (tables
  0x75D84 / 0x75D9D / 0x75DB6, 25 entries each), with a 17-column
  occlusion grid deciding which cell owns each sub-square (cell origins
  0x75DDF, sub-square offsets 0x75DFF).

### Depth scale and shading

| Depth | 0 | 1 | 2 | 3 | 4 |
|-------|---|---|---|---|---|
| Scale (/64), 0x75B6D | 96 | 64 | 43 | 28 | 19 |
| Darkening (/64), 0x75C0C | 0 | 0 | 12 | 28 | 46 |
| Mid-step darkening, 0x75C07 | 0 | 0 | 5 | 19 | 36 |

Side wall ornaments seen obliquely use x-scale 114 (depth ≥2 side) or 76.

### Mid-step frames (inferred, strong)

0x7F258 is a step-animation counter set by the movement code (0x235BF)
when the party walks; normal frames clear it. While non-zero the viewport
is drawn for the in-between position: floor and ceiling are shifted by
0x7170A / 0x7170C, walls are lit with negative depth (selecting the
"mid-step" darkening row and remap tables 10-13 instead of 1-4), creatures
on the party square are skipped and placements through the viewport
rectangle get an extra clip (0x7170E).

## 4. Draw requests (traced)

All viewport and most interface drawing goes through a 0x13A-byte request
struct:

| Offset | Field |
|--------|-------|
| +6 | Archive entry number of the image |
| +8..+11 | Category, index, 1, sub |
| +0x0E, +0x10 | Source x/y skip |
| +0x12, +0x14 | Output width/height (after scaling) |
| +0x18 | Layout id (−1 = explicit position in +0x20/+0x22) |
| +0x1A | Anchor override (−1 = none) |
| +0x1C, +0x1E | Drawing offset x/y |
| +0x2C | Destination bitmap (default: the screen) |
| +0x30 | Colour key (−1 none; −2 compute placement only; −3 skip) |
| +0x32 | Flip flags: bit 0 horizontal, bit 1 vertical |
| +0x34, +0x36 | X/Y scale in 64ths (64 = 1:1) |
| +0x38 | Colour-map length (16 for 4-bit images; 256 for a full remap) |
| +0x3A | Colour map |

Steps:

1. **Init** (0x1B3E8): look up the image; set scale 64/64; add the drawing
   offset: the per-category base (cat, idx, 12, 0xFE) plus the image's own
   offset (0x3EED3):
   - width tag 32 (bit 15 of the width word): offset from attribute
     (cat, idx, 12, sub), high byte x, low byte y, both signed;
   - compressed 8-bit image: signed bytes 4 (x) and 5 (y) of the entry;
   - 4-bit image: the width tag and height tag themselves, as signed 6-bit
     numbers (x, y).
   Verified: of 1,650 image records with the width tag 32, 1,592 have the
   type-12 attribute; none of the 4,026 others do. Copy the image's 16-byte
   colour map into +0x3A.
2. **Light** (0x4E3D5): build the colour map for the cell depth (section 6).
3. **Prepare** (0x1B54A): if scaled, compute the new size with
   `scaled = (v·s + s/2) >> 6` (0x1ACC5) for size and offset; mirror the
   x offset when flipping horizontally; produce a scaled copy (0x1424B for
   8-bit, 0x1410D for 4-bit sources) in a cache keyed by image and scale.
4. **Draw** (0x1B8E5): resolve the layout id (with bit 15 and the offset
   when the offset is non-zero), adjust the source skip for flips, then blit.

## 5. Blitter (traced)

0x12AD2 `(src, dst, dst_rect, src_x, src_y, src_width, dst_width, key,
flip, dst_bpp, src_bpp, colour_map)` dispatches on bit depths:

| Destination | Source | Routine |
|-------------|--------|---------|
| 4-bit | 4-bit | 0x11C88 |
| 4-bit | 8-bit | 0x122C8 |
| 8-bit | any, no colour map | 0x125C4 |
| 8-bit | any, with colour map | 0x128C4 |

Row copies: forward (0x124DB, keyed 0x12504) and reversed for horizontal
flip (0x1253A, keyed 0x12570); with colour map 0x127C0/0x127F1 and
0x1282E/0x1286A. Vertical flip walks source rows from the bottom. The
**colour key is compared with the raw source value** (palette index for
8-bit, nibble for 4-bit) before the colour map is applied; matching pixels
are skipped. No blending exists.

Scaling (0x1424B / 0x1410D) is nearest-neighbour; the exact sampling
rule is still to be transcribed.

## 6. Palette, light and colour remapping

- **Palette**: (1,0,9,254), 256 × (index, R, G, B), uploaded unchanged
  through driver function 0x0A. There is no evidence of palette animation
  in the renderer; brightness changes are done by remapping indices.
- **Colour ramps** (1,0,7,2), verified: byte 0 = ramp count (16), then the
  16 ramp lengths (all 16), then for each ramp its 16 brightness values
  (0-63, ascending), then for each ramp its 16 palette indices, then a
  256 × (ramp, position) map from palette index back to its ramp. The data
  checks out exactly (1 + 16 + 2·256 + 512 = 1,041 bytes; the back map
  agrees with the forward lists). Built at 0x1AE54.
- **Darkening** (0x1AFC2) to light level L (0-64): for each palette entry,
  scale its ramp brightness by L/64 and pick the nearest brightness in the
  same ramp; if the result equals one of the colour keys, step to the
  nearest neighbour that isn't; keys map to themselves. Results are cached.
- **Per-depth light** (0x4E3D5): darkening for a depth is
  `64 − (64 − d)·(64 − ambient)/64`, with d from the depth table and the
  ambient light level from 0x802CC's high word. If the map set provides a
  256-byte remap (8, set, 7, depth) — or (8, set, 7, depth + 9) in mid-step
  frames — that table is applied first and the ambient darkening on top of
  it (0x1B2BE). The set's remap tables are therefore authored depth fog.
- 4-bit images go through the same process on their 16-entry colour map.

## 7. Text and fonts

- **Interface font**: GRAPHICS.DAT (1,0,7,0), 768 bytes = 6 rows × 128
  characters, ASCII-indexed; each byte holds a 5-pixel row in bits 4..0
  (bit 4 leftmost). Rendered (0x10D9F → 0x10E3F) as 6×6 cells: the sixth
  column is always background. Characters are blitted as 4-bit glyphs with
  a two-entry colour map (foreground, background); flag 0x4000 on the colour
  makes the background transparent. Advance 6 px, line height 6 (0x7172E,
  0x71736). **Note:** docs/02 describes (1,0,7,0) as a palette; it is this font.
- **Wall writing font**: image (8, set, 3), a strip of 8×8 glyphs
  (metrics 0x7173A/0x7173C). Glyph index: A-Z → 0-25, '.' → 27, anything
  else → 26 (blank). Lines are centred on the wall face; the text colour map
  comes from (8, set, 3) colour data and the cell's light.

## 8. Open questions

- Exact nearest-neighbour sampling of the scalers (0x1424B, 0x1410D).
- Door, door ornament, stairs, pit and teleporter drawing (0x539CB,
  0x53BFB, 0x53B1B, 0x508FF, 0x509E6) and items (0x518B0) in detail.
- Missiles and spell effects (0x522A7, ids 130-141).
- Creature animation sequences (types 7/8, sub 251-254) and colour variants.
- The 'p'/'q' ceiling/floor colour attributes (0x19AC4).
- The party-square/foreground pass (0x54117) and how the viewport bitmap is
  copied to the screen (rect 3 placement; likely layout id 7 or a fixed
  (0, 40) origin — to confirm).
- The driver side of `int 0xFC` (IBMIOP.EXE), only needed for exactness of
  cursor handling.
