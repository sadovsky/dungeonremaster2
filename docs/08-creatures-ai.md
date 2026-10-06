# Creatures and AI

DM2 creature behaviour is data-driven. Each creature type has a fixed
*info record* compiled into SKULL.EXE. An *AI class* picks from a list of
*behaviour programs*, and each program is a small bytecode of 7-byte
commands, run by an interpreter with a square-search planner underneath.
Animation is a separate per-type frame table in GRAPHICS.DAT, and frames
can trigger gameplay events.

Status labels: **code** = read from the decompilation; **tentative** =
inferred from how a value is used, not yet confirmed.

## Where creature data comes from

| Source | Key | Contents |
|--------|-----|----------|
| GRAPHICS.DAT numbers | (15, type, 11, n) | Per-type attributes, read through 0x1F8FF (cached for n = 0, 1 and 5) |
| Executable table | 0x71968 + 36 × attr5 | The info record (63 records). `0x1F8D9(type)` returns a pointer to it. |
| Executable table | 0x7507A + 4 × attr1 | AI class flags (32-bit) |
| Executable table | 0x74F7E + 4 × program | Pointers to behaviour programs (programs 0 to 62) |
| GRAPHICS.DAT | (15, type, 8, 251) | Action-to-sequence map |
| GRAPHICS.DAT | (15, type, 7, 252) | Frame table, 4 bytes per frame |
| GRAPHICS.DAT | (15, type, 7, 253) | Per-frame drawing descriptors, 8 bytes each, indexed in parallel with 252 (0x14CF2) |
| GRAPHICS.DAT | (15, type, 7, 254) | Further per-type drawing data (used at 0x51540; tentative) |
| GRAPHICS.DAT | (15, type, 2, n) | Sounds; frame byte 0 selects one |

### Attributes (15, type, 11, n) used by the code

| n | Meaning | Notes |
|---|---------|-------|
| 1 | AI class | Indexes the class flags at 0x7507A and the class's behaviour list |
| 4 | Maximum group size | 0x1F9DF; a missing value defaults to 4 |
| 5 | Info record index | Present for 74 of the 76 types; values 2 to 62 |
| 6 | Preload flag | Non-zero means the type's graphics are queued for loading with the map (0x35250 loop over 250 types) |
| 7 | Unknown | Copied to a global (0x7F57E low word) when the AI context loads |
| 0x41 | Alternative attribute used when info+0x1E low nibble is under 2 (0x51EB7) | tentative |
| 0xF9 | Read at 0x42501 | TODO |

Attributes 0, 10, 11, 30 and 160 to 174 exist in the data but nothing
was seen reading them directly; they may be read through generic
helpers. TODO.

### Info record (36 bytes at 0x71968)

