# Champions

How the party's champions are stored and how their numbers change over
time. Addresses are SKULL.EXE function entry points (Ghidra `FUN_` names);
everything was checked against the decompiler listing, and the asm where
the listing was unclear. Formulas follow the code; constants that are
data rather than formula belong in SKULL.EXE and can be printed with
`tools/exe_tables.py`.

The overall design is a close descendant of the original Dungeon Master's
champion system (same four classes, 16 hidden sub-skills, stat pairs,
luck rolls), so names below follow that game where the code agrees.

## Random numbers (0x1C6A1 and friends)

All game randomness goes through one 32-bit linear congruential generator
held at 0x70518:

```
seed = seed * 0xBB40E62D + 11        (mod 2^32)
rnd()      = seed >> 8               0x1C6A1
random(n)  = (rnd() & 0xFFFF) % n    0x1C6B7   (n = 0 returns 0)
randbit()  = rnd() & 1               0x1C6DC
rand4()    = rnd() & 3               0x1C6F6
```

Helpers used everywhere: 0x1C67E = min (signed 16-bit), 0x1C687 = max,
0x1C690 = clamp(lo, x, hi), 0x1C710 = scaled product `(a * b) >> c`
(called as `(x, 6, y)`, i.e. `x * y / 64`).

## Party globals

| Address | Meaning |
|---------|---------|
| 0x7F276 | Number of champions in the party (0-4) |
| 0x7FBD0 | Champion array, 4 × 0x107 (263) bytes |
| 0x7F284 | 1-based index of a champion currently being recruited (excluded from damage, regen and poison until confirmed) |
| 0x7F234 | Party asleep. Skill levels read as 1, dexterity is halved, regen rates double |
| 0x7F23C | When set, champions take no damage (cheat/scripted invulnerability) |
| 0x7F22C | Game time (ticks) |
| 0x7F19C | Time of the party's last move; used for resting bonuses |
| 0x716A0 | Time a creature last attacked the party; gates combat experience |
| 0x7F26A/0x7F26E | Party map X / Y; 0x7F252 (high byte) party facing |
| 0x7FBAC | Pending damage per champion (applied by the screen update, not immediately) |
| 0x7FBA4 | Pending wound bits per champion |
| 0x7FFF8 | Per-champion, per-class level-up counters (4 bytes each) |

## Champion record (0x107 bytes)

| Offset | Size | Meaning |
|--------|------|---------|
| 0x00 | 8 | Name (up to 7 characters, NUL-padded) |
| 0x08 | 20 | Title |
| 0x1C | 1 | Facing direction (0-3) |
| 0x1D | 1 | Cell (formation slot) within the party square |
| 0x1E | 1 | Number of runes entered so far |
| 0x1F | 1 | Number of pending poison events (non-zero = poisoned) |
| 0x20, 0x21 | 1 each | Per-hand action state (0xFF idle; 1 means the hand's defensive bonus is active) |
| 0x22 | 4 | Rune buffer: symbol bytes (0x60 + row×6 + column), NUL-terminated |
| 0x28 | 1 | Direction at the time of recruiting |
| 0x29-0x2C | | Action timers and per-hand cooldown bookkeeping (see 07) |
| 0x2E | 2 | Timer event index (0xFFFF = none) |
| 0x32 | 2 | Redraw flags: 0x08 stats bar, 0x10/0x20 panels, 0x40 dead; 0x400-0x7C00 select panel parts |
| 0x34 | 2 | Wound bits (bit 0 ready hand, 1 action hand, 2 head, 3 torso, 4 legs, 5 feet) |
| 0x36 / 0x38 | 2 / 2 | Health, maximum health (cap 999) |
| 0x3A / 0x3C | 2 / 2 | Stamina, maximum stamina (cap 9999) |
| 0x3E / 0x40 | 2 / 2 | Mana, maximum mana (cap 900) |
| 0x42, 0x43 | 1 each | Signed defence bonus per hand while that hand's action is active (from the action's TA code) |
| 0x44 | 2 | Food (starts 1500-1755, eating caps at 2048, floor −1024) |
| 0x46 | 2 | Water (same ranges) |
| 0x48 | 2 | Poison pool still to be applied (cap 3072) |
| 0x4A | 14 | Seven stats, each a byte pair (current, maximum): luck, strength, dexterity, wisdom, vitality, anti-magic, anti-fire |
| 0x5F | 80 | Experience, 20 × u32: classes 0-3 then sub-skills 4-19 |
| 0xAF | 20 | Signed temporary level modifiers per skill (from items/spells) |
| 0xC3 | 60 | Inventory: 30 thing references (0xFFFF empty); slot 0 ready hand, slot 1 action hand |
| 0x101 | 1 | Champion (portrait) number in GRAPHICS.DAT category 22 |
| 0x102 | 1 | What 0x103 holds: 0 fire shield, 1 spell shield, 3-6 a temporary boost of stat (value − 2), i.e. strength, dexterity, wisdom, vitality |
| 0x103 | 2 | Personal shield strength |

