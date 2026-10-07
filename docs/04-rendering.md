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

### Presenting the viewport (traced)

The view is drawn straight into the 320×200 back buffer. Afterwards
0x138D9 resolves layout record 7, which is rectangle 3 (224×136)
translated to **(0, 40)**, and asks the display driver (`int 0xFC`
function 8) to copy that rectangle to the screen, hiding the mouse
cursor first if it overlaps.

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
| 3100 + 25c + s | Wall-ornament anchor positions on the front face of cell c (0x19B91) |
| 4425-4775 | Side-wall ornament positions for side cells (table 0x7179C maps cell → base id) |
| 5000 + 25c + s | Floor positions for items, creatures and missiles: cell c, sub-square s (0x19B77) |

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
| 0x10 / 0x11 | door seen edge-on / door across the view (panel visible) | door state in slot 3, door thing in slot 4. 0x10 when the square's orientation bit 3 equals facing & 1 |
| 0x12 / 0x13 | stairs seen side-on / stairs seen front-on | 0x12 when orientation bit 3 equals facing & 1; square bit 2 (direction) in slot 3 |
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

### Summary record layout

Each cell's summary is 18 bytes at 0x80040 + 18·cell: bytes 0-1 are the
map x/y of the cell, then the 8 words described above, so the view type
is at +2, the first drawable thing at +6, slot 3 (door state, pit or
stair bit) at +8 and slot 4 (door thing) at +10.

### Cell dispatcher (0x53E9E)

For each cell in draw order:

| View type | Cells 0-15 | Cells 16-20 |
|-----------|------------|-------------|
| 0 wall | creatures standing in the cell's list (0x51203), then the wall faces (0x53E41) | same |
| 1 floor | floor ornament (0x50081), ceiling hole (0x507AB), then cell contents (0x52518, mask 0x1FFFFFF, or 0x1FFFC00 when a floor ornament was drawn) | floor ornament, then creatures |
| 2 open pit | pit (0x508FF), then as floor | floor ornament, then creatures |
| 5 teleporter | as floor, then the teleporter field (0x509E6) | floor ornament, then creatures |
| 0x10 door edge-on | in cell 3 only: the door lintel, image (8, set, 1, 6) at the placement of 5000 + 25·3 + 2, unless the door type's attribute (14, type, 11, 0x40) is set; then as floor | — |
| 0x11 door across | 0x539CB: far half of the cell, door panel, near half | creatures only |
| 0x12 stairs side-on | 0x53BFB, then cell contents | — |
| 0x13 stairs front-on | 0x53B1B, then cell contents | — |

The party square is drawn last by 0x54117 (see below).

### Cell contents (0x52518)

A cell's things are drawn per 5×5 sub-square, in the order given by the
left / right / centre tables (0x75D84 / 0x75D9D / 0x75DB6): the left
columns go left-to-right, right columns right-to-left, the centre column
outside-in, all back row first. The party cell walks only the first 15
entries (the rows in front of the party). At each sub-square, in this
order:

1. **Floor items** (0x522A7) if the cell's item mask has that bit.
2. **Creatures** held at that sub-square's grid point (0x51203, below).
   After a creature has been drawn, missiles in the seven preceding
   sub-squares are redrawn on top of it.
3. **Missiles** (0x518B0) if the missile mask has that bit.

**Creature grid (0x52BF6, 0x52518).** Two 21×17 byte grids (lateral ×
depth, allocated at 0x80044 and 0x8004C each frame) cover the sub-squares
of cells 0-15. Sub-square *s* of cell *c* is grid point (8 + 4·side +
s mod 5, 4 + 4·forward − s div 5) (tables 0x75DDF / 0x75DFF), so
neighbouring cells share their edge rows and columns.
- *Owner grid*: each cell claims 5 lateral × 4 depth points starting at
  (8 + 4·side, 4·forward) (0x75EAF); cells processed later overwrite shared
  columns.
- *Holder grid*: each creature group is entered at its own grid point (from
  its in-square position, or the lunge slot); if that point is taken it
  moves one row nearer the party until a free row is found.
- During a cell's contents pass, a sub-square draws the creatures held at
  its grid point when the cell owns that point (a wall cell takes any
  point); the entry is then cleared so the group is drawn once.
