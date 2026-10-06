# User interface and input

How the screen is laid out, how mouse clicks and keys become commands,
and what each command does. Tool: `tools/uizones.py` dumps the zone and
key tables from the unpacked data object (`re/skull/obj2.bin`).

The input path is:

1. **Mouse or keyboard** to a **command number**, using the zone and key
   tables (below).
2. The command is **queued** in a 3-entry ring of 14-byte events at
   0x7F4DC: x, y, command, and so on.
3. The **dispatcher** at 0x21D6C runs the command.

## Layout: the rectangle table (GRAPHICS.DAT 1,0,4,0)

The positions of all UI elements come from one data entry, key
(1, 0, 4, 0), entry 201 (about 17 KB). It is not hard-coded in the
executable. 0x19D0E parses it at startup into a linked list of blocks
(head pointer at 0x71796), and 0x190E9 looks up a rectangle by id.

| Offset | Content |
|--------|---------|
| 0 | u16 marker 0xFC0D |
| 2 | u16 N, the number of id ranges (30) |
| 4 | N pairs of u16 (first id, last id) |
| then | for each range, for each id in it: 4 × s16 (kind, parent id, a, b) |

When parsing, each range is narrowed to the smallest storage it fits:
- Fields that are constant across the whole range are stored once.
- Small values are stored as bytes.

Flags record which fields were narrowed: bit 0 means the parent is
constant, bit 1 the kind is constant, and bit 2 means byte storage.

Resolving a rectangle (0x1936F) walks up the parent chain to an absolute
screen rectangle:

| Kind | Meaning |
|------|---------|
| 9 | A box of size (a, b); the roots are 320×200 (id 1) and the 224×136 viewport |
| 0-8 | Placed at offset (a, b) from the parent, using one of 9 anchor points (corners, edge midpoints, centre) |
| 10-18 | Same as 0-8 (anchor = kind − 10) but with zero offset |
| 1 | Also used as a size box |

Bit 15 of a requested id adds a caller-supplied offset (used for
scrolling panels). 0x1902B is the point-in-rectangle test.

Rectangle ids that matter:
- 7: viewport
- 40-45: movement arrows
- 161-164: champion portraits
- 209-216: hand cells on the portraits
- 245-250: spell runes
- 252: cast
- 254: delete rune
- 507-536: inventory slots
- 545: mouth
- 546: eye
- 615: champion name
- 229-236: container cells
- 221-226: dialog buttons

## Mouse zones

### Zone records (table at 0x72388)

Each record is 3 words:

| Word | Bits | Meaning |
|------|------|---------|
| 0 | 0-10 | Command number |
| 0 | 15 | First record of a list |
| 1 | 0-13 | Rectangle id |
| 1 | 15 / 14 | Resolve the rectangle relative to anchor rectangle 7 (viewport) / 18 instead of the screen |
| 2 | low byte | Mouse button mask: 0x02 left, 0x01 right, other bits unknown |
| 2 | high byte | Flags: 0x08 disabled; 0x80, 0x40, 0x20 and 0x10 still to be identified (probably repeat or release behaviour) |

A list runs until the next record with bit 15 set. 0x20B01 tests the
click against each enabled record whose button mask matches, and the
first hit becomes the command.

### Choosing active lists (0x203A3)

The set of active lists depends on the current screen state. That
selection is a small byte-coded tree:
- **Roots** at 0x72CCC.
- **Nodes** at 0x72B9C. Each node byte is a predicate index into a function
  table at 0x722A4, with bit 7 meaning "sub-tree".
- **Screen records** at 0x72D1D, 7 bytes each: u16 condition, u16 rectangle
  id that must contain the click, s16 index of the zone list, and a byte.

For example, portrait lists are only active while the champion exists, the
inventory lists only while an inventory is open, and the dialog lists only
during a dialog.

Notable lists (`uizones.py lists` prints all 60 or so):