| Offset | Size | Meaning | Status |
|--------|------|---------|--------|
| 0x00 | u16 | Flags. Bit 0: inanimate or scripted (no normal think step; it is driven by its own state and gets activated when hit). Bit 5: affects missile and door handling. Bits 6-7: size class, compared with how far a door is open to decide whether it fits. Bit 9 is tested in the drawing code. | code (bit 0, bits 6-7); others tentative |
| 0x01 | u8 | Flags: 0x04 and 0x08 (drawing and attack-mode checks), 0x10 (exempt from the slowdown applied away from the party's map) | tentative |
| 0x02 | u8 | Defence (armour) | code |
| 0x03 | s8 | Regeneration period. Every abs(n) × 4 ticks a positive value adds HP up to the maximum; a negative value adds the bonus to a different counter (0x257CC). | code |
| 0x04 | u16 | Base or maximum HP. A transformation sets HP to this plus rand(this / 8 + 1). Also the denominator of the damage-percentage check. | code |
| 0x06 | u8 | Attack strength | code |
| 0x07 | u8 | Checked as a flag in attack code | TODO |
| 0x08 | u8 | Dexterity, used in the to-hit roll; 0xFF means it cannot be hit by other creatures | code |
| 0x09 | u8 | Random jitter applied to the drawn position: bits 0-1 x, bits 2-3 y | code |
| 0x0A | u16 | Terrain mask (see Movement). The top bit is cleared under some condition (0x30102). | code |
| 0x0C | u16 | Item-handling flags. Bit 3: picks up items; mask 0x77 covers the item classes it will take. | tentative |
| 0x0E | u16 | Goal filter mask (ANDed with per-goal masks by the planner) | tentative |
| 0x10 | u16 | More planner flags; bit 5 enables the door-side checks | tentative |
| 0x12 | u16 | Planner flags | TODO |
| 0x16 | u16 | Bits 0-3: alertness; the creature notices the party with probability ((15 − n) × 2 + 1)/(something). Bits 4-7 are used in drawing. Bits 12-15: chance of fleeing or turning away (opcode `Q`). | partly code |
| 0x18 | u16 | Bits 4-7: missile-damage threshold (0xF = immune) at 0x16746. Bits 8-11: wander randomness in the think loop. | code |
| 0x19 | u8 | Bit 0x10: halves incoming spell or missile damage | tentative |
| 0x1A | u16 | Used in the attack-effect code at 0x18758 | TODO |
| 0x1C | u8 | Attack type selector at 0x18758 | tentative |
| 0x1D | u8 | 0xFF disables a branch in the hit code (0x1726B) | TODO |
| 0x1E | u16 | Low nibble selects among attribute variants (0x51EB7) | TODO |
| 0x1F | u8 | Flags: bit 0 and bit 3 (vulnerability or attack checks) | TODO |
| 0x20 | u16 | Read together with 0x1E | TODO |
| 0x23 | s8 | Size: 0 small, 1 medium, 2 large. Sets the volume of the "impact" sound effect and the layout of positions within a square (0x2FBE0). | code |

## Creature thing record (DUNGEON.DAT type 4, 16 bytes)

See also docs/03.

| Offset | Meaning |
|--------|---------|
| 0x00 | Next thing |
| 0x02 | Possessions list head (0xFFFE = empty) |
| 0x04 | Creature type |
| 0x05 | Active slot number (0xFF = inactive) |
| 0x06 | HP |
| 0x08 | 0xFFFF makes the first action after activation 0x11 instead of 0 |
| 0x0A | Status bits. A hit can set or clear any bit through a mask (0x24E62); bit 2 = afraid (set by heavy damage); bits 3 and 6 change direction choice and random wandering; 0x80 marks an item-carrying or stealing state; bits 0x2048 enable the "move away" behaviour. |
| 0x0C | Animation parameter for inanimate creatures (passed to sequence selection) |
| 0x0E | Bits 8-9: facing. Bit 10: temporary flag kept across activation. |

## Active creature slots

Creatures in or near play get a 34-byte slot in a pool at 0x7F898, sized
by 0x7F89C (from DUNGEON.DAT's spare counts, see docs/03). Activation is
at 0x306A8. A slot is free when word 0 is negative. If the pool is full,
the game force-deactivates another creature through 0x1D748 and tries
again; it raises error 0x47 if that fails.

| Offset | Meaning |
|--------|---------|
| 0x00 | Creature thing index (bit 15 set = free) |
| 0x02 | Pending timeline event (0xFFFF = none) |
| 0x04 | Low byte of the tick at activation − 0x7F |
| 0x06 | Regeneration timestamp, (tick / 4) − 1 |
| 0x07 | Drawing jitter: bits 0-2 x, bits 3-5 y, bit 6 flip (re-rolled each frame if the frame asks) |
| 0x0C | Home position: x (bits 0-4), y (bits 5-9), map (bits 10-15) |
| 0x0E, 0x10 | Program variables 0 and 1 (set by the −10 directive or by opcode `Z`) |
| 0x12 | Current program (−1 = none) |
| 0x13 | Current step |
| 0x14 | Damage taken but not yet applied |
| 0x16 | Initialised to 0xFF (TODO) |
| 0x17 | Queued action (−1 = none) |
| 0x18 | Target square: x (bits 0-4), y (bits 5-9), map (bits 10-15) |
| 0x1A | Current action code (see below; 0xFF = none) |
| 0x1B | Direction argument for the action |
| 0x1D | Facing to adopt when a turn action finishes |
| 0x1E | Transform-into creature type, or another argument |
| 0x1F | Stage counter for multi-step actions (such as transforming) |
| 0x20 | Mode for opcode `]` |
| 0x21 | Whether frame events stay armed |

## The creature tick

Creatures run off the timeline (docs/05). Event 0x22 starts a new action
(taking the queued one, or running think), and event 0x21 continues the
current animation sequence: the driver schedules 0x21 while the frame
stepper reports more frames to play and 0x22 once the sequence ends.
(Earlier notes called 0x21 a wake-up event; the driver at 0x25420 shows
otherwise.) Both go through 0x24A88, which
loads the creature, slot, info and animation pointers into globals at
0x7F548 to 0x7F57E, the context every other AI function uses.

1. **0x257CC (step event):** clear the slot's timer. A creature at 0 HP
   is set to 1 HP with 1 point owed. Regeneration (info+3): every
   |n| × 4 ticks since the slot's stamp (+6), a positive n heals up to the
   base HP (info+4) and a negative n adds the same amount to the damage
   owed, so it slowly wears down. Owed damage (+0x14) is applied through
   0x31348, after clearing status bit 15 on animate creatures. An inanimate creature (info flag bit 0) only plays its
   animation. Everything else goes to the animation driver 0x25420.
2. **0x25420 (animation driver):** if a queued action exists (+0x17),
   make it current (+0x1A); otherwise call **think** (0x262F7) to choose
   one. Look up the action's sequence (0x14E42) and step its frames
   (0x14F1B for the first frame, 0x1501A for later ones). When a frame
   has bit 7 of byte 2 set, run its gameplay event through 0x2B75E
   (attacks, spells, transformations). Actions 0x32 to 0x34 are plain
   waits of (action − 0x32) ticks. Finally schedule the next step
   (0x3023F) and add it to the timeline (0x56390).
3. **0x3023F (frame timing):** plays the frame's sound (frame byte 0 low 7
   bits; 0x7F = silent), applies random drawing jitter (frame byte 3 bit 0
   uses info+9) and random flip (byte 3 bit 1), then computes the delay
   as (byte 3 bits 4-7) + rand(byte 3 bits 2-3). Creatures on a map other
   than the party's run slower, unless their AI class has bit 0 set in
   the second flags word (0x7507C). Creatures with status bit 0x40 use
   min(1, base delay).

### Think (0x262F7)

1. The AI class flags (0x7507A + 4 × class) choose a movement mode:
   0x40 means it never wanders; otherwise mode = 4, or 5 when the class
   has 0x20 clear.
2. If the creature can't stay where it is (0x2D792 finds a reason to
   move), it tries to step in the facing direction. If that fails it
   tries up to four directions, turning randomly left or right, using
   info+0x18 bits 8-11 to set how random the choice is.