- In mid-step frames the party's own cell draws no creatures.

Cells 9 and 10 (the far side cells at depth 2) have no contents pass
(table 0x75DCF).

### Item placement (0x522A7 → 0x51EB7)

- Only things of types 5-10 (weapons through misc items) are drawn here.
- An item's quadrant (reference bits 14-15) minus the party facing,
  modulo 4, picks a 5×5 slot: 0 → 6, 1 → 8, 2 → 18, 3 → 16 (table
  0x7168A, inverse 0x159C3; slot 12 is the centre, used by missiles and
  creatures).
- Placement id = 5000 + 25·cell + slot (0x19B77), drawn with bit 15 set
  so that the offsets below move the reference point.
- Items in the party's own cell are skipped when they are in the back two
  rows (behind the camera).
- **Scale** comes from a perspective table indexed by depth·4 + (4 − row),
  row = slot / 5 (0x75B73): 87, 78, 71, 64, 58, 52, 47, 43, 39, 35, 31,
  28, 26, 23, 21, 19, 17, 15. Consecutive depths overlap, so the back
  row of one square matches the front row of the next.
- **Image**: (category, index, 1, sub). Sub is 0 normally, or 1 for an
  item in the centre column's middle slot when that image exists. Open
  chests use subs 4 and 5 instead of 0 and 1 (table 0x75D6A).