Unlisted bytes are touched by only one or two functions and still need
labels.

## Skills and levels

Skills 0-3 are the classes fighter, ninja, priest and wizard (the order
the interface text uses). Skills 4-19 are four hidden sub-skills per
class, class = (skill − 4) / 4. The action strings agree with the
original game's numbering: 4 swing, 5 thrust, 6 club, 7 parry, 8 steal,
9 fight, 10 throw, 11 shoot, 12 identify, 13 heal, 14 influence,
15 defend, 16 fire, 17 air, 18 earth, 19 water.

**Level** (0x46241, `skill_level(champion, skill, with_modifiers)`):

```
if party asleep: return 1
xp = exp[skill]
if skill >= 4:
    class = (skill - 4) / 4
    k = with_modifiers ? modifier[class] + 1 : 1
    xp = (xp + k * exp[class]) >> 1
level = 1; while xp > 511: xp >>= 1; level += 1
if with_modifiers: level = max(1, level + modifier[skill])
```

So level 2 needs 512 experience, and each further level doubles that.
Rank titles for display are interface text (category 7, index 0, sub
4 + level − 1); there are 15.

**Total party level** (0x461E8): the same halving rule applied to the sum
of all champions' class experience. Used to scale some encounters.

## Gaining experience (0x462FA)

`add_experience(champion, skill, amount)`:

1. Fighter and ninja sub-skills (4-11) earn half if no creature has
   attacked the party in the last 150 ticks.
2. Multiply by the current map's experience multiplier (descriptor word
   +12, bits 12-15) when that is non-zero. This answers one of the open
   DUNGEON.DAT questions.
3. Sub-skills earn double if a creature attacked within the last 40 ticks.
4. Add to the skill and, for a sub-skill, also to its class.
5. For every class level gained (A = the new class level), roll:

```
b  = randbit();  r = randbit() + 1
if class != priest: vitality.max += A & randbit()
anti_fire.max += randbit() & ~A
fighter: strength.max += r;  dexterity.max += b;  hp_gain = 3A; st_base = max_stamina / 16
ninja:   strength.max += b;  dexterity.max += r;  hp_gain = 2A; st_base = max_stamina / 21
priest:  wisdom.max += b;    max_mana += A;       hp_gain = A + (A+1)/2; st_base = max_stamina / 25
wizard:  wisdom.max += r;    max_mana += A + A/2; hp_gain = A;  st_base = max_stamina / 32
priest, wizard: max_mana += min(rand4(), A - 1) (cap 900); anti_magic.max += rand4()
max_health  += hp_gain + random(hp_gain/2 + 1)      (cap 999)
max_stamina += st_base + random(st_base/2 + 1)      (cap 9999)
```

The level-up message is text (1, 0, 6 + class).

## Recruiting (0x49242)

Called with the portrait number when a champion is accepted:

- The record is zeroed. Facing = party facing; the cell is the first free
  cell clockwise from the party's facing.
- The name is the champion text (22, n, 24) up to the first space (at most
  7 characters); the rest of that string becomes the title.
