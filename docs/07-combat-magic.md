# Combat and magic

Actions, melee, damage to champions, missiles, explosions and spells.
Companion to `06-champions.md` (record layout, random numbers, levels).
Tables that are data rather than formula stay in SKULL.EXE; print them
from your own copy with `tools/exe_tables.py runes|spells|codes`.

## Action strings

Every weapon, item class and champion bare hand has up to four actions,
stored as text entries (category, index, sub 8-11) such as the made-up
`CHOP:CM4SK4BZ6TR3EX5PB40DM20`. The name before the colon is the menu
label; the rest is a list of two-letter codes, each followed by a signed
decimal number.

**Lookup.** 0x3F7FA `(cat, idx, sub, code)` fetches the text (0x3A921)
and scans it for the code (0x3F791), returning its number or 0 if absent.
The code is a slot number into a table of two-letter names at 0x7590E.
0x3F8FF is the same lookup using the "current action" key held in
0x7FB7B/0x7FB7C/0x7FB7A, which is set when an action is chosen.

| Slot | Code | Meaning (from how the executor uses it) |
|------|------|-----------------------------------------|
| 0 | SK | Skill the action uses and trains (numbering in 06) |
| 1 | LV | Minimum level in SK for the action to appear in the menu (0x3F9F5) |
| 2 | CM | Command: which branch of the executor runs (table below) |
| 3 | BZ | Busy time: ticks before that hand can act again. Halved when the action fails |
| 4 | TR | Tiredness: stamina cost, plus randbit() |
| 5 | ST | Strength or amount: a command-specific magnitude (spell power, effect duration, missile energy) |
| 6 | PA | Parameter: command-specific (for example the missile or explosion type, as 0xFF80 + PA) |
| 7 | TA | Temporary armour: signed defence bonus for this hand while the action is active (stored at +0x42/+0x43) |
| 8 | NC | Charges used. The menu hides the action when the item lacks the charges |
| 9 | EX | Experience awarded in SK on success (halved, or quartered, on failure) |
| 10 | PB | Hit probability for melee |
| 11 | DM | Damage factor for melee |
| 12 | MS | Present in the table, never read by the executor. Possibly unused |
| 13 | SD | Sound sub-index to play from the item's sound set |
| 14 | RP | Repeat timer: when non-zero, written to a per-champion, per-hand table at 0x7FAD8 after a successful action (auto-repeat or recharge display; TODO) |
| 15 | HN | Non-zero: the attack can hit non-material creatures |
| 16 | AT | Attack type (numbering below); creatures with the "resistant" flag take reduced damage unless the type is 1 |
| 17 | WH | Not read by the executor; probably consumed elsewhere through the generic lookup (TODO) |

## Action executor (0x414A5)

`do_action(champion, hand_and_action)` resolves the chosen action:

1. Reads CM, SD, BZ, SK, TR, EX, ST, AT and TA up front. TA becomes that
   hand's defence bonus.
