# Items

How SKULL.EXE identifies items, where their properties come from, and how
the main item rules work (weight, value, charges, slots, containers,
eating and drinking, potions). Function names are Ghidra's `FUN_xxxxx` at
the given address in the relocated image (see `01-executable.md`).

Thing records and reference words are described in `03-dungeon-dat.md`;
GRAPHICS.DAT categories and the attribute namespaces in `02-graphics-dat.md`.

## Thing type to GRAPHICS.DAT category

A byte table at **0x72294**, indexed by thing type (reference bits 10-13),
gives the category whose images, names and attributes describe the thing:

| Thing type | Category | | Thing type | Category |
|-----------:|---------:|-|-----------:|---------:|
| 0 door | 0x0E | | 8 potion | 0x13 |
| 1 teleporter | 0x18 | | 9 container | 0x14 |
| 2 text | none | | 10 misc | 0x15 |
| 3 actuator | none | | 11-13 | none |
| 4 creature | 0x0F | | 14 missile | (follows the carried thing) |
| 5 weapon | 0x10 | | 15 cloud | 0x0D |
| 6 clothing | 0x11 | | | |
| 7 scroll | 0x12 | | | |

Helpers 0x1F262 and 0x1F274 do this lookup. A missile (type 14) is
resolved through its carried thing (record word 1), so a thrown dagger
uses the dagger's category.

> Note for `02-graphics-dat.md`: this table maps doors to category 14 and
> clouds to 13, so the "13 doors / 14 missiles" labels there may be
> swapped. Category 24 (0x18) is used for teleporters. Needs a visual check.

## Index within the category

0x1EFA8 returns the item's index within its category (the `I` of the
GRAPHICS.DAT key). It depends on the thing type:

| Type | Index |
|------|-------|
| weapon, clothing, misc, cloud | word 1 bits 0-6 |
| scroll | always 0 (one scroll type) |
| potion | word 1 bits 8-14 (the potion kind) |
| container | word 2 bits 13-15, plus word 2 bits 1-2 as bits 3-4 (kind 0-31) |
| creature | byte 4 (creature type) |
| text, actuator | from their own helpers (0x1FBD0, 0x1FC2C) |
| missile | follows word 1 to the carried thing |

A reference value of 0xFF80 or above is a "virtual" thing: the low byte
minus 0x80 is used directly as the index.

### Unified item number

Some code (generation rules, shops) uses a single 9-bit item number.
0x1F180 maps it to a thing type, and 0x1F1FC to the index within that type:

| Range | Thing type | Index |
|-------|-----------|-------|
| 0-127 | weapon | n |
| 128-255 | clothing | n − 128 |
| 256-383 | misc | n − 256 |
| 384-431 | potion | n − 384 |
| 432-479 | creature | n − 432 |
| 480-507 | container | n − 480 |
| 508 | scroll | 0 |

## Item attributes

Every item kind has numeric attributes under GRAPHICS.DAT type 11, keyed
(category, index, 11, attribute). 0x1F12F reads attribute *n* of a thing
(it resolves category and index first); a missing key reads as 0.
Meanings so far, from the code that reads them and the values in the data:

| Attr | Meaning | Evidence |
|-----:|---------|----------|
| 0 | Flags word | Bit 0x4000 marks coins and gems (stackable money, see below). Bit 0x10 triggers a champion refresh when the item is equipped (0x45A9D; probably light or magic effects). High bits 0x1000-0x8000 are set on magical items; meaning TODO. |
| 1 | Weight in tenths of a kilogram | The inventory info panel prints it as `x.y` (0x3962A). Summed by 0x1F6E4 with mode 1. |
| 2 | Value | Read through 0x1F8A7. Thieving creatures compare it between a champion's two hands and steal the more valuable item (0x2A9A1). Probably also merchant prices. |
| 3 | Food value | Eating adds it to the champion's food (0x39C3F). Present only on food items. |
| 4 | Allowed-slot mask | See "Equipment slots". |
| 5 | Launcher and ammunition class | Bit 15 set = launcher (bow, sling...); the low bits are a class mask. A launcher works when the ammunition's mask (bit 15 clear) shares a bit with it (0x408A8). |
| 6 | Icon animation (0x37F76): bits 0-4 frame count n, bits 5-7 group size m, bits 8-12 mode, bit 15 animate only while equipped in a slot it fits, bit 14 animate only while it is the item an action is running with. Frames start at icon sub 0x18; an active gate starts them one higher with one frame fewer. Modes: 0 tick mod n, 5 the same offset by the item number, 1 random, 2 party facing, 3 charge fraction `charges·n/(max+1) + 1`, 4 charge groups `tick mod m + (charges·(n/m)/(max+1))·m + 1`, 6 like 4 offset by the item number. Maximum charges (0x1F5D0): 15 for weapons and clothing, 3 for misc. |
| 8 | Melee strength bonus: the strength-for-an-action routine (0x46A19) adds it for melee skills (0, 4-7, 9). Shown on the info panel for weapons when non-zero (0x3962A). |
| 9 | Weapon damage (the info panel draws it as a bar scaled to 100); used in melee and throwing (0x16D72, 0x414A5, 0x478A1). |
| 0x0A | Launcher accuracy: when shooting, the missile's attack value is the launcher's attribute 0x0A + 2 × the shoot level (0x414A5 case 0x20). |
| 0x0C | Missile step (speed) of a thrown item or of ammunition (0x478A1, 0x414A5). When a thrown item has none, the step is max(5, 11 − throw level). |
| 0x0B | Armour: the low byte is the armour value (info bar scaled to 200); the high byte is probably a resistance. |
| 0x0D | Extra damage on hit, e.g. poison (0x16D72). |
| 0x13 | Duration in ticks. When the item is placed in a slot, a timer event (type 14) is scheduled this many ticks ahead (0x45A9D); torches and similar. |
| 0x14-0x1B, 0x1E-0x35 | Rare flags and amounts (bonus effects, special uses). Not yet traced. |
| 0x34 | Extra weight per charge. 0x1F6E4 adds `charges × attr 0x34` to attribute 1 (weight) when asked for it. |
| 0x35 | Extra value per charge, added the same way to attribute 2 (value). |

### Weight and value totals (0x1F6E4)

0x1F6E4(thing, n) returns attribute *n* plus adjustments. 0x1F895 calls it
with *n* = 1 and 0x1F8A7 with *n* = 2.