| List | Contents |
|------|----------|
| @39 | Movement arrows: commands 1-6 on rectangles 40-45 |
| @45 | Viewport: command 0x50 on rectangle 7 |
| @15/22/28/34 | Champion portrait n: open inventory (7-10), hand cells (0x14-0x1B), name area |
| @53 | Inventory: slots 0x1C-0x39, mouth 0x46, eye 0x47 |
| @47 | Inventory side buttons: save 0x8C, 0x8E, close 0x0B, sleep(?) 0x8D, rename 0x48 |
| @97-112 | Action area per champion: hand icons 0x74-0x7B, leader-cell 0x5F-0x62 |
| @117-163 | Action menus: choices 0x71-0x73 and 0x56-0x59, cancel 0x70, rotate champion 0x5D/0x5E |
| @166, @172 | Spell area: runes 0x65-0x6A, cast 0x6C, delete 0x6B |
| @174 | Open container: cells 0x3A-0x41 |
| @183, @191-259 | Dialogs: buttons 0xE4-0xE9 and choices 0xDB-0xDE (two anchor variants each) |
| @0 | Title menu: 0xD7-0xDA and 0xE0 |

## Keyboard (table at 0x729B8)

Records are 4 bytes: a u16 command (bit 15 starts a list) and a u16 key.
The key's low byte is a BIOS scan code; 0x200 means Shift, 0x400 Alt and
0x800 Ctrl. A record of 0x8000, 0x0000 ends the table. Lists are chosen
the same way as mouse lists. Default bindings (`uizones.py keys`):

| Key | Command |
|-----|---------|
| Keypad 4 / 5 / 6 | turn left / forward / turn right |
| Keypad 1 / 2 / 3 | step left / back / step right |
| Keypad 7 / 8 / 9 | commands 0xF0-0xF2 (dispatcher branch at the end of 0x21D6C) |
| 1-4 (top row) | open champion 1-4's inventory, or close it (0x0B) |
| Ctrl+S | save game (0x8C) |
| Esc | pause (0x90) / resume (0x91) |
| Space | 0x52 when not paused; closes the inventory (0x0B) while it is open |
| Enter | start (title menu, 0xD7); also 0xEF and 0x8F in other states |
| Alt+Q | 0xE0 (title menu) |
| Letters, digits, punctuation, Backspace (0xA5), Enter (0xA7), Tab (0xDF), Esc (0x4B) | text entry (save names, champion rename): commands 0xA5-0xDF |

## Command numbers (dispatcher 0x21D6C)