3. Otherwise it **picks a behaviour** (0x26008):
   - When a goal is already being pursued and the creature has noticed
     something (randomly, unless class flag bit 0 is set), it builds goal
     descriptors from its behaviour list (0x25EF0) and runs the planner
     (0x3188A) over the squares around it.
   - The planner returns the best candidate. Its program number becomes
     the new current program, and its target square goes into slot +0x18.
4. **Run the program** (0x261B3 then 0x27CD2) until the queued action is
   set.

### Behaviour lists (per AI class)

Each class has a list of 7-byte entries (pointer at 0x7F584 + 2):

| Byte | Meaning |
|------|---------|
| 0 | Program number |
| 1 | Probability: n > 0 means a 1-in-n chance; n < 0 means a (1 − 1/abs(n)) chance; 0 means always |
| 2-5 | Pointer to goal data in the data object (raw value + 0x70000), passed to the goal builder |
| 6 | Non-zero = more entries follow |

**Choosing the list (0x25962, 0x259CC).** The class's entry in the
pointer table at 0x7518C leads to a list of 6-byte *behaviour sets*: a
16-bit condition mask and a pointer to a behaviour list, ending with a
zero mask (whose list is the default). Before choosing, status bit 3
("badly hurt") is refreshed with probability 1/n, where n is 2 when the
class has flag 0x02 and otherwise depends on the low two bits of the
creature's thing index: the bit is set when HP is under 25% of the base
HP, cleared otherwise. Status bit 1 is then cleared. The chosen set is
the first whose mask equals the status word; failing that, the first
whose bits are all present in it; failing that, the first that shares
any bit with it. Masks with both top bits set (0xC000) call a separate
condition test (0x150AE, not traced). Changing set resets the current
program.

**Goal building (0x25EF0).** Each list entry that passes its probability
roll (the entry for the program already running always passes) becomes
a goal: the goal kind and argument come from bytes 5 and 6 of that
program's current row. The builder 0x26873 dispatches on the goal kind
through a 32-entry table at 0x75248; those routines aren't traced yet,
so the goal data's layout (distance limit and so on) is still open. If
the planner finds nothing, think falls back to program 0x11.

### Programs and the interpreter

There are 63 programs (0 to 62). Each is a pointer into one shared pool
of 7-byte command rows; many programs are just later entry points into
another program's sequence.