- All 30 inventory slots are emptied.
- **Starting-stats table**, type 8 entry (22, n, 8, 0), 26 little-endian
  words (52 bytes):

  | Words | Meaning |
  |-------|---------|
  | 0 | Health (current and maximum) |
  | 1 | Stamina |
  | 2 | Mana |
  | 3-9 | Luck, strength, dexterity, wisdom, vitality, anti-magic, anti-fire; each becomes max(30, value) for current and maximum |
  | 10-25 | Sub-skills 4-19: starting experience is 0 if the word is 0, else 64 << word |

  Class experience is then the sum of its four sub-skills.
- Food and water each start at 1500 + (rnd() & 0xFF).

## Per-tick regeneration (0x47CC3)

Runs once per game tick for each living champion who is not being
recruited. A shared counter G (0x7FFF4) steps by +56 and wraps past 128,
which acts as a cheap varying threshold.

**Mana**, when below maximum:

```
L = level(wizard) + level(priest)           (with modifiers)
if G < wisdom + L:
    gain = max_mana / 40 + 1   (×2 asleep)
    stamina_loss(max(7, 16 - L) * gain)
    mana += min(gain, max_mana - mana)
```

Mana above maximum (from potions) decays by 1 per tick.

**Stamina, food and water.** The more tired the champion, the more the
loop runs: k = 4, plus 2 for every halving of maximum stamina that is
still above current stamina. Base recovery R = clamp(1, (max_stamina >> 8)
− 1, 6), +1 if the party has not moved for 80 ticks and another +1 after
250, ×2 asleep. Then repeat until k reaches 0 or stamina is full:

```
food:  if food < -512: if k < 5: loss += R; food -= 2
       else: if food >= 0: loss -= R;  food -= (k < 5 ? 2 : k/2)
water: if water < -512: if k < 5: loss += R; water -= 1
       else: if water >= 0: loss -= R; water -= (k < 5 ? 1 : k/4)
k -= 1
```

The net `loss` goes through stamina loss (negative means gain). Food and
water are clamped to −1024.

**Health:** if health is below maximum and stamina is at least a quarter
of maximum, and G < vitality + 12, gain (max_health / 128) + 1 (×2
asleep), capped at maximum.

**Stats:** every 256 ticks (64 asleep), each current stat below its
maximum rises by 1; one above its maximum falls by current / maximum.

## Stamina loss (0x47707)

Subtracts from stamina. If stamina drops to 0 or below, it is set to 0
and the champion takes half the overflow as unblockable damage. Stamina
is capped at its maximum. Losses over 9 flag the stats bar for redraw.

## Derived values

- **Luck roll** (0x46786, `lucky(champion, threshold)`): with chance 1/2,
  succeed if random(100) > threshold. Otherwise roll random(2 × current
  luck); success if that is above the threshold. Current luck then moves
  by −2 on success or +2 on failure, clamped to 10..min(220, maximum
  luck).
- **Dexterity** (0x46968): (rnd & 7 + current dexterity) / 2, reduced in
  proportion to load over maximum load, at least 2, halved asleep, then
  clamped between 1 + rand(8) and 100 − rand(8).
- **Strength for an action** (0x46A19, `(champion, hand, skill)`):
  current strength + (rnd & 15) + weapon weight − 12, with extra
  penalties when the weapon weighs more than maximum load / 16. For
  skill ≥ 0, add 2 × that skill's level plus the weapon's attribute 8
  (melee skills 0, 4-7, 9) or attribute 9 (throw and shoot skills 1, 10,
  11; for skill 11 only if attribute 5 bit 15 marks the item as a
  launcher). Then a stamina adjustment (0x4667A), halved if that hand is
  wounded, and the result is clamp(0, value / 2, 100).
- **Maximum load** (0x46824), in tenths of a kg:
  1. s = effective strength (0x466AB, below).
  2. L = 8·s + 100.
  3. If stamina is below half its maximum: L = L/2 + stamina·(L/2) / (max
     stamina / 2) (0x4667A).
  4. If any wound bit is set: subtract L/4 when the legs are wounded (bit
     4), otherwise L/8.
  5. Round up to a multiple of 10: `(L + 9) − (L + 9) % 10`.