| Command | Action | Handler |
|---------|--------|---------|
| 1, 2 | Turn left, right | 0x2339D |
| 3-6 | Move forward, right, back, left (relative to facing) | 0x235BF |
| 7-10 | Toggle champion 1-4's inventory | 0x3A464 |
| 0x0B | Close inventory | 0x3A464 |
| 0x10-0x13 | Party cell clicked (swap champions' positions) | 0x219C2 |
| 0x14-0x1B | Hand cells on the portraits (champion = n/2, hand = n%2) | 0x46029 |
| 0x1C-0x39 | Inventory slots 0-29 of the open champion | 0x46029 |
| 0x3A-0x41 | Open-container cells (slots 30-37) | 0x46029 |
| 0x46 | Mouth: eat or drink the held item | 0x39C3F |
| 0x47 | Eye: show details while held | 0x3A409 |
| 0x48 | Rename champion (dialog; 0x49 name field, 0x4A title field, 0x4B cancel) | 0x496E5 |
| 0x50 | Click in the viewport (see below) | 0x22A68 |
| 0x52 | Leader-related action (only with a leader) | inline |
| 0x55 | Click on a champion's name in the action area | 0x21A49 |
| 0x56-0x59 | Choose from the action list | 0x41081 |
| 0x5D, 0x5E | Rotate a champion's facing within the party | inline (champion +0x1C) |
| 0x5F-0x62 | Select leader by party cell | 0x458F4 → 0x3FE03 |
| 0x65-0x6A | Add a spell rune | 0x42924 |
| 0x6B, 0x6C | Delete rune, cast spell | 0x429F5, 0x428A2 |
| 0x70 | Cancel the action menu | 0x40DEC(−1) |
| 0x71-0x73 | Pick an action | 0x40DEC(n) |
| 0x74-0x7B | Hand icons in the action area (champion = n/2, hand = n%2) | 0x3FD77 |
| 0x7D-0x81 | Unknown (rectangles 49-52, 4 buttons) | 0x149BA, 0x149A2 |
| 0x8C | Save game | 0x3502B |
| 0x8D | Unknown (sleep?) | 0x1AA75, then 0x48890 |
| 0x8E | Freeze or game menu | inline |
| 0x8F | Unknown | 0x46D9C |
| 0x90, 0x91 | Pause, resume | inline |
| 0x92, 0x93 | Unknown | inline |
| 0xD7, 0xD8, 0xD9 | Title menu: new game, new game with the alternate dungeon (sets 0x803F8, so DUNGENB.DAT), resume a saved game | inline |
| 0xDA | Title menu (0x38877) | |
| 0xE0, 0xE1, 0xE3 | Title-screen actions | 0x2005B, 0x2056F, 0x20588 |
| 0xE4-0xE9 | Dialog buttons | 0x42A35 |
| 0xEA-0xED | Menu choices | 0x45D6B |
| 0xF0-0xF2 | Keypad 7-9 | inline |

Movement and turning commands are deferred while the party is busy:
- During movement cooldowns, commands 3-6 are only accepted when the state
  flags at 0x7F26A and 0x7F25A allow them.
- A pending command sets 0x7F484, which keeps it in the queue.

### Input playback

0x21394 can feed commands from a byte-coded script (pointer 0x7F47C)
instead of the mouse. Opcodes (low 6 bits):

| Opcode | Meaning |
|--------|---------|
| 0 | Wait n ticks |
| 1 | Sync |
| 2, 3 | Set flags |
| 4, 6 | Enqueue an event |
| 5 | Run the dispatcher directly |

It is probably used by the intro or demo and by tutorial sequences. Not
needed for normal play.

## The leader hand (cursor item)

- **Held item:** the item in the leader's hand is a single global,
  0x7FBB4 (0xFFFF when empty), drawn as the mouse cursor.
- **Leader:** the leader's index is at 0x759A2 (high word; −1 = none).
- **Open inventory:** the open champion's number, plus one, is at 0x7F970
  (high word; 0 = no inventory open).
- **Hand to cursor:** 0x45DEF puts an item in the hand. It caches the
  item's flags and weight, redraws the cursor and updates the load display.
- **Slot clicks (0x46029):** these swap the held item with the slot's
  contents when 0x152C4 allows the held item there. Equipping hands,
  pouches, quivers, neck or container cells refreshes the action area.

### Viewport clicks (0x22A68)

Coordinates are made relative to the viewport origin at 0x716F8. The
handler then tries these in order:

1. **Things drawn in the last frame.** A table at 0x7F2EC, with count
   0x7F3FE, records each drawn thing's screen rectangle and a kind byte:
   - Kinds 1-3 (items on the floor or in an alcove): take the item if the
     hand is empty (0x226C9), otherwise put the held item there (0x229BB).
   - Kind 4 (door button): press it. This plays a sound, queues timer
     event 0x58 and marks the door.
   - Kind 6 (wall ornament or creature): 0x4DDC0, then 0x22544.
2. **With an item in hand:**
   - The two near floor cells (rectangles 0x2F8 and 0x2F9) drop the item.
   - The two cells of the square ahead (0x2FA, 0x2FB) also drop it, but
     only if that square is open.
   - Cells of a creature in front can receive the item (giving; 0x227EA).
   - Otherwise 0x22942 handles a throw into the view.
3. **With an empty hand:** rectangles 0x2FD and 0x2FE are the wall in
   front (switches, actuators).

## Dialogs (category 26)

Every message box, menu and question is data-driven:
- **Building and running:** 0x1A37D(dialog, arg) builds the dialog, and
  0x1A0A4 runs it and returns the chosen button.
- **Lines:** a dialog's lines are the texts (26, dialog, 5, sub) for sub 0-19.
- **Background:** the image (26, dialog, 1, 0); dialog 0 provides the
  default frame.
- **Buttons:** (26, dialog, 11, sub) gives the button code. The low byte
  is the value returned (the line number if 0); the high byte is a
  hotkey or default-button marker.
- **Clicks** arrive as commands 0xDB and up (0xDB + button index).

Save, load and disk-swap prompts all use this mechanism; for example, the
save path asks dialog 0x1B and then 0x0D, and the load path 0x0F and 0x0E.

## Title screen

The title loop is at 0x386F5. The menu picture is the 320×200 image
(5, 0, 1, 4): a signpost whose New, Resume and Quit boards line up with
zone rectangles 407 (91,52 48×26), 409 (14,65 65×27) and 434 (38,94
45×30); rectangle 411 is a small plaque at the bottom (command 0xDA).
Image (5, 0, 1, 1) is a credits screen, not the menu. Commands come from
zone list @0 and key list @0 (Enter = 0xD7, Alt+Q = 0xE0). The hotspot
entry (5, 0, 7, 4) is not decoded (TODO); the zone list is enough.

## Main game screen (verified by rendering)

The interface is drawn with the draw-at-layout routine 0x1BE6A
`(cat, idx, sub, target, layout id, colour key)`, where a key of −1/0xFFFF
means opaque. Most panels are first built in an off-screen buffer the size
of a layout rectangle (0x1BABB) and then copied to the screen (0x1BB0E),
but resolving the same ids straight to screen coordinates gives the same
result. Colours come from the 16-byte table (1, 0, 13, 254), loaded once
(global 0x7F210).

| Element | Image | Layout id | Notes |
|---------|-------|-----------|-------|
| Movement arrows (0x42AE4) | (1, 3, 2 + 2k), or 14 + 2k for the alternate set (flag 0x7F3A4) | 40-45 | Grid of 29×23 buttons from (229, 129) |
| Champion box (0x48140) | (1, 2, s): s = 0 alive, 1 dead (health 0), 9 inventory open | 161 + n | Boxes are 77×37 along the top edge, x = 0, 81, 162, 243 |
| Portrait (0x487F9) | (22, portrait, 0) | 173 + n | Portrait index is champion byte +0x101 |
| Dead champion's name (0x48890) | text, colour [15], transparent | 165 + n | Only drawn in the dead state |
| Stat bars (0x481DC) | filled rectangles | 193 + n + 4·bar | Health, stamina, mana. Height = parent box × current / maximum (0x19BF2, minimum 1 px), placed through the bar record's own anchor. A shadow copy offset by the words at 0x71724/0x71726 is filled first with colour [0]; the bar uses colour [table 0x759CC, one byte per champion] |
| Damage starburst (0x48733) | (1, 2, 3), key 10, plus the number in colour [15] on [8] | 177 + n | Only while the champion has damage to show |
| Leader bar (0x43332) | (1, 4, 20) and (1, 4, 14) | 60 and 59 | Name text at 61 in colour [9] (leader) or [15] |
| Spell panel (0x43686) | (1, 5, set + 1), set = champion byte +0x1E | 92 | Six rune symbols drawn as font characters `'`' + 6·set + k` at ids 255-260; entered runes at 261 and up (0x435D3) |

### Inventory panel (0x48890, inventory open)

Drawn into a viewport-sized buffer, so its layout ids resolve relative to
the viewport (rect 3), then shown where the 3D view normally is.

| Element | Image / source | Layout id |
|---------|----------------|-----------|
| Background | (7, 0, 0) | 4 |
| Slots | table at 0x75538 (below) | 507-536 |
| Mouth | frame (1, 2, 4), then (7, 0, 0x25) | 545 |
| Eye | frame (1, 2, 4), then (7, 0, 0x20), or 0x21 while a container is open | 546 |
| Name and title | text, colour [15] | 553 |
| Health, stamina ÷ 10, mana | "cur/max" numbers (0x483CB), colour [13] | 550-552 |
| Load | numbers; colour [8] over the maximum, [11] over 5/8 of it, else [13] | 555 |
| Food and water (0x39A4D) | panel (7, 0, 1) at 494; bars at 496 (colour [5]) and 497 (colour [14]) over −1024..2048 (0x398FF); labels (7, 0, 6) and (7, 0, 7) at 500 and 501; poison bar 499 and label (7, 0, 8) at 502 while poisoned | 494-502 |

**Slot table (0x75538).** 38 records of 8 bytes: a layout id and the
sub-index of the slot's "empty" picture in (7, 0) (0xFF = none). Records
0-7 are the hand cells on the champion boxes (champion × 2 + hand, ids
209-216); records 8-37 are inventory slots 0-29 (ids 507-536). The slot
drawer 0x3815D puts a frame (1, 2, 4), 5 or 6 (selected) under hand cells
and the first six inventory slots, then the item icon with colour key 12.
The icon is sub 24 of the item's (category, index); 0x37F76 picks a later
sub for animated or charged items (attribute 6), which the remake does
not model yet.

### Action area (0x3FE68 and its helpers)

Positions in the action area are by party cell relative to the facing,
`rel = (cell + 4 - facing) & 3`, never by champion index. The idle panel
is drawn in this order (all checked pixel for pixel against the original):

- **Hand cells (0x42DA6)**, for each living champion and each hand:
  - The cell is at id 0x4A + rel for the ready hand, 0x46 + rel for the
    action hand.
  - The tile is (1, 4, 2), or (1, 4, 4) when that hand is the last one
    selected.
  - The hand's icon is centred on the tile as a 17x17 box:
    - With an item, a one-pixel drop shadow in colour 0 comes first, and
      the icon's colour map is run through the 256-byte remap table
      (1, 0, 7, 1) (0x1AF61), which gives the dimmed look.
    - With an empty hand, the bare-hand picture (1, 2, 7 + hand) is drawn
      plainly.
    - Item icons here always use the base frame: the sub-index routine is
      called with its context flag off.
  - A busy hand, or the whole party asleep, adds a checkerboard of colour
    0 over the cell (0x13745).
  - A dead champion's cells are cleared to colour 0.
- **Formation cells (0x4315D):** back image (1, 4, 6 or 8) at 0x57 + rel
  and front image (1, 4, 10 or 12, +1 for the leader) at 0x53 + rel. Both
  are mirrored for rel 1 and 2 and keyed on nibble 4. The back image is
  checkerboard-shaded while the party sleeps or the champion shows damage.
- **Formation grid (0x42EDD):**
  - The floor is (8, map graphics set, 0xF5) at 0x2F.
  - Each living champion's figure is a 17x17 cut from that champion's
    sheet (1, 6, champion) at x = 17 x (rel + 4 while invisible), drawn at
    0x35 + rel and keyed on nibble 12. The sheets are 8 cells wide.
- **Last-selected hand:** the original sets 0x7FB50/0x7FB4C when a hand
  or party cell is clicked and redraws a cell only when its contents
  change, so the highlighted tile stays on screen. After a new game it is
  the leader's action hand. The remake keeps it as presentation state.
- **Action menu (0x43827):**
  - Each row is (1, 4, 0x15) at 0x3F + row, with the action's name at
    0x42 + row as shadowed text at the plain position (no row shift).
  - Below the rows comes a strip (0x433D6): floor (8, set, 0xF6) at 0x5D,
    the champion's figure at 0x5E, then (1, 4, 0x10) at 0x60 and
    (1, 4, 0x12) at 0x61.
  - The name bar above (0x43332) draws the name the same way.
- **Which actions are listed (0x3F9F5):** action strings 8-11 are tried,
  keeping the first three that pass all of these:
  - they have a command (CM);
  - their hand restriction (WH) is 0 or the hand + 1;
  - the champion's level in their skill (SK) reaches the required level
    (LV);
  - for a bare hand's command 0x11, there is something to use (0x3FC6D:
    slot 12 for the action hand, or slots 7-9).

  Items held in the hand have further checks (0x3F927, code 8) that the
  remake does not model yet.