| Byte | Meaning |
|------|---------|
| 0 | Opcode letter, `?` (0x3F) to `b` (0x62). A negative value is an inline directive that is run and skipped: −10 sets program variable byte 1 (0 or 1) to byte 2. |
| 1 | Next step if the handler returns "done" (−2) |
| 2 | Next step on any other result |
| 3, 4 | Signed arguments, copied to globals before the handler runs (0x7F56E and 0x7F570) |
| 5 | Bits 0-4: goal kind for the planner. Bits 5-7: flags; 0x40 means "only when on the party's map". |
| 6 | Goal argument |

Next-step values (0x26249):

| Value | Meaning |
|-------|---------|
| ≥ 0 | Jump to that row |
| −2, −3 | End the program (current program becomes −1) |
| −5 | Next row |
| −6 | Previous row |
| −7 | Stay on this row |
| −8 | Skip one row |

Handler results:

| Value | Meaning |
|-------|---------|
| −2 | Done (success) |
| −3 | Failed |
| −4 | In progress; an action was queued and the step stays the same |

Opcodes (dispatch at 0x27CD2, index = letter − 0x3F):

| Letter | Handler | Behaviour |
|--------|---------|-----------|
| `?` | 0x27F28 | Try to step forward (movement test with the current facing) |
| `@` | 0x27F6E | Try turning left or right at random, then the other way; otherwise turn |
| `A` | inline | Queue action 0x13 |
| `B` | 0x28017 | Approach or interact with the target square (0x2EA68); with argument 4, first check possessions through 0x2FF1E |
| `C` | inline | Clear the action (0) |
| `E` | 0x28AAD | Pick up items from the target or the square ahead (mask info+0x0C & 0x77) |
| `F` | 0x28344 | Does the creature in the square ahead carry an item of kind arg 3 (in the quadrant given by arg 4 relative to its facing, or any when 0xFF)? Done if yes, failed if not (0x2FF1E). |
| `G` | 0x28138 | Drop or throw a carried item (needs possessions and info+0x0C bit 3) |
| `H` | 0x28DF0 | Guard check two squares straight ahead: done when a creature without type flag 0x01 stands there or the party does; otherwise queue action 0x1D (wait) and stay. |
| `I` | 0x28574 | Merchant waiting for a customer (see Merchants): done when the creature or party ahead holds coins (kind 0x10) or gems (kind 7); otherwise count down +0x0E and idle (actions 0x1D, 0x1E, 0x1F). |
| `J` | 0x28711 | Merchant haggling over goods placed on the counter (see Merchants). |
| `K` | 0x28E99 | Merchant settling a sale: compares the money on one half of the counter with the price of the goods on the other (see Merchants). |
| `L` | 0x280AC | Set the target to the square ahead and queue action 0x15, or 0x16 when arg = 1 |
| `M` | 0x281F3 | Target the square ahead (target facing = opposite of own) with item kind arg 3 (default 0x3F); if the creature there holds that kind, queue action 0x18 (take it from them) and stay; done if it holds none. |
| `N` | 0x2905A | Possession transfer: first discard kind arg 4 (or the global default at 0x7F7DA; −2 skips this), then if a possession of kind arg 3 (default 0x7F7D8) exists, put it on the creature's own square through 0x2EA68 mode 0x81. Failed when nothing matches. |
| `O` | default (0x29C0F) | Queue the action given by arg 3 (or the global default) |
| `P` | 0x2911C | Creature flag word (active record +0x0A): arg 4 low nibble 0 clears bit arg 3, 1 sets it, other values test it; modes 3 and 4 copy bits from the global switch list at 0x7F7DE (entries of type 0x13 or 0x14). A change queues action 0x33 unless arg 4 has bit 0x10. Done when the bit already had the wanted state. |
| `Q` | 0x2923E | Move one step toward the target square. Returns "done" on arrival. Info+0x16 bits 12-15 give a chance of breaking off (quartered while the creature is afraid). |
| `R` | 0x27E28 | Path toward the planner's target (0x2C404 with move flags 2 or 3 for goal types 8 and 9) |
| `S` | 0x28017 | Same handler as `B`, after clearing the arguments |
| `T` | 0x293A4 | Runs the planner again (0x3188A) |
| `U` | 0x29448 | When on the party's map: work out the direction toward the party (0x1863D) and, if the next square that way isn't a wall, turn or step that way (0x2C005). |
| `V` | 0x27EEB | Queue action 0x27 or 0x28 with a random facing (look around) |
| `W` | 0x294CA | Movement test variant |
| `X` | 0x29544 | Act on the target square (0x2F2CF) |
| `Y` | 0x296D5 | Deal with the creature directly ahead: queue action 0x1D and trade or swap possessions with it (0x286C8 or 0x29598) |
| `Z` | 0x2992D | Queue action 0x23 + min(arg, 2) (attack-style action) when the status bit 0x80 or the argument allows it |
| `[` | 0x299BA | Use, drop or eat the first possession, depending on its type: weapon or clothing byte 2 bit 7, potion byte 3 bit 7, misc item validation. Otherwise, with chance 1/8, wander. |
| `\` | 0x29B16 | Look for a special text thing under the creature. If it holds a creature type, set +0x1E to it and queue action 0x3B (transform); otherwise queue action 0x33 and register an event 0x13. |
| `]` | 0x29BE7 | Queue action 0x3D + arg with mode and argument from globals |
| `^` | default | Same as `O` |
| `` ` `` | 0x29C2D | Act on the target with 0x2CC42 |
| `a` | 0x29C88 | Chance test: done with probability arg 3 percent (`random(100) < arg3`), else failed. |
| `b` | 0x29CAA | Check possessions for either of two item kinds (args 3 and 4) |