- **Charged items:** add the charges times attribute 0x34 (when *n* = 1) or 0x35 (when *n* = 2).
- **Potions, when *n* = 2:** if the value v is above 1 it becomes
  v/2 + power × (v/2) / 255 (power is word 1's low byte), so a full-power
  potion is worth about its listed value and a weak one about half.
- **Containers, unless their state bits (byte 4 bits 1-2) say otherwise:**
  add the totals of everything inside, recursively.
- **Money containers** (0x1F2AB): a container with clear state bits whose
  type has a contents rule, text (20, index, 5, 0x40); in this archive
  only container type 2. Each misc item inside counts as its attribute
  times (stack count + 1), where the stack count is word 1 bits 8-13;
  other things inside add their own totals. When *n* = 1 (weight), the
  stacked total is divided by 5, rounding up (`(total + 4) / 5`); for
  value it is added in full.

## Charges and stack counts

0x1F606 reads and updates charges. The bits live in record word 1:

| Type | Charge bits | Range |
|------|-------------|-------|
| weapon | 10-13 | 0-15 |
| clothing | 9-12 | 0-15 |
| misc | 14-15 | 0-3 |

Inside a money container, misc items use word 1 bits 8-13 as a stack
count minus 1 (so 1 to 64 coins per thing).

## Equipment slots

Each champion has 30 inventory slots, a u16 array at champion offset
+0xC3 (absolute 0x7FC93 + champion × 0x107). Slots 30-37 are the open
container's 8 cells, held separately at 0x7F91C.

0x152C4(item, slot, mode) decides whether an item may go in a slot. For
slots 2-12 it ANDs attribute 4 with a per-slot mask from the table at
0x7166C. Hands (0, 1) and backpack slots (13-29) accept anything.
Container cells (30-37) reject items whose attribute 4 has bit 15 set
(large items) and apply an extra check when a container sits in slot 12.

| Slot | Mask | Use (from the masks the items carry) |
|-----:|-----:|-------|
| 0 | 0x0200 | hand |
| 1 | 0x0100 | hand |
| 2 | 0x0001 | head |
| 3 | 0x0004 | torso |
| 4 | 0x0008 | legs |
| 5 | 0x0010 | feet |
| 6 | 0x0020 | pouch |
| 7-9 | 0x0040 | quiver (small weapons, ammunition) |
| 10 | 0x0002 | neck |
| 11 | 0x0020 | pouch |
| 12 | 0x0080 | quiver, main slot (any weapon) |
| 13-29 | none | backpack |
| 30-37 | none | open container |

Attribute 4 bits seen in the data:

| Bit | Meaning |
|-----|---------|
| 0x0001 | Head |
| 0x0002 | Neck |
| 0x0004 | Torso |
| 0x0008 | Legs |
| 0x0010 | Feet |
| 0x0020 | Pouch |
| 0x0040 | Quiver |
| 0x0080 | Main quiver slot |
| 0x0100, 0x0200 | Hands |
| 0x0400 | Only on torch-like items; checked when the slot argument is negative, probably a wall torch holder |
| 0x8000 | Too large for containers |

## Containers

- **Record:** word 1 points to the first thing inside. The kind comes
  from word 2 (above). Byte 4 bits 1-2 hold the container's state:
  0 means its contents count towards weight and can be shown.
- **Money containers:** 0x1F2AB says a container is one when the text key
  (20, kind, 5, 0x40) exists. That text is the contents rule described
  in `13-text.md`. Coins and gems inside stack, using the count bits above.
- **Container panel:** opening a container shows it in the inventory
  panel with 8 cells (slots 30-37, commands 0x3A-0x41).

## Item kind lists (0x1538D)

Several systems need "which items count": item relay actuator 0x40,
merchants, and creatures that collect or steal. They read a kind list,
text (15, creature or list index, 5, sub) from GRAPHICS.DAT, and turn it
into a 512-bit set with one bit per item number (see "Item numbers"
above). The list is a compact string:

- A letter sets the item-number base for the numbers after it: `W`
  weapons (0), `A` clothing (0x80), `J` misc (0x100), `P` potions (0x180),
  `C` containers (0x1E0, or 0 when the caller asks to leave containers
  out) and `S` the scroll (0x1FC). Other letters leave the base unset.
- Digits form a number; `-` makes the number before it the start of a
  range.
- When the next letter (or the end) arrives, the pending number or range
  is marked at base + number, and the base is cleared again.

So a list like `W2-4J7` (an invented example) selects weapons 2 to 4 and
misc item 7. Item relay 0x40 reads sub (word 2 bits 7-10) × 3 + 0x20 of
creature type *data* & 0xFF. The engine's parser is `items::KindSet`.

## Eating and drinking (command 0x46, 0x39C3F)

Clicking the mouth area with an item in the leader's hand feeds it to the
open champion:

- **Food (attribute 3 > 0):**
  - Adds attribute 3 to the champion's food (offset +0x44), capped at 2048.
  - Plays a short chewing animation of the mouth icon.
  - The item is consumed.
- **Water containers (0x39BBB):** add 800 to water (offset +0x46), capped at 2048. The container is kept.
- **Potions (type 8):**
  - Word 1 bits 8-14 are the kind and bits 0-7 the power.
  - Three amounts come from the power p: s = p/25 + 8, the divisor
    d = ((511 − p) / ((p + 1)/8 + 32)) / 2, and p itself. The effect
    depends on the kind (implemented in `crates/dm2-engine/src/potions.rs`):

    | Kind | Effect |
    |------|--------|
    | 6 | Dexterity +s through the stat raise 0x459C8 |
    | 7 | Strength +(p/35 + 5) |
    | 8 | Wisdom +s |
    | 9 | Vitality +s |
    | 10 | Cure poison (0x475D3) by s + p + (p/7)² |
    | 11 | Stamina + min(max − current, max/d) |
    | 12 | Armour bonus for the drinker: party effect 0x4565A with type 2, strength a = s + s/2, lasting a² ticks |
    | 13 | Mana = min(900, mana + 2s − 8); above the maximum, half the excess over max(mana, maximum) is taken off again |
    | 14 | Health + max/d, then wounds are cleared at random: AND the wound bits with a random value max(1, p/42) times, then once per try, up to 10 tries, stopping as soon as they change |
    | 15 | Water + 1600, capped at 2048 |

    Other kinds have no drink effect and the potion is not consumed.
    Afterwards stamina and health are clamped to their maxima.
  - **Stat raise (0x459C8):** when a gain pushes the current value past
    its maximum, the gain loses a quarter of itself for every full 20
    points of overshoot beyond 20; the result is clamped to 10-220.
  - **Cure poison (0x475D3):** walks the champion's pending poison events
    (type 0x4B), taking the cure amount off each event's remaining dose
    and the poison pool, deleting events that are used up and decrementing
    the poison-event count at +0x1F. The engine stores the dose in the pool
    only, so it cancels the pending events once the pool reaches 0.
  - Afterwards the potion becomes an empty flask: misc item kind 0x14 is
    created through 0x1F07C and placed where the potion was.

The champion's status flags at +0x33 are updated so the portrait redraws.
Champion offsets used here: +0x36/+0x38 current and maximum hit points,
+0x3A/+0x3C current and maximum stamina, +0x3E mana, +0x44 food,
+0x46 water. The full champion layout belongs in `06-champions.md`.

With an empty hand, clicking the mouth only waits for the button release
(it shows the food and water bars). Command 0x47 (the eye area) does the
same, showing the champion's details while the button is held.

## Throwing and the viewport

Taking, dropping and throwing all go through the viewport click handler
(see `10-ui-input.md`). Items dropped in the far half of the view are
thrown (0x227EA); the missile code then reads attributes 9, 0x0A and 0x0C.

**Throw cost (0x478A1).** Throwing costs the thrower stamina through
0x47707, with the amount from 0x4663A: let h = weight / 2 (weight in tenths
of a kg, contents included); the cost is clamp(h, 1, 10) plus half of each
positive value of h − 10, h − 20, ... So light items cost a little and
heavy ones rise steeply (a 5 kg item costs 19, a 12 kg item 85).

## Special items

- **Keys:** misc kinds 9-24. Locks are actuators that test the item
  index (see `05-timeline.md` / `08-creatures-ai.md` when written).
- **Coins and gems:** misc items with attribute 0 bit 0x4000. They stack
  inside money containers, and their weight is divided by 5 there.
- **Empty flask:** misc kind 0x14, created from drunk potions.
- **Torches and lights:** attribute 0x13 (duration) and attribute 0 bit 0x10.
- **Magic map, compass and similar:** not traced yet. The compass is a
  misc item; it probably redraws based on the party's facing.

## Function index

| Address | Purpose |
|---------|---------|
| 0x1EFA8 | Thing → index within its category |
| 0x1F12F | Thing → attribute *n* |
| 0x1F180 / 0x1F1FC | Unified item number → thing type / index |
| 0x1F262, 0x1F274 | Thing → category (table 0x72294) |
| 0x1F2AB | Is this a money container? |
| 0x1F347 | Is this a misc item with attribute 0 bit 0x4000? |
| 0x1F606 | Read and clamp an item's charges |
| 0x1F6E4 | Attribute total including charges and contents |
| 0x1F895 / 0x1F8A7 | Weight total / value total |
| 0x152C4 | May this item go in this slot? |
| 0x408A8 | Is the launcher in hand compatible with this ammunition? |
| 0x39C3F | Eat or drink |
| 0x39BBB | Water container check |
| 0x45DEF | Put an item in the leader's hand (cursor) |
| 0x45E9E / 0x45F27 / 0x45FCB | Take from or put into a champion's slot |
| 0x46029 | Inventory or hand slot click (commands 0x14-0x41) |
| 0x2A9A1 | Creature steals the more valuable hand item |