- **Stacking**: a counter per quadrant, starting at 0 and wrapping at 16
  (0x522A7), indexes a table of (x, y) nudge pairs (0x75B94) into the
  offsets 0, 1, 2, 3, 0, −3, −2, −1 (0x75BC2), so piled items fan out
  slightly. The first item takes the first pair too. The offsets are added
  to the request's screen position after scaling (0x4E502); the y nudge
  is skipped for alcove items (0x51EB7's fifth argument).
- The per-category offset attribute (cat, 0xFE, 12, sub) is added.
- Colour key: attribute (cat, index, 11, 4) when present, else 10. The
  attribute can be 0x8000; the request's key field is 16 bits, so that
  value never matches a pixel and the item is drawn fully opaque
  (verified: treating it as key 0 punches holes in container images).

### Missiles and spell effects (0x518B0)

Thing type 14, drawn at its quadrant's sub-square. The image comes from the
carried thing: an item's own category, or category 13 for spell clouds and
explosions (carried value ≥ 0xFF80). In the party's own cell only the front
quadrants (0 and 1) are drawn.

- **Flight kind** (0x151E6), from which images exist: no sub 8 → the
  carried item is drawn as an item, 92 pixels up; sub 8 but no 12 → kind 3
  (one image for every direction); with 12: sub 10 present → kind 1, else
  sub 9 present → kind 0, else kind 2.
- **Scale**: 64 in the party cell, except for spells; otherwise table
  0x75B8D (64, 52, 43, 35, 28, 23, 19) at index depth·2 − quadrant/2
  (nothing is drawn if negative). Spells scale that by their power p (p/2 +
  128 for 0xFF82): factor `((p·128/255) + 1)/2` through the 64ths scaler,
  at least 8.
- **Direction** comes from the missile's timeline event (bits 10-11 of its
  word at +8; the missile record's word 3 is the event index).
- **Flying along the line of sight** (direction parity equals the party's):
  - kind 0: on squares with even x + y, sub 9 for quadrants 0-1 and 8
    otherwise; on odd squares, a vertical flip, and sub 9 for quadrants
    2-3, 8 otherwise;
  - kinds 1 and 2: sub 8 for kind 2, or kind 1 flying away from the party;
    sub 10 otherwise;
  - then: mirrored on left-hand cells, and on centre cells for quadrants 0
    and 3; spells in odd quadrants also flip vertically.
- **Flying across the view**: sub 12. Kind 0 mirrors in quadrants 0 and 3,
  then adds a vertical flip on odd squares or toggles the mirror on even
  ones; other kinds mirror when flying to the party's right.
- **Flip mask**: attribute (13, index, 11, 1) for spells, 3 for items.
- **Position**: placement 5000 + 25·cell + slot, 92 pixels up before
  scaling, so missiles fly at chest height.

### Creatures (0x51203 → 0x50DEE)

- **Drawing descriptor**: 8 bytes, kept in the active creature slot (+8;
  static creatures use their record) and fetched from (15, type, 7, 0xFD)
  by sequence base + frame (0x14CF2). Bytes 0-3 are image subs per view,
  byte 4 the sub-square, byte 5 the scale index, byte 6 a signed shift,
  byte 7 two flag bits per view (view v at bit (3 − v)·2).
- **Alternate descriptor**: when the slot's state byte (+0x1A) is 0x13, a
  second descriptor (from slot words +0xE / +0x10) supplies the
  sub-square, scale index and shift, while the images still come from the
  first. Earlier notes took this for a colour-variant remap; 0x14CF2 only
  fetches descriptors.
- **View**: `(party facing − creature facing) & 3`, or 2 when bit 2
  (0x0004) of the type's info word 0 is set ("always faces the party").
- **Fallbacks**: if the sub for the view is missing, use the opposite
  view's sub (mirrored if that view is odd); if that is missing, the front
  (index 2). If still missing, sub 250 + view (as 0xFA..0xFD counted down:
  view − 6 as a byte), trying the opposite odd view mirrored, and finally
  252.
- **Mirroring**: flag bit 0 of the view always mirrors; bit 1 mirrors only
  when bit 6 of the slot's position byte (+7) is set.
- **Scale**: depth scale (0x75B6D) multiplied by the per-frame byte at
  (15, type, 7, 0xFE)[scale index·4 + view] / 64.
- **Position**: placement 5000 + 25·cell + sub-square, rotated by the
  view. Offsets, added before scaling:
  - the position byte's nudges: bits 0-2 for x and 3-5 for y, through the
    0, 1, 2, 3, 0, −3, −2, −1 table (0x75BC2);
  - the shift byte *k*: for views 0 and 2, x moves by `(k/2 ± 64·k) >> 6`
    (minus normally, plus when a fallback mirrored the image); for side
    views, y moves by `(k/2 ∓ 7·k) >> 6` (−7 for view 1, +7 for view 3).
- **Colour key**: attribute (15, type, 11, 4), default 4 (0x1F9DF).
- **Pointer lean** (formerly read as an "attack lunge"): when flag
  0x802CA is set, a creature group in cell 3 is drawn with a fixed
  sub-square and scale picked by word 0x72246 from tables 0x75BB4 /
  0x75BBB: (2, 52), (14, 64), (22, 78), (22, 78), (22, 78), (10, 64),
  (12, 64), with view 0. Neither value comes from the creature's attack.
  The frame setup (around 0x54034) sets 0x802CA when 0x7F3A4 is non-zero
  or 0x72FFC is not −1, then 0x50BF4 derives 0x72246 from the mouse
  pointer's position inside the viewport: six regions by the pointer's
  offset from the centre (|dx| < 20, |dy| < 15 thresholds), each mapped to
  a neighbouring square; the region is kept only if that square holds a
  creature group the push/swap test (0x24171) accepts, otherwise 6 (none).
  So it is interface hover state, and the remake leaves it to the frontend
  (`CreatureDraw::lunge`, currently unset).
- Cells 16-22 have no contents pass, so creatures four squares away are
  not drawn.

### Doors (0x539CB → 0x5346E)

Door square bits 0-2 are the state (0 open, 1-3 partly closed, 4
closed, 5 destroyed); bit 3 is the orientation. The door thing's word 1
holds:

| Bits | Meaning |
|------|---------|
| 0 | Which of the map's two door types (descriptor word +14 bits 8-11 or 12-15) |
| 1-4 | Door ornament, 1-based into the map's door-ornament list (0 = none) |
| 5 | Panel slides vertically as one piece (clear: two halves) |

Drawing, for cells 0, 3-8 and 11-15 (table 0x75F48):

1. Things in the far half of the cell, then the panel, then the near half.
2. **Panel image**: (14, door type, 1, depth − 1) at 1:1 if that image
   exists; otherwise (14, type, 1, 0) scaled by the depth scale (113/64
   at depth 0). Colour key from attribute (14, type, 11, 4), default 10.
3. **Ornament** (if any): composed onto a copy of the panel first.
   Image (11, ornament, 1, depth − 1 or 0) with colour key (11, orn, 11,
   4) (default 9), at placement 2000 + 4·attr(11, orn, 11, 8) + (3, 2,
   1, 0 for depth 0-3).
4. **Destroyed** (state 5): a damage overlay (14, type, 1, 0x41) is
   composed onto the panel, placed through attribute (14, type, 11, 10).
5. **Placement**: per-cell base ids (table 0x75F28): cell 0 → 3810, 3 →
   3790, 4 → 3780, 5 → 3800, 6 → 3760, 7 → 3750, 8 → 3770, 11 → 3730,
   12 → 3720, 13 → 3740, 14 → 3700, 15 → 3710.
   - Closed or destroyed: base.
   - Partly open, vertical panel: base + state.
   - Partly open, split door: the panel is drawn twice at half width,
     once at base + state + 6 (right half, shifted by the half width) and
     once at base + state + 3.
   - Open: nothing.

### Pits, ceiling holes and stairs

- **Pit** (0x508FF, view type 2): image (8, set, 1, sub) at a per-cell
  placement. Subs from table 0x75CAC (or 0x75C9C when square bit 2 is
  set; no square in this dungeon has it): 107, 108, 108, 110, 111, 111,
  113, 114, 114, –, –, 118, 119, 119, 121, 121. Placements (0x75C6C):
  862, 861, 863, 859, 858, 860, 856, 855, 857, –, –, 853, 852, 854,
  850, 851. Right-hand cells are mirrored (0x75C8C); the party cell
  mirrors by the floor parity. Cells 11 and up are drawn only when slot 3
  is 0.
- **Ceiling hole** (0x507AB, cells 0-8, only when the set's flag word
  bit 0 is set): when the square directly above, on the map one layer up,
  is an open pit. Subs 153, 154, 154, 156, 157, 157, 159, 160, 160 at
  placements 871, 870, 872, 868, 867, 869, 865, 864, 866; right cells
  mirrored.
- **Stairs front-on** (0x53B1B, view type 0x13): index cell·2 + square
  bit 2. Subs (table 0x75F58) 79/59, 80/60, 81/61 for cells 3-5, 82/62,
  83/63, 84/64 for cells 6-8, 85/65, 86/66, 87/67 for cells 11-13 and
  88/68, 89/69 for 14-15. If the set lacks that image, the partner image
  from 0x75F78 is drawn mirrored instead. Placements from 0x75F98 (800
  to 823).
- **Stairs side-on** (0x53BFB, view type 0x12, cells 1, 2, 4, 5, 7, 8):
  subs 205/199, 206/200, 207/201, 208/202, 209/203, 210/204 at
  placements 832, 833, 830/828, 831/829, 826, 827.
- **Party square** (0x54117): stairs underfoot draw two images: subs
  0x4D and 0x4E at placements 0x338 and 0x339 (going down), or 0x39 and
  0x3A at 0x32B and 0x32C (going up). A door edge-on underfoot draws the
  lintel (sub 6). A pit underfoot uses the cell 0 pit image. Then the
  ceiling hole, the floor ornament, items, the teleporter field, and
  finally creatures and missiles in the party's own square (0x5142D).

### Teleporter field (0x509E6)

Category 24, index 0, holds the effect: sub 20 is a noise texture and
subs 0-5 are shape masks by depth. A per-cell 4-byte record (0x75CDC)
gives a phase byte, the mask sub (bits 0-6; 0x7F means none) with bit 7
meaning mirrored, and the effect width and height (224×136 for the party
cell down to 36×49 far away). The field is placed at 702 + cell
(table 0x75CBC). Each frame the noise texture is copied into the mask
shape at a random horizontal offset (one random byte, 0x1C6A1) and a row
offset of `(random bit + phase) · 16` (0x1C6DC), so it shimmers. A
mirrored mask of odd width is copied one pixel narrower. Both random
numbers come from the game's own generator, so in the original the RNG
sequence depends on whether a teleporter is in view; the remake uses a
separate visual generator to keep the simulation deterministic. Sound (24, 0, 2,
137) is the teleport sound.

### Floor and wall ornaments

- **Wall ornaments** (0x4F3DF): category 9 images; position id from
  0x19B91 (`cell·25 + 3100 + slot`, or the side-wall base table 0x7179C);
  colour key from attribute (9, orn, 11, 4); size scaled by the depth table
  below, with an aspect override from attributes 0x14 / 0x15 for depths 2
  and 3.
- **Attribute 5** packs the placement: low byte = grid slot + 1 (default
  slot 12, the centre of the 5×5 face grid), high byte = anchor kind. The
  anchor is not the layout record's own kind: 0x4F3DF passes it to the
  drawer 0x4E502, which stores it in the draw request, and 0x1B8E5 hands it
  to the resolver 0x1936F as the override argument (only 0xFFFF keeps the
  record's kind). So an ornament with no high byte is **centred** (kind 0)
  on the grid point, not bottom-anchored as the side-face records (kind 7)
  would place it. Confirmed against the original: the start view's side
  ornament moved ~20 px down and now matches.
- **Far cells 16-20 show front-face ornaments** too: the dispatcher runs
  the wall faces (0x53E41) for every wall cell, and the face table gives
  cells 16-20 a front face. Their ids are 3100 + 25·cell + slot like the
  nearer front faces, scaled at depth 4 (19/64). Confirmed against the
  original while walking north on map 0: a gate ornament four squares
  ahead appears in both.
- **Open: depth-4 side cells (17, 18).** Against the original, their wall
  images reach 2-3 px further towards the centre (x 90-92 at the start
  view, which the original leaves black), with or without mirroring.
  Shifting the mirrored image 2-3 px outward fixes the start view, but
  the rule isn't traced; the walls go through the same drawer with the
  record's own anchor (0x53C7B → 0x4E502), so the difference is likely
  in a clip set for the far row or in the blitter.
- **Animated ornaments** (0x1E3DA): attribute (cat, orn, 11, 0x0D) gives a
  frame count (bit 15: frames start at 1), cycled by `(tick + phase) mod
  count`; without it, an optional frame string (cat, orn, 5, 0x0D) is
  indexed the same way, a digit giving frames 0-9 and a letter `char −
  0x4B`. The frame keeps 6 bits (bits 10-15 of the face word) and adds
  4·frame to the image sub. The phase is 0 for wall ornaments; some
  actuators supply one from their record.
- **Wall writing**: text things in mode 0, or in mode 1 with bits 11-15 =
  14, shown when bit 0 is set. This dungeon uses only the mode-1 form,
  whose text is GRAPHICS.DAT message (3, 0, 5, bits 3-10). The map set's
  panel is drawn first (sub 0xFC on the front face, 0xFD on the left, on
  the right 0xFE or 0xFD mirrored) at slot 12, with the side x-scales.
  On the front face the text is then laid out at 1:1 on a transparent
  bitmap the size of the panel: lines 10 pixels apart (8×8 glyphs, metrics
  0x7173A), vertically centred, each line centred horizontally (a line
  wider than the panel is skipped), glyphs from the strip (8, set, 3) (A-Z
  → 0-25, '.' → 27, others blank). The bitmap is scaled and placed like
  the panel.
- **Alcoves**: an ornament with attribute (9, orn, 11, 10) holds items. On
  front faces at depth 1, unless the ornament has image sub 0x0F, the items
  lying in the wall's quadrant that faces the party are drawn as items at
  the ornament's position (0x528C5).
- **Floor ornaments** (0x50081): from the cell summary's floor slot (the
  set's attribute 0x6B, or an actuator or text thing on the square);
  category 10.

### Depth scale and shading

| Depth | 0 | 1 | 2 | 3 | 4 |
|-------|---|---|---|---|---|
| Scale (/64), 0x75B6D | 96 | 64 | 43 | 28 | 19 |
| Darkening (/64), 0x75C0C | 0 | 0 | 12 | 28 | 46 |
| Mid-step darkening, 0x75C07 | 0 | 0 | 5 | 19 | 36 |

Side wall ornaments seen obliquely use x-scale 114 (depth ≥2 side) or 76.

### Mid-step frames (traced)

0x7F258 is a step counter. The movement code (0x235BF) sets it to half the
party's move time when that is above 1, and the main loop counts it down
once per tick (0x24691). While it is non-zero the view shows the
in-between position:
- the ceiling (layout 700) moves 2 pixels up and the floor (701) 3 pixels
  down (0x7170A / 0x7170C, applied in 0x4E32A);
- lighting uses the mid-step darkening row 0, 0, 5, 19, 36 (0x75C07) and
  the map set's remap tables depth + 9 (10-13) instead of 1-4 (0x4E3D5);
  walls are drawn with their depth negated, which takes a separate branch
  of the same routine: darkening from the signed bytes at 0x75C01 + depth
  (0, −7, −9, −10 for depths 1-4, i.e. brightening), raised to at least
  −0x802CE, with the set's remap table 1;
- creatures in the party's own cell are not drawn (0x51203);
- placements through the viewport rectangle are clipped to (21, 8, 182 ×
  110) (0x7170E, in 0x1936F).

### Hit table (0x7F2EC)

While the view is drawn, clickable things record where they landed, in a
table of 12-byte records: the placed rectangle (x, y, w, h), the thing (or
0xFFFF), the view cell and a kind byte; the count is the word at 0x7F400.
A viewport click (0x22A68) tests the records in order and acts on the
first that contains the point:

| Kind | Recorded by | Click |
|------|-------------|-------|
| 1 | near floor cells | with an item in hand: drop it there |
| 2 | floor items (0x522A7); items sharing a quadrant extend one record (0x51CC6) | empty hand: take that item |
| 3 | items in the alcove ahead (0x528C5), one record | take, or place the held item in the alcove |
| 4 | door buttons at positions 3 and 4 (0x530D1) | press the button (a wall click when the door lacks the button flag) |
| 6 | wall ornaments in cells 1-3 (0x4F3DF) | the wall-click routine (0x4DDC0) |

### Backdrops (0x54699, traced; implemented)

After the ceiling and floor and before the cells, the view runs the map
set's backdrop scripts. For graphics set *s*, every non-empty text entry
(23, s, 5, n) with n below 100 whose image (23, s, 1, n) exists is a
script: two-letter lower-case keys, each followed by an optional `=` and
`-` and a decimal number (0x3F791; the last occurrence wins, a missing key
reads 0). The keys used:

| Key | Meaning |
|-----|---------|
| `cd` | Layout id to place image n at (drawn with bit 15 set, so the x offset below is a placement offset) |
| `mv` | Mode: 0 = always drawn at full size, 1 = a landmark placed by world position |
| `xl`, `yl` | Mode 1: the landmark's global position (map origin + square) |
| `fd` | Mode 1: the distance at which it is drawn at full size |
| `fw` | Mirroring kind: 8 or 0x40 follow the set's floor-flip mode, 2 or 0x20 its ceiling-flip mode (0x54874), using the same position parity as walls |

Mode 1 (0x5439A with 0x542F6): rotate (landmark − party) into view space
by the facing (forward, lateral). Nothing is drawn unless forward ≥ 1.
The distance is the integer square root of forward² + lateral² (Newton's
method; 1 when the sum is at most 2). The scale is `max(1, 64 −
(distance − fd))` in 64ths, and the x offset `lateral × 210 / distance`
screen pixels; the y offset is 0. Mode 0 uses scale 64 and no offset.

The executor (0x544BE) draws through the lit drawer 0x4E502 with the
mirror flag, the scale and the layout id, then adds the offsets. In a
mid-step frame it shrinks the offsets and scale by 52/64 and adds the
walking shift (not yet modelled). Outdoor sets use this for a horizon
strip (always drawn) and several distant landmarks.

### Outdoor weather and time of day (0x59F38, 0x5A073; traced, not implemented)

Outdoor map sets run a weather and clock model that changes both the
light and the colours:

- **Clock:** an hour index advances every 0x555 (1365) ticks, 24 per day
  (`(tick + offset) / 0x555 mod 24`, offset at 0x80434). The hour picks a
  light adjustment from a 24-byte table at 0x760EC (0x80472).
- **Light:** when the set's environment flag (0x8047B, from a per-state
  table at 0x75BE2) is on, the darkness step (0x389C2) adds
  `thresholds[clamp(0, 0x8047C + 0x80472, 5)]` to the light sum. A flag at
  0x7F248 forces the step to 0 (full light) until the next update. The
  step then drops by one when the light word at 0x7F972 exceeds 12.
- **Colours:** category-23 images are drawn through a 16-entry colour map
  chosen by the light index at 0x802CC (0x4E226), so the sky and horizon
  are recoloured by time of day and weather. The remake draws them with
  their stored colours, which is why its outdoor sky has the wrong tint.
- **Weather:** a rain intensity (0x8047E, 0 = none) rises and falls at
  random; its level (thresholds 0x10, 0x40, 0x80) and the wind direction
  relative to the facing (0x8047F) pick the rain overlay among images
  0x6D-0x74 of the set, drawn at a random offset each frame through
  layout 702 with the game's RNG (0x4E79C). Cloud and storm backdrops use
  images 0x67-0x6C (0x5A808), and storms can strike squares with
  lightning (0xFFB0 explosions) and play thunder. Ornaments and objects
  get a wet overlay while it rains (0x4E930).
- **Save:** the load path (0x370D2) restores these globals, so a
  comparison against the original needs the saved weather and clock.

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
   `scaled = (v·s + s/2) >> 6` (0x1ACC5) for size and offset (the extra
   offsets a caller adds are scaled together with the image's own); negate
   the x offset when flipping horizontally (verified: right-hand pit images
   with a non-zero x offset only line up with this rule); produce a scaled
   copy (0x1424B for 8-bit, 0x1410D for 4-bit sources) in a cache keyed by
   image and scale.
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

### Scaling (0x1424B for 8-bit, 0x1410D for 4-bit)

Nearest-neighbour. The output size is `(v·s + s/2) >> 6` for scale s in
64ths. With source width w, height h and output width W, height H:

- Rows: `T = (h << 7) / H` (integer). Output row j samples source row
  `(T/2 + j·T) >> 7`. A row identical to the previous one is copied
  instead of resampled (same result).
- Columns, 8-bit: `S = (w << 7) / W`; output column i samples source
  column `(S + 2·S·i) >> 8` (8.8 fixed point, starting half a step in).
- Columns, 4-bit: output column i samples `(S/2 + i·S) >> 7` (the
  4-bit routine 0x13F1D steps by S in 1/128 units). This differs from
  the 8-bit rule only by rounding when S is odd.
- 4-bit rows are padded to an even number of pixels.

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
  ambient light level from 0x802CC's high word, i.e. word 0x802CE, which
  the frame setup sets to the party's darkness step (0x7F282, 0-5) × 10.
  Walls are drawn through this routine with depth 0 (and the negated
  depth in mid-step frames, see above); the ceiling and floor are darkened
  by 0x802CE alone (0x4E32A, no depth row or remap). At a new game's start
  the step is 1, so everything is darkened by 10/64: leaving this out made
  the remake's start view about 19% brighter than the original in DOSBox. If the map set provides a
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
  0x71736). (docs/02 originally called this entry a palette; corrected.)
- **Wall writing font**: image (8, set, 3), a strip of 8×8 glyphs
  (metrics 0x7173A/0x7173C). Glyph index: A-Z → 0-25, '.' → 27, anything
  else → 26 (blank). Lines are centred on the wall face; the text colour map
  comes from (8, set, 3) colour data and the cell's light.

## 8. Rust implementation (`crates/dm2-engine/src/viewport/`)

`render_ex(assets, dungeon, map, x, y, facing, &ViewExtras)` draws the
view; `render` uses default extras. `ViewExtras` carries the game tick,
a lighting switch and ambient level, a seed for visual-only randomness
(the teleporter shimmer never touches the game RNG), per-layer switches,
creature descriptor indices and missile directions. All draws go through
one helper modelled on 0x4E502: image + drawing offset, caller offsets,
x-scale override (attributes 0x14/0x15 at depths 2-3), scaling, flip,
layout placement, depth light map, colour key.

What is drawn: ceiling, floor, walls; wall ornaments from text and
actuator things (front and side faces, side x-scales 114/76, per-ornament
slot from attribute 5); floor ornaments (set default 0x6B, overridden by
text/actuator things; per-cell subs 0x75C1A with the scaled fallback
0x75C31); ceiling holes; pits; stairs front-on, side-on and underfoot;
doors (panel per depth or scaled, ornament and damage overlays composed
onto the panel, vertical and split opening); the edge-on lintel; floor
items per sub-square order; creatures from the drawing descriptors with
the view/fallback/mirror rules and per-frame scale; missiles and spell
effects (sub by direction, power scaling for category 13, −92 height);
teleporter fields; depth lighting via set remap tables or ramp darkening
(0x1AFC2, including the step away from key colours).

Verification: with lighting off and only the layers the Python test
renderer draws, 25 views (walls, doors, pits, stairs, items) match
`tools/viewport.py` pixel for pixel, except where the Python tool lacks
the flip-offset rule above (two pit views, which also match once that
rule is patched into it). A sweep over every walkable square and facing
(16,944 views) runs without errors; regression tests pin eight view
hashes.

`render_full` also returns the hit table (`viewport::hits`), and
`hand::viewport_hit` takes exactly the floor or alcove item a record points
at. The frontend passes each creature's position byte and facing rule in
`ViewExtras::creatures`.

Now modelled from the traces above: wall-writing text; alcove items;
animated ornament frames; the creature grid, descriptor shift, position
nudges, mirror bit, facing rule, alternate descriptor and lunge; missile
kinds, subs, flips and scales; the teleporter row offset and mirrored-mask
shift; mid-step ceiling/floor shifts, lighting, party-cell creatures and
clip; door buttons (category-12 branch); hit records for floor items,
alcove items, door buttons and wall ornaments.

Simplifications, still TODO:
- Door buttons drawn from a door-button ornament (the branch selected by
  the cell summary rather than the door's flag bit 6) are not drawn.
- Floor items fan out by the stacking table (read from the user's
  SKULL.EXE by the frontend into `ViewExtras::stack_nudges`); alcove items
  are still drawn at the ornament's slot without stacking.
- Kind-1 (drop on floor) records are not recorded; `hand` keeps its
  layout-region rule for drops.
- The frontend supplies missile directions (`missiles::view_dir`), the
  alternate descriptor (`CreatureView::alt_frame`), the darkness step and
  ambient level, and the mid-step flag. The pointer lean is interface
  state and is not supplied yet; in-square creature positions don't exist
  (groups occupy whole squares, docs/08).
- Mid-step frames: the original defers the party's move until the step
  counter runs out and draws the in-between frame from the old square.
  The remake moves at once and plays the frame afterwards from the square
  just left (`GameState::walk`, presentation only, not saved), so the
  timing of sensors relative to the frame differs.
- The 'p'/'q' fill colours and ambient light sources are not modelled.

## 9. Open questions

- Outdoor weather and time of day (section 3) are traced but not
  implemented: the hour clock, the environment light term, the
  time-of-day colour maps for category-23 images, rain overlays, clouds
  and lightning. Until then outdoor views differ in brightness and tint
  (map 1 view (2,9,N): 22,952 differing viewport pixels, down from
  26,124 once backdrops were drawn).
- Loading in the original a remake save whose party was moved onto map 2
  or 3 stops with system error 71 (0x47, raised only by creature
  activation 0x306A8 when no slot can be freed). Moving it onto map 1 works,
  and the dungeon headers in both saves are identical. The pool is sized
  `min(non-flagged creature records + 100, header word)` at 0x342F9; why
  activation fails is not known.

- Where the actuator-supplied phase for animated ornaments comes from, per
  actuator type.
- The door-button ornament branch of 0x530D1 and the cell-summary flag that
  selects it.
- The 'p'/'q' ceiling/floor colour attributes (0x19AC4): they are read
  when the three nearest cells on the left or right are open, but their
  effect on the fill colour is not traced.
- The split-door half-panel flag 0x10 in the draw request.
- The driver side of `int 0xFC` (IBMIOP.EXE), only needed for exactness of
  cursor handling.