- **Effective stat** (0x466AB): the current (or maximum) byte of the stat
  pair. For the current value, if a temporary boost is active (+0x103
  non-zero and +0x102 between 3 and 6, boosting stat +0x102 − 2, i.e.
  strength to wisdom), add `random((min(+0x103, 100) · value >> 7) + 1) +
  4`. Then add the signed per-stat modifier at +0x58 + stat and clamp the
  result to 10-220 (0x1C690).
- **Maximum load** (above) and **current load** (0x47BC5) drive the load
  display: load above maximum shows in one colour, above 5/8 of maximum
  in another.
- **Stat-adjusted value** (0x46745, `(champion, stat, value)`): scales a
  value down by a resistance stat (used for vitality, anti-magic and
  anti-fire). Exact curve TODO.

## Food and drink (0x39C3F, command 0x10)

Eating adds the item's food value to food, capped at 2048. Drinking water
adds 800 to water, also capped at 2048. Potions apply their effect (see
07). The consumed item is removed.

## Poison (0x474FC, event 0x4B)

`poison(champion, amount)`:

- Immediate unblockable damage max(1, (amount + 30) / 64).
- Then, if amount − 1 is non-zero, add it to the poison pool at +0x48
  (capped at 3072), increment the poison count at +0x1F, and schedule
  timeline event 75 (0x4B) for this champion 36 ticks later. Each event
  repeats the process with the remaining pool, so poison wears off
  geometrically.
- Cure (0x475D3, `(champion, strength)`) removes pool and events; death
  calls it with 10000.

## Sleep

Resting sets 0x7F234. While asleep, skill levels read as 1, dexterity is
halved, all regeneration doubles, and stats recover every 64 ticks.
Taking damage wakes the party (0x46D9C).

## Death (0x46ECA)

When health reaches 0:

- Health is set to 0 and the champion is flagged dead; runes and timers
  are cleared; any open inventory or leader panels belonging to them
  close.
- Possessions are dropped on the party square (0x46DC9).
- A misc-item thing (type 10) with bit 7 set and the champion index in
  bits 14-15 is created at the champion's cell: the champion's bones.
- Poison is cured.
- If no champion is left alive the game ends (0x7F24C = 1, 0x209E0),
  otherwise leadership passes to the first living champion.

**Resurrection** (traced):

1. **Trigger** (floor sensor code, 0x4CDCC). The bones item (category 21,
   index 0; its charge value is the champion's index) is dropped on a
   square whose thing list holds an altar marker (tested by 0x1FDF0). If
   the champion index is valid (below the party size at 0x7F276), event
   0x0D is scheduled for the next tick. The event carries the champion
   index (+5), x and y (+6, +7), cell (+8) and stage (+9, starting at 2).
2. **Event 0x0D** (0x59050) runs in three stages, counting +9 down:
   - **Stage 2:** spawn the rebirth effect 0xFFE4 at the altar cell
     (0x16746) and reschedule 5 ticks later.
   - **Stage 1:** find the matching bones in that cell, take them off the
     square and delete them; reschedule 1 tick later.
   - **Stage 0:** revive the champion (0x49CBB).
3. **Revive** (0x49CBB):
   - reset the record through 0x46E4D and clear +0xFF;
   - empty all 30 inventory slots (the belongings were dropped at death);
   - max health becomes `max(25, max − max/64 − 1)`, a permanent loss of
     about 1.6% plus 1, and current health becomes half the new maximum;
   - set flag 0x40 in +0x33, clear any boost or shield (+0x102, +0x103);
   - refresh the portraits and panels.

## Open questions

- Remaining record bytes (0x29-0x2C, 0xCF, 0xD9, 0xDB, 0xFF, 0x105).
- The curve inside 0x46745 (stat adjustment).
- What 0x46E4D resets on revival, and the altar marker test 0x1FDF0.