- **Open container:** fills the action area with 8 cells at screen ids
  229-236 (zone list @174, commands 0x3A-0x41).

### What the remake implements (`crates/dm2-engine/src/hand.rs`)

- Slot clicks swap the held item with the slot when `slot_fits` allows it;
  loads are recomputed.
- Container cells hold the container's list (record word 1). Following the
  first game, a container is shown open while it sits in the open
  champion's action hand. That trigger is still an assumption: the
  original decides it through the zone condition tree (list @174), which
  is not decoded.
- **0x7F224 is the eye flag, not a container flag.** 0x3A409 (the eye
  click) sets it while the button is held and clears it on release, and
  0x20588/0x206C6 reset it with its neighbours 0x7F21C and 0x7F220. The
  inventory panel draw (0x48890) uses it to choose what the details area
  shows:
  - flag clear: the open champion's action-hand item (slot 1). A scroll
    shows its text (0x3918A); otherwise the default food and water view
    is drawn (0x39A4D);
  - flag set: the leader's held item's details (0x3962A with mode 1), or
    the champion's own details when nothing is held (0x3A12A).
  The eye icon's glyph (layout 0x222) switches with the flag. A separate
  state at 0x7F284 (set by 0x494FB to a champion number + 1, cleared by
  0x49A17) replaces the details with a fixed wall-ornament picture
  (category 9, index 0x5B) through 0x39B3F; it belongs to the recruit
  flow, not to containers.