2. Works out the square in front of the champion (party position plus the
   direction delta for the champion's facing) and what stands there.
3. Switches on CM (summary below).
4. Afterwards, while the champion is alive and the action was not
   cancelled: applies BZ as the hand's busy time (0x40A0A), takes TR as
   stamina loss (0x47707), adds EX experience to SK (0x462FA), and, on
   success, records RP. A sound plays unless suppressed.

| CM | Effect |
|----|--------|
| 2 | Invisibility for max(32, ST) ticks (timeline event 0x47) |
| 3 | Cast a projectile from the item: missile type 0xFF80 + PA, energy ST. Mana cost 7 − min(6, level(SK)); if mana is short, energy scales down in proportion. On failure EX is halved |
| 4, 8 | Melee (below). 8 also bashes a door ahead when the door is in the right state |
| 5 | Influence-type action: level(14) + ST against the target (0x410F7) |
| 6, 0x26, 0x27 | Light-level effects of strength ST (0x412E1, also used by the light spells) |
| 7 | Explosion type 0x0E on the party's square with strength max(2, ST) |
| 9 | Adds 32 × ST to a party counter at 0x7FFF0 (cap 255) |
| 10 | Climb down: works when the square ahead is a pit and nothing blocks it |
| 0x0B | Adds ST to a party counter at 0x7FFEF (cap 200) |
| 0x0C-0x0F | Party shield effect of strength 4 × max(32, ST), with no mana cost (0x45815) |
| 0x10 | Eat or drink the item in hand (0x39C3F) |
| 0x11 | Use a container's contents (0x3FC6D) |
| 0x20 | Shoot: needs a matching launcher and ammunition in the other hand (0x408A8); launches with strength from skill 11 and the weapons' attributes 9, 10 and 12 |
| 0x21-0x23 | Party shield effect of strength 3 × max(32, ST), costing 4 mana |
| 0x24 | Heal: while health is below maximum and mana remains, spend 2 mana per step to restore min(10, level(13))-sized steps |
| 0x2A | Steal (0x478A1); easier when attacking from the creature's side or back |
| 0x2C | Use the item on the square ahead (0x1FAC8) |
| 0x2D-0x2F | Minion control: attach to an existing minion or create one (types 0x30, 0x33) |
| 0x30 | Dismiss a minion (0x30B3D) |
| 0x31-0x33 | Summon a minion of type 0x31, 0x34 or 0x35 in front of the party, power ST / 8 |
| 0x36 | Special item action (0x4BFD4); shows a failure message when it does nothing |

## Melee (0x41171 then 0x18A57)

0x41171 checks that a creature stands ahead. A champion in a back cell can
only strike if the front cell on that side is empty; otherwise "can't
reach" ends the action. With PA = 1, the attack only works on creatures
whose attribute bit 0x20 is set. It then calls the core.

**Core** `melee_damage(champion, creature, PB | HN<<15, DM, SK, AT)`:

```
D2 = 2 * map_experience_multiplier               (map descriptor +12 bits 12-15)
info = creature type info
if info.dexterity_byte(+8) == 0xFF: miss                      (cannot be hit)
if creature is non-material (info[0] & 0x20) and not HN: miss
threshold = ((rnd & 31) + info.dexterity + D2 + 2 * light_term - 16) >> 1
hit = dexterity(champion) > threshold
      or rand4() == 0
      or lucky(champion, 75 - PB)
if not hit: stamina_loss(randbit() + 2); return 0

s = strength(champion, hand, SK)
if s > 0:
    s += random(s/2 + 1)
    d = (DM * s) >> 5
    d += (rnd & 31) - (info.armour(+2) + (rnd & 31) + D2)
if s == 0 or d <= 1:                       # weak hit, may still graze
    if rand4() == 0: miss as above
    x = rand4()+1 (2..4 here) ; d += rnd & 15
    if d > 0 or randbit(): x += rand4(); if rand4() == 0: x += max(0, d + (rnd & 15))
    d = x
d >>= 1
d += random(d) + rand4()
d += random(d)
d >>= 2
d += rand4() + 1
if (rnd & 63) < level(SK, with modifiers): d += d + 10        (critical)
if weapon attribute 13 and (rnd & 31) < d: d += slayer_bonus(creature, attr13)   (0x31574)
add_experience(SK, ((info.word(+0x16) >> 8 & 15) * d >> 4) + 3)
stamina_loss(rand4() + 4)
if info[0x19] & 0x10 and AT != 1: d >>= randbit() + 1
damage_creature(creature, d)                                   (0x24E62, flags 0x6002)
```

`light_term` is the word at 0x7F282 and needs confirming. All randomness
is the generator in 06.

## Attack types

Used by champion damage, missiles and actions (AT):

| Type | Kind | Defence |
|------|------|---------|
| 0 | Unblockable | Added straight to pending damage |
| 1 | Fire | Anti-fire stat, then the personal fire shield, then armour like the blunt types |
| 2 | Self (e.g. stamina overflow) | Defence halved, plus the ninja class level (tentative) |
| 3 | Blunt (default for missiles) | Armour |
| 4 | Sharp | Armour, using the sharp-resistance variant of each piece |
| 5 | Magic | Anti-magic stat, then the personal spell shield; skips the armour scaling |
| 6 | Psychic | Scaled by 115 − wisdom; none at wisdom ≥ 115 |
| 7 | Lightning | Armour |
| 8 | Like 2 | Like 2 |

## Damage to champions (0x4722A)

`damage_champion(champion, amount, body_parts, attack_type)`. Nothing
happens if the champion is dead or being recruited, the party is
invulnerable, or amount < 1.

1. Type 0: add to pending damage and stop.
2. Defence = average over the selected body parts (bits 0-5: ready hand,
   action hand, head, torso, legs, feet) of that slot's armour value
   (0x46BDC, with the sharp flag for type 4).

   **Armour value of one part** (0x46BDC), 0..100:
   - Each hand item whose attribute 0x0B has bit 15 set (a shield) adds
     (item armour + strength(champion, hand, parry)) × w[part], shifted
     right 4 bits for the shield's own hand and 5 for the other. w is a
     6-byte per-part table at 0x759A6 in SKULL.EXE.
   - Item armour is the low byte of attribute 0x0B; against sharp attacks
     it is scaled by ((high byte & 7) + 4) / 8 (0x46BAD).
   - Base: random(vitality / 8 + 1), halved against sharp attacks; plus
     the armour bonus when +0x102 is 2; plus both hands' TA bytes.
   - Parts other than the hands add the armour of the item worn there.
   - A wounded part loses rand4() + 8; a sleeping party halves the total.
   - Result: clamp(0, total / 2, 100).
   - Tentative: the shield term adds the parry strength to the item armour;
     the decompiler output is garbled at that point.
3. **Hand defence:** for each hand whose action is active, add its TA
   bonus. If the total is positive and (rnd & 15) < level(7, parry) +
   total/8, the parry works: a blockable hit loses that much, and is
   ignored entirely if that brings it to 0; then defence += total / 4.
4. Per type, as in the table above. For the armour types the result is
   amount × (130 − defence) / 64.
5. **Wounds:** v = vitality-adjusted((rnd & 127) + 10). While the damage
   exceeds v, set a random body-part bit (1 << rand8) & body_parts, and
   double v.
6. A sleeping party wakes up.
7. Add to pending damage (0x7FBAC) and pending wounds (0x7FBA4); the
   screen update applies them and checks for death.

## Missiles

**Creation** (0x16457) makes a missile thing (type 14, 8 bytes):

| Offset | Meaning |
|--------|---------|
| +2 | What flies: an item reference, or an explosion type 0xFF80-0xFFBF |
| +4 | Kinetic energy (≤ 255) |
| +5 | Attack (≤ 255) |
| +6 | Timeline event index |

It schedules event 29 (0x1D), or 30 when 0x7F1A4 is set, for the next
tick. The event payload packs x (5 bits), y (5 bits), direction (2 bits,
at bit 10) and step energy (4 bits, at bit 12). If no missile slot is
free, a thrown item just drops on the square.

**Champion launches** (0x47773): from the party square, on the
champion's side of the formation, facing the champion's direction.
Energy, attack and step energy are each capped at 255.

**Projectile spells** (via 0x47813): if mana is enough, pay it; step
energy = 10 − min(6, max_mana / 32); if the energy is below 4 × that, add
4 to the energy and set step = energy / 4. Then launch.

**Impact damage** (0x16D72) sets an attack type and a side value (poison
for champions, slayer bonus for creatures), then returns damage. E is the
kinetic energy and A the attack byte.

- 0xFF80 fireball: type 1 (fire), base 10 + (rnd & 15) + (rnd & 31)-style roll.
- 0xFF82 lightning: type 7, base × 16 + E.
- 0xFF81: carries poison 10 + (rnd & 15), with a blunt base roll.
- 0xFF86 poison bolt: type 5, poison = E, damage E/8 + 1.
- Other explosion types: type 5, no direct damage.
- **Items:** with throwing attribute 9 non-zero, type 4 (sharp); base =
  (attr9 + E/2) × k² / 128 where k = (A >> 4) + 3. The side value is
  the item's attribute 13; if (rnd & 127) ≥ E it is reduced by
  random(value/2 + 1). Add weight + rand4(); doubled when (rnd & 0x1FF) < A.
- **Final roll** for all: d = base + random(((base + E)/16 + 1)/2 + 1) +
  rand4(); then d = max(d, 2 × (d − (32 − A/8))); then d = min(d, 2E).

**Hitting the party:** the champion in the struck cell takes
`damage_champion(d, head|torso, type)`. The parry flag is set when that
champion faces into the missile. Poison applies with a 7-in-8 chance,
doubled for 0xFF86.

**Hitting a creature:** d' = (d × 64) / armour(+2) + slayer bonus,
quartered for "resistant" creatures unless type 1. Non-material creatures
are only hit by 0xFF83. Creature flag bit 0 makes missiles glance off or
pass, depending on further flags.

Thrown potions turn into explosions on impact (potion type 3 becomes
0xFF87, type 0x13 becomes 0xFF80).

## Explosions (0x16746)

`explode(type, strength, x, y)` creates an explosion thing (type 15) on
the square. For damaging types (0xFF80, 0xFF82, 0xFFB0, 0xFFB1 and the
spell explosion 0xFF8E) it:

- Damages every creature on the square: base = (E/2 + 1) + random(E/2 + 1)
  + 1, minus random(2r + 1) where r is the creature's resistance nibble
  (type info word +0x18, bits 4-7; 15 means immune), quartered for
  non-material creatures.
- Damages the party with type 1 on all body parts (0x4766B, mask 0x3F)
  when the party is on the square.

0xFF84 and 0xFF8D act on doors ahead (open or break, depending on the
door's flags). 0xFFE4 is the first step of rebirth. 0xFFA8 is a "fizzle"
puff, used when a spell or action cannot complete.

## Magic

### Runes (0x42924, 0x429F5)

Four rows of six runes: power, element, form, alignment. A rune symbol is
0x60 + row × 6 + column. Each champion holds up to four symbols at
+0x22, with the count at +0x1E.

Entering a rune costs mana:

```
cost = rune_cost[row][column]                      (table at 0x757DC)
if this is not the first rune: cost = cost * power_mult[first rune] / 8   (0x757F4)
if mana >= cost: pay it and append the rune; else nothing happens
```

Each power level multiplies the rest of the spell by 1, 1.5, 2, 2.5, 3
or 3.5 (from the table). 0x429F5 removes the last rune without refunding
it.

### Spell table (0x757FE, 33 × 8 bytes)

| Offset | Size | Meaning |
|--------|------|---------|
| 0 | u32 | Rune key: element/form/alignment symbols in bytes 2, 1, 0; byte 3 is a required power rune, or 0 for any power |
| 4 | u8 | Base level |
| 5 | u8 | Skill used and trained (13-19 sub-skills, or 2/3 = whole class) |
| 6 | u16 | Bits 0-3 kind (1 potion, 2 projectile, 3 other, 4 summon), bits 4-9 type, bits 10-15 duration factor |

The spells use 1 to 3 runes after the power rune and none requires a
specific power. Lookup is 0x421FD.

### Casting (0x428A2, then 0x422F5)

```
power = first rune column + 1                       (1-6)
required = base + power
xp = (rnd & 7) + 16*required + 8*base*(power-1) + required^2
cooldown = duration * (power + 18) / 24
deficit = required - level(skill, with modifiers)
while deficit > 0:
    if (rnd & 127) > min(wisdom + 15, 115):
        add_experience(skill, xp >> deficit); fail ("needs practice")
    deficit -= 1
```

On success, by kind:

- **Potion** (1): needs an empty flask in a hand, otherwise fails with
  "need flask" and the runes stay. The flask becomes potion `type` with
  strength 40 × power + random(16).
- **Projectile** (2): energy = clamp(21, (2L + 4) × (power + 2), 255),
  where L = level (doubled for type 4); missile 0xFF80 + type at no
  further mana cost.
- **Other** (3), by type:
  With q = power + 1 and s = 4q:
  - 0, 1, 5: light effects 0x27, 6 and 0x26 of strength 36q (0x412E1).
  - 3: invisibility: schedules event 0x47 32q ticks ahead (and counts
    active invisibility at 0x7FFEE).
  - Party effects (0x45815), as type → (kind, strength): 2 → (1, s² + 100),
    8 → (0, s² + 100), 4 → (2, s²), 6 → (5, (s+3)²), 7 → (4, (s+3)²),
    9 → (6, (s+3)²), 10 → (3, (s+3)²).
  - 0x0B: adds 32q to counter 0x7FFF0, capped at 255 (the move-time
    counter: every champion moves at 1 tick per step while it is set).
  - 0x0E: explosion 0xFF8E on the party square with energy
    clamp(21, (2L + 4)(p + 2), 255).
  - 0x0F: creates the item named by attribute key (13, 15, 11, 0x42): into
    the leader's hand if it is empty, otherwise dropped on the party square
    in a random cell.
- **Summon** (4): creates a minion of creature type `type` in front of the
  party, with power scaled by (2L + roll) × power / 6. Type 0x35 instead
  sets an existing minion of that type to state 0x13, so it is recalled.
  When the minion can't be placed, a fizzle explosion appears.

After success the champion gains xp in the skill and is busy for the
cooldown (0x40A0A); both happen only when the cooldown is non-zero. A
failed level check grants xp >> (required − level), using the original
level deficit, not the remaining loop count. Spell kinds outside 1-4
succeed with no effect.

**Lookup detail** (0x421FD): the key packs the runes into bytes 3, 2, 1, 0
in entry order (power first) and stops at the first empty rune; at least
two runes are needed. A spell whose byte 3 is 0 matches the low three
bytes only, so any power works. Result codes passed to the message routine (0x4384B)
are 0x10 failure, 0x20 meaningless runes and 0x30 need flask, each ORed
with the class. The rune buffer is cleared except after "need flask".

### Party shields and effects (0x45815, 0x4565A)

`party_effect(kind 0-6 or 0xF, strength)` adds a timed party effect of
magnitude strength / 32 (spell shield, fire shield, and others). Called
from items, it costs 4 mana; with less than 4 mana the effect is halved
and the mana drops to 0.

## Open questions

- Exact meanings of RP and WH, and the 0x7FFF0 and 0x7FFEF counters
  (probably magic-footprint and see-through-walls style effects).
- Names for each spell type, matched to their in-game effects.
- The term at 0x7F282 in the hit roll.
- Full missile flight per tick (step energy, collisions) and the
  explosion life cycle in the timeline. That belongs with 05-timeline.