### Movement legality (0x2D792)

Each candidate square is classified into a bit, and the move is allowed
only if that bit is in the creature's terrain mask (info+0x0A):

| Square | Class bit |
|--------|-----------|
| Wall | 0x0001 |
| Floor, open trick wall, passable door | 0x0002 |
| Pit, closed (bit 3 clear) | 0x0006 |
| Pit, open and visible | 0x000C (0x800C in one search mode) |
| Pit, open and invisible (bit 0) | 0x8024 |
| Stairs | 0x0100 |
| Door with the "missing" bit pattern (door byte 3 has 4 set and 2 clear) | 0x0200 |
| Door closed (state 4), or open less than the creature's size class (info+0 bits 6-7, or 1 if the door lacks flag 0x20) | 0x4200 |
| Teleporter, inactive, no destination | 0x0402 |
| Teleporter, active, destination allowed for this creature (0x1F9FF) | 0x2000 |
| Teleporter, square bit 3 set and the teleporter's word 2 bits 13-14 equal 1 or 3 | 0x0400 |
| Trick wall, closed (bit 0 clear or set) | 0x0040 or 0x0080 |
| Solid rock | never |

Other checks in the same function:
- The square must not hold another creature group (a group-merging
  exception applies).
- Moves next to the party are handled separately.
- The four positions within a square are tracked (offset tables at
  0x752AC and 0x752AE, chosen by the creature's size, info+0x23).

## Frame events (0x2B75E, code)

The frame's gameplay event depends on the current action:

| Action | Handler | Effect |
|--------|---------|--------|
| 1, 2, 9 | 0x29DE7 | Move to the target square: re-run the movement test, then move the group through the shared move routine 0x4B108 (sensors, pits, teleporters). With info +1 bit 0 it attacks instead (action 0x26). |
| 3, 4 | 0x29F39 | Turn-step |
| 5 | 0x29F6B | TODO |
| 6, 7 | 0x2A357 | Turn: facing becomes +0x1D (when that is a full turn-around, a random side is picked) |
| 8, 0x26 | 0x2A3B9 | Melee attack on the target square (below) |
| 0x0A-0x0F, 0x15-0x18 | various | Item, actuator and possession handling (TODO) |
| 0x13 | 0x2ADB8 | Death or disappearance |
| 0x1A, 0x2B, 0x2C | 0x2ADE1 | TODO |
| 0x27, 0x28 | 0x2A835 | Look around |
| 0x19, 0x29, 0x2A, 0x2D, 0x2E | 0x2ACBC | Put possessions down on the target square |
| 0x2F-0x31 | 0x2B23B | TODO |
| 0x35-0x3A | 0x2A088 | TODO |
| 0x3B, 0x3C | 0x2B35D | Transform |
| 0x3D-0x40 | 0x2B570 | TODO |
| 0x55 | 0x2B724 | Give up (used by think) |

Actions 0x1B-0x25 have no frame event; the merchant and "emote" actions
are animation only. After an event, actions with flag bits 0x03 in the
table at 0x75136 record the tick in slot +4.

**Movement test outcome (0x2D792 tail).** When a step is legal the test
itself chooses the action: 1 to walk straight on (2 when the goal is
within one square), 3 or 4 to turn-step left or right, 9 to back off in
mode 6, or 0 when the action table says so. It records the target square,
direction, facing and mode in the slot. A step that needs a full
turn-around queues a turn (0x2C005) instead.

## Damage and death

- **Creature vs creature (0x31113):**
  1. The hit lands if `rand(32) + attacker.dex ≥ rand(32) + defender.dex`;
     otherwise it misses 75% of the time.
  2. Base damage = `attacker.str + min(attacker.str, rand & 15) − ((defender.def + rand(32)) >> 3)`.
  3. If that is below 2, there's a 50% miss, and otherwise the damage is `rand(4) + 2`.
  4. Then `d += rand(d) + rand(4)`, then `d += rand(d)`, then `d = d/4 + rand(4) + 1`.
  5. Finally, 50% of the time subtract `rand(d/4 + 1)`.
  6. The result goes to 0x24E62 with type 2.
- **Creature attacking the party (0x2A3B9, code):** if the target is the
  party's square, the living champions are collected. The number struck
  is 1 (2 with info +9 bit 0x20); with info flag 0x08 every champion, or a
  random count when flag 0x10 is also set. Victims are picked at random
  (flag 0x10) or by the champion cell facing the attacker. Against a door
  square the creature bashes it with random(1.5 × attack); against another
  creature it uses the creature-vs-creature roll.
- **Blow against a champion (0x18758, code):**
  1. **Dodge:** unless the party sleeps, compare the champion's
     dexterity against the creature's dexterity plus (rnd & 31) minus
     16, combined with a random bit, or pass a luck test (60). Attack
     type 9 doubles the creature's dexterity; type 8 can't be dodged.
  2. **Body part:** chosen from the four nibbles of info +0x1A.
  3. **Strength:** s = attack + min(attack, rnd & 15), minus twice the
     champion's parry level (not for type 8). Below 2, it's a miss half
     the time, otherwise 2 + rand4().
  4. **Damage:** from s through random(s/2) and rand4() terms, roughly
     a quarter of the total plus 1, randomly trimmed by up to half. It
     goes to the champion damage routine 0x4722A with the attack type
     from info +0x1C.
  5. **Poison:** on a hit with info +7 non-zero, a random bit decides
     whether poison (scaled by vitality) is added.
- **Owed damage and death (0x31348, code):** unless the class has flag
  0x04, being hit also signals the creature's home square (actuator call
  0x4BBE4). When owed damage reaches HP, HP is set to 1 and the death
  action 0x13 is queued (0x24DB5); inanimate creatures are removed
  directly (0x30FE3). Class flag 0x800 runs an extra check first.