- The mouth eats food (attribute 3) and drinks potions. Every potion kind
  with a drink effect is modelled (docs/09), and the potion becomes an
  empty flask (misc 0x14).
- Viewport clicks use the six regions 0x2F8-0x2FE: take the top item of a
  floor cell, drop into a floor cell (the square ahead only when it is
  open), or click the wall ahead (`click_wall`, with the held item). Any
  other click with an item throws it.
- Throwing follows 0x478A1:
  - energy = strength for the throw skill (10) + random(energy / 4 + 8) +
    throw level;
  - attack = clamp(40, (rnd & 31) + 8 × level, 200);
  - step = attribute 0x0C, or max(5, 11 − level);
  - experience is 8, or 12 + attribute 9 / 4.
  - The stamina cost here is weight / 10, a stand-in for 0x4663A.
- Not yet modelled: the drawn-things hit table (clicking a specific item
  sprite, door buttons, giving items to creatures in front).
- Actions run through `combat::do_action` and `apply::apply_action`, and
  the hand stays busy for the BZ ticks.
- Runes are entered, removed and cast through `magic`.

## Zone rectangles

A zone's rectangle id is a record placed inside a size box (its parent,
kind 9); the clickable area is that box's size at the record's placement.
Anchor flag 0x8000 makes the box relative to the viewport frame (rect 7,
screen (0, 40)); 0x4000 relative to rect 18. This rule resolves every id
used by the zone lists.