- **Taking a hit (0x24E62):** adds the damage to slot +0x14.
  - Unless the creature is already afraid, it becomes afraid (status
    bit 2) on a hit over 30, on a hit over 4 with a 1-in-4 chance, or
    when the hit is more than 15% of its maximum HP.
  - Unless its class has flag 0x80, it then has a 50% chance to turn
    toward the party.
  - A percentage roll can set or clear a status bit (`1 << (flags & 0x1F)`).
  - When the pending damage reaches HP, or a disabling status bit is
    applied, the current action is interrupted (0x3064D) and the square
    is refreshed (0x3059D).
- **Missile and spell damage on creatures:** handled at 0x16746 and
  0x1726B. Damage is scaled ×64 / info byte 2, then reduced by info+0x18
  bits 4-7, and halved by info+0x19 bit 0x10.
- **Transforming (0x2B35D, 0x2B570):** the type changes to slot +0x1E.
  HP becomes the new base HP plus rand(base/8 + 1), and the creature is
  re-placed if the new type can't stay on that square. Action 0x3B runs
  this in three stages with sound and particle effects; action 0x3C ends it.

## Animation

- **Action map (15, type, 8, 251):** pairs of (action code, first frame),
  ending with −1.
- **Frame table (15, type, 7, 252):** 4 bytes per frame:
  - **Byte 0:** sound id (bits 0-6); 0x7F means silent.
  - **Byte 1:** the high nibble is non-zero while the sequence continues
    and zero to end it. The low nibble is a branch chance: the next frame
    is taken when (rand & 15) ≤ value, and 0xF means always.
  - **Byte 2:** bits 0-5 are a relative jump to the next frame (0 = stop).
    Bit 6 marks loop or continue. Bit 7 means "run the gameplay event".
  - **Byte 3:** bit 0 enables random position jitter; bit 1 flips the
    image at random; bits 2-3 are random extra ticks; bits 4-7 are the
    base duration in ticks.
- **Drawing descriptors (15, type, 7, 253):** 8 bytes per frame, chosen by
  the same index.
- **Stepping (code):** a sequence is addressed by its start frame (from
  the action map; an action missing from the map uses the value paired
  with the −1 terminator) plus an offset, 0xFFFF meaning "before the
  first frame".
  - *Advance* (0x14F1B) moves by the current frame's byte-1 high nibble
    (stopping if it is 0), then skips frames whose branch roll fails:
    (rnd & 15) ≤ low nibble, or always for 0xF. It reports "playable"
    only if the reached frame's total duration (byte 3 bits 2-3 plus
    bits 4-7) is non-zero.
  - *Next* (0x1501A) follows byte 2's 6-bit jump; a jump of 0 means
    stop. While the slot is armed (+0x21) and frames have byte 2 bit 6
    set, the driver chains through them in zero time, firing their
    events.
- **Timing (0x3023F, code):** the delay is base + random(extra). Status
  bit 0x40 makes it at most 1. Status bit 0x08 cuts it to 75% (at least
  1). While the party sleeps (0x7F234) it doubles, or quadruples off the
  party's map. Creatures off the party's map in some states (bit 15 set,
  bit 1 clear, class second-word flag 0x01 clear) are slowed further;
  that roll isn't fully traced. The counter at 0x7FFEF (champions code:
  raised by action 0x0B) freezes creatures without info flag +1 0x10:
  their step is pushed back 4 ticks, and a death action (0x13) runs at
  triple delay.
- **Inanimate creatures:** their sequence length counts frames up to the
  end marker, and they encode a looping animation state in the creature's
  +0x0C word (0x8000 | arg << 6 | count).

## Merchants, minions and NPCs

What's established so far:
- **Classes:** these creatures use the AI classes whose behaviour
  programs contain the `Y`, `[`, `\`, `]`, `G`, `E`, `P` and `` ` ``
  opcodes (programs 13, 15, 28 and 61, for example).
- **Trading:** `Y` handles the creature directly in front and exchanges
  possessions. `E` and `G` move items between the floor and the
  possession list. `b` tests what the creature is carrying.
- **Item values:** item categories 16 to 21 carry per-item numbers with
  attribute numbers 0 to 4 (present on 90 to 190 items each). One of
  these is probably the trade value.

### Merchants (traced)