## Interface states (checked against the original in DOSBox)

Captures of the original under DOSBox were compared with the remake's
screenshots (`dm2 --screenshot ... [--cmd C] [--load SAVE]`; `--load`
renders a state the original saved). These rules make every interface
pixel match in the states checked (new-game start, inventory open,
bare-hand action menu):

- **Champion box (0x48890):**
  - The portrait (0x487F9) shows only while that champion's inventory is
    open. Otherwise the box holds the name at 0xA5 + n and the two hand
    slots.
  - The leader's name is in colour 9, the others in 0xF (0x48DD3).
  - The hand slots do get their frame (0x3815D): (1, 2, 4), 5 when that
    body part is wounded, or 6 for the hand whose menu is open. The 18x18
    frame is centred on the 16x16 slot box without being clipped, so it
    starts one pixel up and left of the slot. Empty-slot pictures move
    one sub further when the part is wounded. The eye, the mouth and the
    first six inventory slots use the same frame placement.
- **Shadowed text (colour flag 0x4000):**
  - The shadow is two copies in colour 0, one row down and one row down
    and right.
  - In the champion box and the inventory (name, stats, load line), the
    text itself is one row lower than plain text. In the action menu and
    its name bar it stays at the plain position.
  - The low colour byte passed with the flag (colour 0xC at 0x43332)
    does not show.
- **Right panel (0x3FE68):** with nobody selected it shows the idle
  action area above. Selecting a champion through a hand cell (0x74-0x7B)
  or a party cell (0x5F-0x62) shows the name bar with that champion's
  action menu, and 0x3FD17 deselects. The spell panel (0x43686) is also
  part of the selection state and isn't shown by default.
- **Inventory open (0x3A464):** the arrows panel is shaded with a colour-0
  checkerboard (0x13B7C on layout id 9, sized as rectangle 8).
- **Inventory name line (0x229):**
  - Name and title are joined by the separator string (pointer at
    0x760E0; a single space here) unless the title starts with ',', ';' or
    '-'.
  - The five name-bar images from 0x48863 are drawn before the name,
    because the first one, (7, 0, 0x11) at 0x238, covers the whole bar.
- **Food, water and poison bars (0x398FF):**
  - The fill fraction is computed in 1/10000 steps as a 16-bit value,
    then scaled to the box width by 0x19BF2, with a minimum of one pixel.
  - A colour-0 copy offset by (2, 2) is drawn first as the shadow.
  - The bar colour becomes 8 below -512 and 0xB below 0.

Not yet compared: the active spell panel, the held-item cursor, dialogs
(save, slot list, "game loaded"), the paused screen and the inventory
with the eye pressed.

## Open questions

- Meaning of the zone flag bits 0x80, 0x40, 0x20 and 0x10, and of button
  bits other than 0x01 and 0x02.
- The predicate functions at 0x722A4 (which state each selects). The
  remake chooses the same lists from explicit UI state instead
  (`crates/dm2-engine/src/input.rs`).
- Commands 0x52, 0x7D-0x81, 0x8D-0x8F and 0x92-0x93.
- Rectangle anchor details for kinds 0-8 (which index is which corner).