A merchant stands facing a counter square. The two halves of that square
(item quadrants, relative to the merchant's facing) act as the "goods"
side and the "money" side. Money means items of kind 0x10 (coins) or 7
(gems), tested with 0x2F636. Everything else on the counter is goods.

- **Value of a pile** (0x286C8 → 0x1C8E5): walks the items in one
  quadrant and adds up either their value or their count, filtered by
  kind.
- **Pricing** (0x15958): the goods' total value is rounded down to an
  amount that can be paid with at most 18 coins, choosing greedily from
  the coin denominations table (values at 0x7F3A8, count at 0x7F3FA,
  largest first).

**`I`, waiting.** If goods (anything that isn't money) lie in the near
quadrant, the merchant counts down +0x0E; at 6 or below it resets the
counter to 9 + rand4() and plays action 0x1F (presumably pushing the item
back). Otherwise it is done as soon as the creature or party ahead holds
coins or gems; if not, it counts down +0x0E and plays 0x1D (idle), or 0x1E
when the counter runs out (counter reset to 5).

**`J`, haggling.** With no stray goods in the near quadrant:

1. offer = value of the coins plus the gems on the money side; with no
   money, the remembered offer (+0x10) is cleared and the step ends.
2. price = the pricing rule above applied to the goods side. If price > 16,
   a random discount is taken off: `price −= random(16) · price / 100`.
3. ratio = offer · 100 / price. The previous offer is kept in +0x10.
4. If offer ≥ price: accept. +0x0E becomes min(offer, undiscounted price),
   action 0x1C, done.
5. If the offer hasn't changed, count down +0x0E. When it runs out and
   ratio > 76, the merchant may give in: with
   `random(max(1, 100 − ratio)) < 5`, combined with a rand4() roll, it
   plays 0x20, otherwise 0x1B. While counting, it plays 0x1D.
6. If the offer went up: with rand4() ≠ 0 and ratio ≤ 76 + (rnd & 7) it
   keeps waiting (0x1D); otherwise it resets +0x0C and makes the same
   0x20 / 0x1B decision.

Action meanings (tentative, from how they are used): 0x1B refuse, 0x1C
accept or sell, 0x1D wait, 0x1E prompt, 0x1F reject the item offered,
0x20 accept grudgingly.

**`K`, settling.** If goods sit in the quadrant that should hold money,
the merchant plays 0x1F and the step fails. Otherwise:

1. price = the goods value, rounded as above, plus any gems on the goods
   side.
2. paid = the coins plus gems on the money side.
3. If paid < (gems on the goods side + the amount still owed in +0x0C),
   it refuses (0x1B).
4. Otherwise, if paid differs from the last amount (+0x0E), it plays
   0x1C and remembers it in +0x10. +0x0C becomes max(0, paid − price),
   the change still due. Done.

**Leads not yet traced:** `Y` (0x296D5) moves the goods between the
counter and the merchant's possessions (0x29598).

Minion summoning lives in the spell code (docs/07).

### The planner (0x3188A, structure)

The planner searches outward from a start square for the first square that
satisfies any goal in a goal list. It is called by think (0x26008), by
opcode `T` and by two other AI helpers.

- **Inputs:**
  - the start x and y;
  - the goal kind of the calling step (byte 5 bits 0-4);
  - a list of goal records, each with a distance limit (byte 0), target
    fields, the goal kind (byte 7), arguments (+8, +10) and pass flags
    (+0x10: bit 0 is tested in the first pass, bit 1 in the second).
- **Search:** breadth-first, distance-limited. It uses scratch grids
  allocated per map (128 bytes per map, plus a 1 KB queue) and checks each
  step with the movement test 0x2D792, so it follows the same terrain rules
  as real movement. It crosses stairs and pits into other maps
  (0x2BEAF, 0x2BBAD), and it skips goals whose distance limit has
  been passed.
- **Results:** a match writes the target (map, x, y) and the distance
  back into the goal record (bytes 2-6) and returns the goal's index.
- **Goal kinds matched in the final switch:**

| Kind | Satisfied when |
|------|----------------|
| 2 | The square is the party's. Argument mode 1 also requires the party to face one of the directions in a 4-bit mask; mode 3 requires the next square in the goal direction to be an open, real pit. |
| 3 | The square is the party's. |
| 8, 9 | A thing search (0x2C0A2) finds a matching item or object at the square, filtered by the item mask at 0x7F574. Kind 9 is a variant flag. |
| 0x0F, 0x11 | The creature can interact with the square (0x2EA68 mode 0), such as an actuator or an item on the floor. |
| 0x12 | A creature of type +8 stands there. Mode 1 matches any such creature. Otherwise the square ahead of it must hold the party or a creature without flag 0x01. |
| 0x13-0x1A | Other branches exist but are not traced. |

- **Scoring:** there is no separate score. Since the search is
  breadth-first, the first match is the nearest one. Ties are broken by
  the order of the goal list and the order squares are visited. The
  planner also draws random numbers (0x1C6A1, 0x1C6B7), probably to vary
  that order; not confirmed. The flags at 0x7F572 (0x100 / 0x110 /
  0x108) change which passes run.

## Implementation status (crates/dm2-engine/src/creatures)

- **Data:** read from the user's SKULL.EXE and GRAPHICS.DAT at runtime
  (`data.rs`), never embedded.
- **Done:** the slot pool and activation (on arriving on a map, on
  spawn and when hit); the step event; animation stepping and timing;
  think with behaviour-set selection, behaviour picking, the planner
  (BFS on one map) and the program interpreter; movement legality; moves
  through the shared move routine; turns; melee attacks on the party,
  doors and other creatures; death; transforms; merchant pricing and the
  `I`/`J`/`K` rules (`merchant.rs`, with the money classifier still a
  stand-in).
- **Partial:** goal kinds beyond 0-3, 8, 9, 0x0F, 0x11 and 0x12; the
  opcodes `F`, `J`, `K` (on real counters), `M`, `N`, `W`, `Y`, `[`,
  `\`, `]` fail so that programs fall through; groups occupy whole
  squares (no in-square positions or group merging); the planner doesn't
  cross maps.

## Open questions

- Planner goal kinds 0x13-0x1A and the exact visiting order of the search.
- The `Y` trade handler (0x296D5) in detail.
- The full list of action codes. Known ones: 6 and 7 (turn), 0x11, 0x13,
  0x15 and 0x16, 0x1D, 0x23 to 0x25, 0x27 and 0x28, 0x32 to 0x34 (wait),
  0x3B and 0x3C (transform), 0x3D and up, and 0x55.
- What the AI class flag bits mean beyond 0x01, 0x08, 0x10, 0x20, 0x40,
  0x80 and 0x410.
- What the frame events in 0x2B75E do.
