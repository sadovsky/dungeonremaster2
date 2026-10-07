# Core simulation: main loop, timeline, movement, actuators

How SKULL.EXE advances the world. Addresses are in the flat image (see
`01-executable.md`). Names are working names. Confidence tags:
**code** = read from the code; **tentative** = inferred, needs checking.

## Global state used throughout

| Address | Meaning |
|---------|---------|
| 0x7F22C | Game tick (u32). Incremented once per main-loop iteration. Saved with the game. |
| 0x72244 (high word of 0x72242) | Current map, i.e. the map whose tables are "selected" (see map switching) |
| 0x7F262 (high word of 0x7F260) | Party map |
| 0x7F26C / 0x7F26E | Party x / y |
| 0x7F254 (high word of 0x7F252) | Party facing (0-3) |
| 0x7F26A (low word) | Party movement cooldown, in ticks |
| 0x717F4 (high word of 0x717F2) | Pending map change for the party (−1 = none) |
| 0x717F2 (low word) | "Tick elapsed" flag set by the timer interrupt |
| 0x7F38C | Viewport-dirty flags (bit 0 = redraw, 3 = full redraw) |
| 0x70518 | Random number generator state |

## Random numbers (code)

One generator for the whole game (0x1C6A1):

```
state = state * 0xBB40E62D + 11        (32-bit wrap-around)
result = state >> 8                    (24-bit)
```

| Address | Returns |
|---------|---------|
| 0x1C6A1 | `state >> 8` |
| 0x1C6B7 | `random(n)`: `(result & 0xFFFF) % n`; returns 0 when n = 0 (no generator step) |
| 0x1C6DC | One random bit: bit 0 of `result` |
| 0x1C6F6 | Random 0-3: bits 0-1 of `result` |

Seeding: the state starts at 0 in the executable's data. The only other
writer is the load path (0x370D2), which restores it from offset +4 of the
60-byte game header block read with the saved game. It is also written into
saves (0x3502B). There is no time-based seeding, so a new game is
deterministic given identical input timing.

**Measured against the original (DOSBox).** A new game starts from seed 0.
Three separate new games saved at ticks 57, 92 and 158 stored generator
states whose low 16 bits each match exactly one plausible draw count from
seed 0: about 2,422, 3,728 and 6,309 draws. All three runs started with
the same food and water, which from seed 0 are draws 121 and 122. So:

- before the starting champion is recruited the original makes 120 draws;
- after that it draws about 37-39 times per tick, even with the party
  idle in the starting cave.

Round 3 settled this with a draw log taken inside the running original
(below, "Draw log"). The remake now matches the original's draw stream
exactly up to the recruit: 122 draws, and the starting champion's food and
water come out at the original's 1697/1686.

**Idle draw rate (measured, round 2).** Two more new games, left idle and
saved, give 4,564 draws by tick 114 at DOSBox's normal speed and 1,281 by
tick 27 with the CPU slowed to a fixed 6,000 cycles. A straight line
through the idle runs gives about 37.7 draws per tick plus about 262 before
tick counting starts, and it also fits round 1's saves at ticks 57 and 92
within about 10 draws. The slowed run did not draw fewer per tick, so the
source is tied to game ticks, not to how fast the CPU spins. The ~262 is
more than the 120 before the recruit: about 140 more draws happen between
the recruit and the first tick. The two saves 92 ticks apart with the party
facing a wall (map 0 (1,1)) give a similar 41 per tick, so the 3D view is
not the main source either.

**Ruled out by tracing** (tools/rngshallow.py, rngcalls.py, rngorphans.py,
rngptrs.py): every one of the 277 direct calls to the generator routines is
inside a known function, and no generator-reaching function is reached only
through a stored pointer, so there is no hidden interrupt callback. Of the
main loop's per-tick work (0x24691), the weather step 0x5A073(0) draws once
per tick while it isn't raining; 0x54EAC, 0x47113, 0x4904F, 0x210DC,
0x557B5, 0x1616D and the input wait 0x224A9 draw nothing at idle; 0x4910B
draws only for an animated item held on the cursor; the render-time draws
are the rain overlay (0x4E79C, 0x4E930, only while raining) and the
teleporter and cloud fields (0x509E6), none of them in the starting cave.
The start map's two pending 0x5A events (wall and floor ornament
animations, 0x5964E) draw nothing.

**Draw log (round 3).** A DOSBox 0.74 build with a small hook in its normal
CPU core (it matches the generator's `imul eax,[state],0xBB40E62D`
instruction, so it catches every inlined copy) logged each draw with the
game tick (0x7F22C), four stack words and the creature whose AI context is
loaded (0x7F548). The generator is inlined at three places: 0x1C6A1 (raw,
also behind `random(n)` at 0x1C6B7), 0x1C6DC (one bit) and 0x1C6F6 (two
bits). A new game left idle for 158 ticks gave:

| Phase | Draws | Callers |
|-------|-------|---------|
| Before the recruit | 120 | weather start 0x59F38 (3), new-game creature pass 0x3624F (28), one weather step 0x5A073 (1), then 80 creature activations through 0x3023F/0x14F1B (88) |
| Recruit | 2 | 0x49242 (food and water) |
| Tick 1 | about 417 | every activated creature's first step: set selection 0x259CC (two per think), frame timing 0x3023F, think 0x262F7, context roll 0x24BFC |
| Idle, per tick | 36.2 | 0x3023F 41%, 0x259CC 24%, 0x24BFC 12%, 0x14F1B 10%, 0x262F7 9%, weather about 1 per tick |

**Creature pass at play start (0x34236 → 0x34106).** 0x34236 runs the
per-map pass for every map in order (selecting each map, then restoring the
party's), at the main loop's start, after loading (0x342A3) and after
saving (0x3502B); 0x24629 runs it for the arrival map on a map change. The
pass visits the current map's squares column by column; on each square the
first creature group without a slot is either activated (0x306A8) when its
type's info byte 0 bit 0 is clear, or left dormant when the bit is set:
0x301F3 → 0x14E42 writes the first frame of action 0x11's sequence to
record word +8 and its frame count to word +10, flagged 0x9000 (or
`(w12 & 0x3F) << 6 | 0x8000` with record word +12 set), keeping the old
word's 0x6000 bits and an old 0x8001 pattern (mask 0x803F). A dormant group
draws nothing. In the shipped dungeon 219 groups are dormant and 80 are
awake (round 2's "all 299 have the bit set" was wrong). Dormant groups wake
only through the actuator signal 0x2538C (which activates a group whose bit
is set) or by being hit; the thing-move routine 0x4B108 activates a moved
group only when the bit is clear.

**Activation (0x306A8)** runs the frame scheduler 0x3023F at once for awake
types: it starts the current action's sequence (0x14E42 → 0x14F1B, a draw
per branching frame) and rolls jitter, flip and extra ticks. The off-map
slowdown branch (×4 plus a random bit) is not taken here: the log shows the
extra-tick and jitter draws at activation but no off-map bit. The first step
is due on the next tick.

**When the off-map slowdown applies (round 4).** A hook on every entry to
0x3023F (not just on draws) showed that an idle creature's six-frame
sequence is slowed ×4 on only two of its frames: the one after think and
the one whose frame event ran. Its status word stays 0x8000 throughout. The
deciding term is the class-flags test: 0x3023F reads the flags through the
loaded AI class index, and 0x24A88 resets that index to −1 at the start of
every creature event; set selection in think and the context setup before a
frame event load it. With −1 the lookup lands on the 4-byte entry before the
class table, which is a relocated pointer whose third byte has bit 0 set, so
plain frame steps skip the slowdown. The remake reproduces this with a
per-event "class loaded" flag (`GameState::creature_class_loaded`).

**The wander list (0x73392).** Think's opening, read from the disassembly
because the decompile garbles it: list 0x73399 sets the idle action with no
draw; list 0x73392 makes one draw r = rnd & 7 and then stands still
(r 4-7), turns to face tick & 3 (r 0), or walks to the square ahead (action
2, r 1-3) unless it is a wall, rock, or an edge link to a map whose
creature list lacks the type (0x1F9FF). Only other lists run the context
setup and its alertness roll.

**Result (round 6).** With the frame-event results, quarter turns and
the armed-byte reset ported (docs/08, "Frame events"), an idle new game
matches the original's draw log draw for draw through tick 157, the whole
recorded log: 6,255 draws on both sides, the same tick and creature for
every one. `tools/rngcmp.py` compares the original's log with the
remake's ordered draws from `examples/rngseq`.

**Resuming after a load (round 6).** The original's first tick after
loading runs one weather step before the per-map creature pass, then the
usual weather step at the end of the tick: two weather draws around the
activations. The remake does the same when play resumes after a load
(and after a save, which takes the loaded state's flags). Compare a save
written by the remake with its trailer stripped, since the original reads
only the 16 low bits of the random state from the save stream. From the
pit probe's save, the draw stream then matches the original's for the
whole first tick (90 draws) and parts at tick 1702, where the planner
chooses differently (docs/08, goal kinds 6 and 7).

**Round 7: map changes inside the tick.** The main loop's world phase
(0x24691) applies a pending map change (0x24629, whose map entry 0x3AB31
ends with one weather step), drains the due events, repeats while the
events leave another change pending, then runs the regular weather step.
Its walking pass carries out a step after the mid-step frame and, when the
step leaves a map change pending (a pit fall, stairs), runs the world phase
again before the tick ends. The original's draw log of a pit fall shows
exactly that: the fall damage, then the new map's entry weather draw, then
a second regular weather draw, all on the same tick. The remake now does
the same, so a command that changes the map enters the new map that tick.
With the planner's path test and goal ranking ported (docs/08), the pit run
matches the original draw for draw from the load through tick 1709 (656
draws; it parted at tick 1702 before); the idle run still matches through
tick 157. At tick 1710 the original's planner still finds the creature's kind-7
goal (program 1, action 3) although the party has fallen to the map
below; the remake finds nothing within the off-map limit and falls back.
The likely cause is "the party" the planner uses: the projected party
(0x7F8D2-0x7F8D6), which 0x2FE35 refreshes only when the creature's map
differs from the current map and otherwise leaves as it was, so after the
fall it may still name the pit square on the upper map. The remake uses
the live party position. Confirming this needs the projected values read
from the running original.

**Result (round 4).** Idle new game, original against remake: 663 against
666 thinks over ticks 2-157, 36.2 against 35.9 draws per tick, and ticks 0-1
draw 420 against 419.

**Round 5: draw for draw through tick 51.** Comparing the remake's ordered
draw log (`examples/rngseq.rs`, through `rng::trace_seq_start` and
`trace_context`) with the original's, creature by creature:

- **The 8 tick-1 draws** come from the movement test run on the creature's
  own square: think's first danger check (0x2D792 with the current square
  as destination) runs the missile danger scan (0x2D52D) in mode 5, and the
  scan rolls `rnd & 7` for the direction behind the creature. Ported with the
  scan's blocking test (0x2B9FC) and think's whole danger and escape block.
- **Event order.** A creature's timeline event carries the creature's type
  (record byte +4) as its priority byte (0x3059D), so same-tick creature
  events run higher types first. The remake had priority 0, which reordered
  every creature on tick 1.
- **No re-plan roll.** The behaviour picker (0x26008) runs on every think.
  Its two-bit "keep the plan" roll only applies while the event's path cache
  (0x7F7D4/D5/D7, reset by the context setup) is in use, which it never is at
  that point. The remake's extra `random(4)` re-plan roll is gone.
- **Alertness flag (0x7F589).** The context setup sets it when
  `n / 4 + random(n + 1)` is at most the ticks since the slot's last action
  (slot +4, mod 256), with n = (15 - alertness) * 2.

Ticks 0-1 now draw exactly the original's 542 including setup, every tick-1
draw is at the same site for the same creature, and the stream matches
through tick 51. **Open:** on tick 52 creature 0x1039 (type 0x38) reaches
frame offset 4 in the original but offset 2 in the remake. Its frame-event
frame (offset 2, chained, jump 2) fires in both; the original then follows
the chain, which needs the slot's armed byte (+0x21) set, but the remake's
frame events always return 0. Returning each handler's success (as 0x2B75E
passes back its handler's result) armed far too often and broke the stream
at tick 11, so which handlers return non-zero is still to be traced.

**Slot pool.** Sized at game start (0x342F9) as min(awake groups + 100,
creature records): 180 for the shipped dungeon. The remake had a fixed 75,
which left the five awake creatures on maps 42 and 43 inactive.

**Remaining gap.** The remake now matches through the recruit and makes
about 32 draws per idle tick against 36.2; its creatures think about 20%
less often (518 thinks in 156 ticks against 663, across the same 80
creatures). Every per-step routine draws the same kinds of numbers, so the
difference is in how long the chosen actions last, not in which draws are
made. Tools: `examples/rngtrace.rs` counts the remake's draws per call site
(through `rng::trace_start`), `examples/creaturewhy.rs` explains a
creature's activation state.

**Idle upkeep matches.** Over 114 idle ticks from a new game the original
drains food by 4 and water by 2 with health and stamina unchanged
(SKSAVE7: food 1697 → 1693, water 1686 → 1684); the remake drains the same
amounts. Only the starting values differ, because of the random-stream gap.

**New-game creature pass (0x3624F, creature case).** While the dungeon
loads for a new game, every creature group on every map (in map, column,
row and list order) gets its type's base hit points (info word +4) in
record word +6. If the type's info byte 0 bit 0 is clear, word +10 is
cleared and word +0xC records the group's home square (x in bits 0-4, y in
bits 5-9, map in bits 10-15). If it is set, words +8 and +10 are cleared
and, unless record byte +0xE bit 7 is set, each link of the possession
chain gets a random cell from a 2-bit draw (0x1C6F6), starting with the
group's own possession word.

Every call site advances the shared state, so the order of calls matters if
the remake wants identical behaviour. The `random(n)` helper only advances
the state when n is non-zero.

## Main loop (0x24691, code)

The loop never returns except to quit. One iteration is one game tick:

1. **Pending map change.** If the party has a pending map (0x717F4 ≠ −1):
   arrive on that map (0x24629: load the map's graphics set, music, redraw),
   place the party on its square (0x4B108 with "no source"), clear the
   pending map.
2. **Timeline.** Run every due event (0x59A5C, below). If an event caused
   another map change, go back to step 1.
3. Per-tick champion update 0x5A073(0) (tentative: status timers).
4. Unless the game is frozen (0x7F234): run the creature update 0x54EAC
   (skipped while 0x7F284 is set), then, if no inventory screen is open,
   redraw the viewport when dirty (0x2F8F5), draw the 3-D view from the
   party's position and facing (0x54B3F), and present the frame (0x138D9).
   A remote view counter (0x7F258) repeats this step from an alternate
   position: tentative, a magic-eye-type effect.
5. **Input.** Process queued input (0x210DC).
6. Per-tick housekeeping:
   - 0x232B8 (unknown).
   - Sound update (0x1616D).
   - Champion regeneration and timers (0x47113, which may schedule event 0x0C).
   - Decrement a counter at 0x7FFF0.
   - Every 64 ticks (every 16 if the frozen flag is set), run 0x47CC3
     (tentative: food, water and stat decay).
   - Interface updates (0x4910B, 0x4904F).
7. If the quit flag 0x7F24C is set, run the ending (0x206C6) and leave.
8. **tick += 1.**
   - Every 512 ticks, run 0x39044.
   - Decrement the movement cooldown and the counters at 0x7FFEF and 0x7F25A.
   - Run 0x557B5.
9. **Wait for the next tick.**
   - Clear the tick flag.
   - Spin in 0x224A9 until the tick flag is set and input is enabled (0x7F244). This drains the player's command queue (up to 3 queued 14-byte commands, handled by 0x21D6C) and polls input.
   - In the same loop, a trick wall the party stands in closes or reopens (the 0x7F220 logic).
10. Switch the selected map back to the party's map and check for a music change (0x10AF6).

Input is also polled before every timeline event (0x210DC is called inside
0x59A5C), so long event bursts don't drop clicks.

### Tick timing (code)

The tick is driven by an interrupt handler at 0x10CC2, installed through
the `IBMIOP` launcher's service interface (`int 0xFC`, services 0x2A, 0x2D,
0x0F). On every interrupt it:

- adds a per-interrupt increment (word at 0x7EFAC, returned by the install
  call) to a sub-tick accumulator (0x7F256);
- also adds to a millisecond-style counter at 0x7F228 (used by input timing);
- sets the tick flag when the accumulator reaches the threshold at 0x7F268.

The main loop sets the threshold to 8 and resets the accumulator at the
start of each tick. One code path sets the threshold to 1 (fast mode, used
while some modal state is active; tentative).

### Timer rate (from IBMIOP.EXE)

`IBMIOP.EXE` is packed with LZEXE 0.91; `tools/unlzexe.py` unpacks its
load image to `re/ibmiop.bin` for disassembly. The launcher (a Borland
C++ program) hooks int 8 and reprograms PIT channel 0 (mode 3, port
0x43 value 0x36) with **divisor 0x136B = 4971**:

- The interrupt rate is 1,193,182 / 4971 ≈ **240.0 Hz**.
- On every 4th interrupt (**60 Hz**) it calls the far callback that the
  game registers (stored at [0xA9D0] by the setter at 0x2877). The call
  is skipped while a busy flag is set.
- It keeps the BIOS clock right by subtracting the divisor from a 16-bit
  accumulator and chaining to the original int 8 handler on each borrow
  (about 18.2 Hz).
- On exit it restores divisor 0 (the standard 18.2 Hz) and the old
  vector.

The game's handler (0x10CC2) therefore runs at 60 Hz. Each run adds the
increment at 0x7EFAC to the sub-tick accumulator; a tick fires when the
accumulator reaches the threshold of 8. The increment is the word the
launcher's service 0x0F writes into the shared buffer at 0x8048C after the
handler is installed (0x5A8B1). That service wasn't found in the
unpacked launcher, so the increment is still unconfirmed.

**With an increment of 1, the tick is 8/60 s ≈ 133 ms (7.5 ticks per
second).** That fits the first game's roughly 6 per second. The remake
should use 7.5 Hz as the default and keep it configurable until the
increment is confirmed (in DOSBox, read 0x7EFAC or count 0x7F22C over a
minute). The fast-mode threshold of 1 would then be 60 ticks per second.

**Measured in DOSBox (confirmed).** Running the original under DOSBox 0.74
and holding the party's forward move on map 0 from the start square, the
viewport redraws once per tick while walking. Timestamped window captures
gave redraw intervals of 0.12-0.15 s, averaging 0.134 s over ten steps
(capture jitter about ±20 ms). The tick is therefore 133 ms (7.5 Hz),
consistent with an increment of 1. The tick is driven by the launcher's
timer, so DOSBox CPU cycles don't change it.

## Selecting a map (0x1C724, code)

Most code works on "the current map", a set of globals that point at one
map's tables. Switching the map updates:

- the column pointer table for that map (0x7F3E4);
- the map descriptor pointer (0x7F3BC);
- width and height (0x7F40E, 0x7F40C);
- the first-thing table base (0x7F3F0);
- the current map number;
- the "party as seen from this map" position (0x7F27A/0x7F27C/0x7F27E).

That last position is normally the party's real position. When the party
is inside a linked second map (0x7F230 set and the selected map is the one
in 0x7F278), it is the alternate position instead. Tentative: this is how
DM2 handles the party standing on an outdoor/indoor seam.

Timeline events carry their map in the top byte of the time field, and the
processor selects that map before running each handler. Handlers that touch
other maps switch and switch back.

## Timeline / event queue

### Storage (code)

- **Event array:** 0x760E8 points to an array of 12-byte event records.
  Capacity is the high word of 0x80426; error 0x2D if it is exhausted.
- **Free list:** unused records are chained through their first word, with
  the free head in the low word of 0x80426. A free record has type 0.
- **Heap:** 0x80420 points to an array of u16 record indices forming a
  binary min-heap; the count is 0x8042A. 0x8042C is the high-water mark of
  used records.
- **Deferred sift:** 0x80424 holds a heap position whose sift is pending
  after a removal. It is applied lazily before the next heap operation.

### Event record (12 bytes)

| Offset | Size | Field |
|--------|------|-------|
| 0 | u32 | bits 0-23: due tick; bits 24-31: map number |
| 4 | u8 | Event type (0 = free) |
| 5 | u8 | Priority / small parameter (champion index or mask, action priority ...) |
| 6 | u8 | x (or the low byte of a 16-bit parameter) |
| 7 | u8 | y (or the high byte) |
| 8 | u8/u16 | Parameter: cell, a thing reference, packed coordinates ... |
| 9 | u8 | Action for square events: 0 = set, 1 = clear, 2 = toggle |
| 10 | u16 | Extra parameter (some types) |

Time is compared on the low 24 bits only, so the map byte never affects
ordering. The tick counter has 32 bits, but the queue wraps after 2^24
ticks: tentative, not a practical concern.

### Ordering (0x560B8, code)

Event A runs before event B when:

1. its due tick is earlier; or, on equal ticks,
2. its type number is **higher**; or, on equal types,
3. its priority byte (+5) is **higher**; or, on equal priority,
4. its record index is lower (an earlier-allocated slot).

### Operations

| Address | Operation |
|---------|-----------|
| 0x56390 | Schedule: copy the 12-byte event into a free record and push it on the heap. Returns the record index; −1 if the type is 0. |
| 0x5643D | Pop the earliest event into a caller buffer and free its record |
| 0x562FF | Delete a record by index (frees it and removes it from the heap) |
| 0x562CE | Re-sift a record after its time changed |
| 0x5646F | "Is an event due?": the heap is non-empty and the head's tick ≤ the game tick |
| 0x560F9 | Find a record's heap position (error 0x46 if missing) |
| 0x56134 | Sift a heap position up, then down |

Missiles and active creatures store their event's record index so they can
move or cancel it later; for example a missile keeps it in its word 3, and
creature movement rewrites the x/y and map bytes of the creature's event in
place.

### Processor (0x59A5C, code)

While an event is due: poll input, pop the event, select its map, and
dispatch on its type. When none are due, select the party's map again.
Handlers that reschedule their own event add to the time field and call
0x56390 again with the same record contents.

### Event types

Who schedules each type was recovered by scanning every call to 0x56390
(script: `re/sched_scan.py`, which works on the gitignored analysis output).

| Type | Handler | Scheduled by | Meaning |
|------|---------|--------------|---------|
| 0x01 | 0x564C6 | door action, itself | Door animation step: one state per tick (see Doors) |
| 0x02 | 0x56AF9 | 0x18D9E (door bashing) | Door destroyed: set the door square's state to 5 |
| 0x04 | per square, below | actuators (0x4BBE4), cross-map relay | Square action: deliver set/clear/toggle (+9) to square (x, y), cell (+8) |
| 0x0C | 0x59026 | champion update 0x47113 (+5 = champion) | Resets a per-champion word to 0xFFFF and flags the champion for redraw. Tentative: end of the "damage received" display. |
| 0x0D | 0x59050 | floor sensors 0x4CDCC | Resurrection at an altar, in three stages driven by +9: stage 2 spawns the rebirth effect 0xFFE4 at (x, y) and waits 5 ticks; stage 1 removes and deletes the champion's bones from cell +8 and waits 1 tick; stage 0 revives champion +5 (0x49CBB). See 06-champions. |
| 0x0E | 0x59207 | 0x45A9D | Unknown (champion-related caller) |
| 0x15 | 0x160DB | sound queue 0x15CA9 | Play a delayed or positional sound; +6 = sound slot |
| 0x19 | 0x18395 | explosion creation 0x16746 | Explosion or cloud lifetime step |
| 0x1D, 0x1E | 0x17A7B | missile launch 0x16457 | Missile movement step. 0x1E while the launch flag 0x7F1A4 is set, which actuator shooters set. Tentative: 0x1E means launched by the dungeon rather than by a champion. +6 = missile thing; +8 packs x (bits 0-4), y (5-9), direction (10-11), cell (12-15). |
| 0x21, 0x22 | 0x257CC | creature AI 0x3059D, 0x252C3 ... | Creature group behaviour tick; +5 = creature type, +6/+7 = square. 0x22 is used when the creature record's word +8 is in use. Details belong in 08-creatures-ai. |
| 0x3C, 0x3D | 0x58FC2 | 0x4A096 | Five ticks after a thing arrives on a square: deferred arrival processing. 0x3D variant via a flag argument. Tentative. |
| 0x46 | 0x5917D then 0x389C2 | 0x412E1 | End of a light or darkness effect: adds the signed level in +6 back to the light counter 0x7FFEC (see 07-combat-magic, "Light and darkness"). |
| 0x47 | inline | 0x414A5, 0x422F5 | Decrement counter 0x7FFEE. When it reaches 0, mark the champion whose inventory is open (0x7F972) with flag 0x40. Tentative: light or magic-map duration. |
| 0x48 | inline | 0x4565A | A party effect expires: for each champion in mask +5, subtract the u16 at +6 from the effect amount at +0x103 (not below 0). See 07-combat-magic, "Party shields and effects". |
| 0x4B | inline | 0x474FC | Champion +5: decrement champion byte +0x1F and subtract the u16 at +6 from field +0x48, then 0x474FC. Tentative: an expiring per-champion effect. |
| 0x54 | 0x5A073(1) | itself, weather start (0x59F12) | Weather step: advance the rain curve one step, reschedule after random(256)+50 ticks; after 32 steps start a new cycle (see docs/04 "Outdoor weather"). The weather state (rain intensity and level, cloud level and build-up, storm, wind, curve step, pattern and multiplier, next hour change) is saved in the globals record, bytes 0x2A-0x3B (docs/12), and restored on load; the hour offset and hour light are recomputed from the dungeon |
| 0x55 | 0x59293 | actuator 0x32 (0x570B1) | One-shot ornament step: add 1 to the actuator's 9-bit frame counter (word 1 bits 7-15); when it reaches a multiple of the ornament's cycle length (0x56CF4) clear the busy bit, otherwise reschedule for the next tick |
| 0x56 | 0x593CF | clock actuators (0x592FA) | Periodic actuator tick (types 0x1E, 0x33-0x37) |
| 0x57 | inline | wall sensors 0x4C134 | Re-arm an actuator: clear bit 0 of the thing's word 2 |
| 0x58 | 0x59608 | 0x22A68 | Clear bit 11 of word 1 of thing +6 |
| 0x59 | 0x5961B | actuator 0x2C (0x56F11) | If thing +8's word 2 bit 2 is clear: clear bit 0 and redraw |
| 0x5A | 0x5964E | 0x56D6A | Actuator follow-up (0x56D6A also reads ornament attributes) |
| 0x5B | inline (same as 0x57) | wall actuator 0x31 | Re-arm after the debounce delay |
| 0x5C | inline | wall sensors 0x4C134 | Set bit 0 of word 1 of thing +6/+7 |
| 0x5D | inline | floor sensors 0x4CDCC | If the event's map (+8) is the party's map: move the party to (x, y) taken from +6 bits 0-4 and 5-9, then turn it to direction bits 10-11. A delayed teleport. |
| 0x5E | inline | floor actuator (text thing kinds 0x13/0x16) | Pick a random direction (or the direction toward the party) and run 0x30BA6: tentative, spawn or redirect a creature |

All other values are ignored by the processor.

## Square actions (event 0x04)

The handler is chosen by the target square's element type (bits 5-7 of the
square byte):

| Element | Handler | Behaviour |
|---------|---------|-----------|
| Wall (0) | 0x58304 | Run the wall actuators and text on the target cell (below) |
| Floor (1) | 0x57476 | Run the floor actuators and text on the square |
| Pit (2) | 0x58F5C | Bit 3 (open) follows the action. When it opens, 0x58C6F drops whatever stands there. Then run the floor actuators. |
| Stairs (3) | none | Ignored |
| Door (4) | 0x568F7 | Open, close or reverse the door (see Doors) |
| Teleporter (5) | 0x58EDB | Unless word 2 bits 1-2 are both set, bit 3 (active) follows the action; activating it immediately teleports whatever stands there (0x58C6F). Then run the floor actuators. |
| Trick wall (6) | 0x56A03 | Bit 2 (open) follows the action. Closing is refused while the party or a solid creature occupies the square: the event is retried next tick. |
| Rock (7) | none | Ignored |

The action value is resolved by 0x57D27: set → 1, clear → 0, toggle →
the current value inverted.

## Actuators (thing type 3)

### Record (8 bytes; replaces the DM1 assumption in 03-dungeon-dat.md)

| Word | Bits | Field |
|------|------|-------|
| 1 | 0-6 | Actuator type |
| 1 | 7-15 | Data (9 bits): item kind, creature type, counter, delay or map, depending on type |
| 2 | 0 | Busy / triggered latch, cleared by re-arm events 0x57, 0x5B, 0x59 |
| 2 | 2 | Enabled / state bit (toggled by "state" actuator types). For relays: send the configured action instead of forwarding the incoming one. |
| 2 | 3-4 | Action sent to the target: 0 set, 1 clear, 2 toggle; 3 = "follow", which sends set while the condition holds and clear when it stops |
| 2 | 5 | Inverted: respond to a clear instead of a set |
| 2 | 6 | Play the actuator sound when fired |
| 2 | 7-10 | Delay in ticks before the target event (also used as a shift count by type 0x45) |
| 3 | 0-3 | Ornament (tentative, from DM1) |
| 3 | 4-5 | Target cell |
| 3 | 6-10 | Target x |
| 3 | 11-15 | Target y |

"Firing" an actuator (0x4BC4C) schedules a type-4 square-action event on
the current map:

- **time:** now + the actuator's delay + any extra delay;
- **target:** the square and cell given in word 3;
- **action:** the actuator's action;
- **priority byte:** 1 for set, 3 for clear, 2 for toggle (0x4BBE4).

So on the same tick, clears land before toggles, which land before sets.

### Wall actuator types (handler 0x58304, code)

The handler walks the target cell's thing list. Actuators are handled with
a jump table at 0x581F8 indexed by `type − 7` (types 7-0x49). Text things
(type 2) on the cell are also handled: kinds 5, 6 and 7 follow the action
(visible flag), kind 7 also mirrors the state onto the linked map, and
kind 0x17 plays a sound on set. Types not listed below do nothing.

| Type | Handler | Behaviour |
|------|---------|-----------|
| 0x07, 0x09 | 0x57A63 | Shooter: create one (0x07) or two side-by-side (0x09) items of kind *data* and launch them as missiles. The shot is placed from the event itself, not from a target: it starts on the square one step from the event square (+6/+7) in direction *d* (event +8), flying in direction *d*, in cell (*d* + 2) & 3 (and the next cell for the second shot). Word 3 holds no target here: bits 4-11 are the kinetic energy and bits 12-15 the step energy; the attack byte is always 100. A single shot adds a random bit to its cell, drawn after the item is created. |
| 0x08, 0x0A | 0x57A63 | As above, but launches spell missiles (explosion type 0xFF80 + *data*) |
| 0x0E, 0x0F | 0x57A63 | As above, but launches the item(s) lying on the event square in cells *d* and *d* + 1 |
| 0x12 | inline | End the game: stop sound, set 0x7F23C, start the ending (0x2005B) |
| 0x16 | inline | Cross-map relay: forward the same action, at the same tick and priority, to square (word 3 x, y) on map *data* & 0x3F. Cell = *data* bits 6-7 if the target is a wall, else 0. |
| 0x1D | inline | Up/down counter in *data* (9 bits; a value with bit 8 set counts as below zero): a clear increments it, a set decrements it (a set is ignored when word 2 bit 2 is set and the count is already 0). Only a change between "zero or below" and "above zero" fires: in "follow" mode it sends clear when (at zero) equals the inverted bit and set otherwise; in other modes it fires the configured action only on reaching zero. |
| 0x1E, 0x33-0x37 | inline, then 0x592FA | State switch plus clock. The action sets or clears word 2 bit 2. While enabled and not busy, it schedules event 0x56 and marks itself busy. The period is *data* × {1, 8, 16, 32, 64, 128} ticks (by type); the first event is due at `tick + tick mod period`, so the phase depends on the current tick, not on the random generator. Event 0x56 (0x593CF) keeps the multiplier in byte +8 and a phase bit in byte +9. In "follow" mode each period flips the phase and sends set on phase 1, clear on phase 0, continuing while the phase is 1 or the clock is enabled. Otherwise each period fires the configured action while enabled; once disabled the clock stops and clears its busy bit. |
| 0x20 | 0x572A8 | Timer. When word 2 bit 2 is clear it relays every action; when set, only a matching action (set, or clear if inverted), and it then sends its configured action instead of the incoming one. The target event is due after (word 2 bits 7-10) + *data* ticks. |
| 0x26 | inline | State switch only: word 2 bit 2 follows the action |
| 0x2C | 0x56F11 | Animated ornament switch. Word 2 bit 2 is the switch (set/clear/toggle as usual), bit 0 "animating", and word 1 bits 7-14 hold the animation phase. The cycle length n is the ornament's number attribute 0x0D (category 9 on walls, 10 on floors; the ornament comes from word 2 bits 12-15), else the length of its frame-digit text (type 5, sub 0x0D), else 1 (0x56CF4). Switching on sets bit 0 and stores the phase `(n − tick mod n) mod n`, so the animation starts on frame 0; with the sound bit it also plays the ornament's sound 0x88 and schedules repeats through event 0x5A (0x56D6A, repeats not modelled in the engine). Switching off computes `(phase + tick) mod n`: at 0 the animation stops at once, otherwise event 0x59 is scheduled for the end of the cycle and clears bit 0 then, unless the switch was turned on again. When inverted in "follow" mode the incoming action is also passed to the target. |
| 0x2D | inline | If *data* is 1-400: decrement it and forward the incoming action. If 401-499: a percentage gate. One `random(100)` call; the gate fails when *data* − 400 ≤ the roll, so it passes with probability (*data* − 400)%. In "follow" mode it sends set on a pass and clear on a fail; otherwise it forwards the incoming action only on a pass. |
| 0x2E | inline | Creature generator: on set, create a creature of type *data* at the target square through 0x30BA6. Direction comes from word 2 bits 3-4, or random if bit 2 is set. If bit 5 is set, also store a value from word 2 into the creature's word +8; bit 6 plays a sound. |
| 0x31 | inline | Debounced relay: if not busy, mark busy and schedule a re-arm (0x5B) after *data* ticks; if the action matches, fire the target (with the configured action when bit 2 is set) |
| 0x32 | 0x570B1 | Play the ornament's animation once. If word 2 bit 0 (busy) is clear: set it, reset the 9-bit frame counter in word 1 bits 7-15, schedule event 0x55 for the next tick (bytes 8-9 the actuator, bytes 10-11 the wall/floor flag) and, with the sound bit, play the ornament's sound 0x88. A trigger while it is playing does not restart it. If word 2 bit 2 is set it also relays the event as 0x3D does (0x571F3). |
| 0x3B, 0x40, 0x47, 0x48, 0x49 | 0x57E6C | Item relay between the event's square and the actuator's target. 0x40 matches against an item kind list (docs/09, "Item kind lists"): text (15, *data* & 0xFF, 5, (word 2 bits 7-10) × 3 + 0x20), instead of the single *data* kind. 0x47 and 0x49 reverse the direction. 0x48 and 0x49 move only the first match. Items carried by creatures on the square are searched too. |
| 0x3C | inline | Item generator: on set (or clear if inverted), create an item of kind *data* and place it at the target square and cell (0x57D4C) |
| 0x3D | 0x571F3 | Relay with *data* as extra delay. In "follow" mode: not inverted, it forwards the incoming action after the delay; inverted, it forwards the action at once and then sends the opposite action (toggle stays toggle) after *data* ticks, making a pulse. Other modes fire the configured action on a matching trigger (set, or clear if inverted). |
| 0x41 | inline | Randomise: set *data* to a random value below an ornament attribute (category 9 or 10, attribute 0x0D). Tentative: the ornament frame count. The engine uses the ornament's cycle length (0x56CF4), which starts from that attribute. |
| 0x42 | 0x56B39 | Face creatures: on a matching trigger (set, or clear when inverted), the creature group on the target square is turned to face *data* & 3 (0x49EF8 in absolute mode; for types with flag bit 0 the routine also turns the cells of the things the group carries). |
| 0x43 | 0x5737C | Set a script variable: apply the incoming action to variable *data* through 0x1512E with operation = action, plus 3 when inverted (so inverted set adds 1, inverted clear subtracts 1, inverted toggle does nothing), then fire the target with the configured action (word 2 bit 2 set) or the incoming one. |
| 0x44 | 0x573E9 | Test a script variable: r = (variable *data* is non-zero). A set or toggle passes when r differs from the inverted bit; a clear passes when they agree. A passing event fires the target, with the configured or incoming action as for 0x43. |
| 0x45 | 0x572A8 | Long timer: like 0x20, but the delay is *data* << (word 2 bits 7-10) |
| 0x46 | inline | Set, clear or toggle bit 13 of word 1 of the door or teleporter record on the target square (meaning of that bit still unknown) |

### Script variables (0x150AE read, 0x1512E write)

Actuators 0x43 and 0x44 work on 192 dungeon script variables, the same
ones a save game stores:

| Ids | Storage | Notes |
|-----|---------|-------|
| 0-63 | Bits of the 8-byte bitmap at 0x7F100 | A write stores "non-zero" |
| 64-127 | Bytes at 0x7F080 + id (0x7F0C0 onward) | Clamped to 0-255 |
| 128-191 | Words at 0x7F008 + 2·id (0x7F108 onward) | 16 bits, wrapping |

Ids from 192 up read as 0 and ignore writes. The write routine takes an
operation and an operand (EDX): 0 set to 1, 1 clear to 0, 2 toggle (1 when
0, else 0), 3 add the operand, 4 subtract it, 6 assign it; other values
leave the variable unchanged.

### Floor actuator types (handler 0x57476, code)

| Type | Behaviour |
|------|-----------|
| 0x0B, 0x28 | 0x56BA5 with word-1 bits 7-10 and 11-15, the event square, the target square and the action. Tentative: creature-affecting trap. |
| 0x20, 0x45 | Timers as on walls |
| 0x27 | Marks a teleporter square as an edge link that is currently disabled (see Map edges). It has no effect of its own when triggered. |
| 0x2C | 0x56F11 (variant 0) |
| 0x2E | Move or rotate the party: target = word 3 x/y if word 2 bit 2 is set, else the event square. Direction = word 2 bits 3-4, relative to the party's facing unless bit 5 is set (absolute). Done through 0x4BED2. |
| 0x32 | 0x570B1 |
| 0x3A | 0x2538C(x, y, action == set). Tentative: creature-related. |
| 0x3B, 0x40, 0x47-0x49 | Item relays as on walls |
| 0x3D | Relay as on walls |
| 0x42, 0x43, 0x44 | As on walls |

Text things on floors: kinds 0x13 and 0x16 schedule event 0x5E on set;
kind 0x17 plays a sound.

Floor markers (text things with word 1 bits 1-2 equal to 1, kind in bits
11-15) react to the party walking on (0x4CDCC):

- **Kind 9, random pulse:** see "Floor sensors".
- **Kind 10, unstable floor:** when the party steps on, sum over living
  champions load ÷ (maximum load ÷ 2), giving a pressure P. The chance is
  min(90, 10P + 25), or 10P + 50 when word 1 bit 0 is set. One
  `random(100)` call decides. On a slip, event 0x5D is scheduled for this
  tick to put the party back on the square with its current facing, and a
  champion picked with `rand4()` (or the leader, if that one is dead) cries
  out with their sound 0x82. Otherwise the floor ornament named in bits
  3-10 plays its sound 0x88.
- **Kinds 0x0B and 0x0C:** destination and source markers of random pits.

### What triggers actuators

Actuators only receive square actions. The events are produced by:

- **Floor sensors** (0x4CDCC): called by the move routine when the party,
  a creature or an item leaves or enters a square. They schedule square
  actions (0x4BBE4 and 0x4BC4C), delayed teleports (0x5D) and staged
  effects (0x0D).
- **Wall sensors** (0x4C134): clicking a wall, or putting an item into
  it. They also schedule the re-arm events 0x57 and 0x5C.
- Other actuators: chains through the target square.

The trigger conditions inside 0x4CDCC and 0x4C134 (which thing types
or item kinds count, and which cell must be occupied) are still to be
written up; they belong with the interaction notes.

## Doors (code)

A door square stores its state in bits 0-2:

| State | Meaning |
|-------|---------|
| 0 | Open |
| 1-3 | Partly closed |
| 4 | Closed |
| 5 | Destroyed |

States 0, 1 and 5 are passable. The door thing (type 0) word 1 holds:

- bit 9: direction (set = opening, clear = closing);
- bit 10: animation running;
- bit 12: cleared when an action starts a move;
- the rest: door type and ornaments (see `03-dungeon-dat.md`).

**Door action (0x568F7).** A destroyed door ignores actions. If an
animation is already running, the action can only reverse it (0x57D27 on
bit 9). Otherwise an open door starts closing, a closed door starts
opening, and a part-way door moves according to the action. The handler
sets the animating bit and starts the type-1 event.

**Door animation (0x564C6).** One state per tick:

- A destroyed door, or one that is no longer animating, stops.
- **Closing onto the party:** if the door closes on the party's square
  while living champions are there, the door snaps back fully open.
  Champions take damage (0x4766B, damage value derived from the door
  attributes) and a hurt sound plays.
- **Closing onto creatures:** if a solid creature group is in the square
  and the door's state has reached the group's size class (creature
  attribute bits 6-7; non-material creatures are ignored), the creature
  takes damage (0x24E62, halved for creatures with attribute 0x19 bit
  0x10). The door then backs off one state and a sound plays.
- **Otherwise** the door steps one state toward its goal. Reaching 4
  (closed) or 0 (open) ends the animation. Sound 0x8F plays when it shuts;
  0x8E plays on other steps.

**Door type and attributes.** Word 1 bit 0 selects the map's door type 0
or 1 (descriptor word 14, each enabled by a descriptor flag); with neither
enabled the type is 0xFF (0x1FE1C). Attribute (14, type, 11, 0x0E) is the
door's strength and (14, type, 11, 0x0F) the damage it does when closing on
something. Missing attributes read as 0.

**Closing details (0x564C6).** The party check applies only while the door
is part-way (state ≠ 0). A creature is hit when its size is at most the
current state; the size is 1 unless door word 1 bit 5 is set, in which case
it comes from the creature attribute bits 6-7. A blocked step costs an extra
tick before the next one.

**Bashing (0x18D9E).** Arguments: square, damage, delay, and whether the
hit is magical. Physical hits require door word 1 bit 8, magical hits bit 7.
If the door is closed (state 4) and the damage is at least its strength, it
is destroyed at once (delay 0) or by event 0x02 after the delay. The party's
bash (moving into a closed door) adds, for each of the two front champions,
a strength term plus `rnd() & 15`.

## Party movement (code)

**Command.** The party move command (0x235BF) takes a relative direction:
command 3-6 minus 3 gives forward, right, back, left.

**Cooldown.** Moving is allowed only when the cooldown has expired. A move
adds half the largest per-champion move time (0x46892, which depends on
load and condition) to the cooldown. While the remote-view counter is
active the move is queued.

**Stamina per attempt.** Before the move is classified, each living
champion pays `load × 3 / max_load + 1` stamina (0x47707), so blocked
attempts and wall bumps cost the same as real steps. Against the original
in DOSBox this is consistent with walking six steps from the start costing
2 stamina by tick 92 once a regeneration call (every 64 ticks) has landed
between the steps; the probes don't record each press's tick, so the
interleaving itself isn't confirmed.

**Destination.** The destination square is computed from the facing by
0x1C9E9, and 0x23D13 classifies the move:

| Result | Situation | Effect |
|--------|-----------|--------|
| 1 | Standing on stairs and moving backward | Take the stairs |
| 2 | Destination is a stairs square | Step onto it and take the stairs at once |
| 3 | Destination blocked (0x4AF72) | No move. Every bump hurts (0x234A8): for each of two starting cells, (facing + move offset + 2) and (+3), the party cells are searched in an order taken from a table at 0x716CC (row = 2 × move direction + bit 1 of the start cell, the start cell first incremented when the direction is north or south), and the first living champion found takes 1 point of damage through 0x4722A (body-part mask 0x18, attack type 2). A champion found twice is hit once. When the damage lands, that champion's cry plays (category 22, their portrait, sub 0x8A). Confirmed against the original in DOSBox: walking to the end of the start corridor and pressing on into the wall nine times cost Torham 8 health, and the remake now loses the same. A closed door is also bashed (0x18D9E, random damage scaled by the party's move time). |
| 4, 5 | A solid creature group stands there | Try to swap or push (0x24171, 0x23E5B, 0x24328); otherwise a 5-point bump (0x24E62 with 0x4005). Tentative. |
| 6 | Free | Move with 0x4B108. If the move leaves the map through a teleporter edge link (0x1D113), the party teleports instead. |

**Blocking rules** (0x4AF72, read from the assembly):

| Square | Blocks? |
|--------|---------|
| Wall | Yes |
| Floor, pit, stairs | No |
| Door | Only in states 2, 3 and 4 |
| Teleporter | No |
| Trick wall | No if bit 2 (open) or bit 0 (illusory) is set; otherwise yes |
| Solid rock | Yes |

**Stairs (0x232DD).** Bit 2 of the stairs square picks the direction: 0
means layer +1, 1 means layer −1. The party leaves its square (leaving
sensors run), 0x1CC7E converts its position to the map on the adjacent
layer, that map becomes pending, and the facing comes from the arrival
stairs (0x1CE6F).

**Adjacent-layer lookup (0x1CC7E).** The position is made global with the
map's origin. The game walks a per-layer list of maps and takes the first
whose bounds, widened by one square on every side, contain the position and
whose square there is not rock. A teleporter square whose record has word 2
bit 0 set counts as rock. Out-of-map lookups read as rock, so in practice
the square must lie inside the map.

**Stairs facing (0x1CE6F).** Stairs bit 3 picks the axis: clear means
east-west, set means north-south. The game looks at the neighbour to the
east (or north). If it is a wall or stairs the facing is west (or south),
otherwise east (or north): the party faces out of the stairwell.

**Walking animation.** When the slowest champion's move time is above 1,
0x235BF does not move at once: it records the command and starts the
mid-step countdown (0x7F258), and the move completes when it runs out. The
cooldown added is the larger of half the move time and the countdown flag.

**Stamina.** Every attempted step charges each living champion a stamina
cost derived from their load (0x47707).

**Turning.** 0x45869 sets the party's facing. It also turns every
recruited champion's facing (+0x1C) and cell (+0x1D) by the same amount,
so the formation keeps its shape relative to the party and the interface
shows the same arrangement after a turn. Rotation from teleporters and
actuator 0x2E goes through the same routine.

## Moving things (0x4B108, code)

`move(thing, from_x, from_y, to_x, to_y)` is used for the party (thing =
0xFFFF), creatures, items and missiles. A negative `from_x` means the thing
is not on the map yet; a negative `to_x` means remove it from the map.

1. **Leave the source square:** 0x4A0FE for the party (missiles sharing
   the square can hit it), 0x4A34A for things.
2. **Resolve the final destination.** It's recorded in the globals
   0x80020-0x80026 (map, x, y, cell) after following pits and teleporters.
   If the thing type is affected (0x1F12F mask 0xF8) and the destination is
   on the party's map, the result redirects the move.
3. **Change map if needed.** For the party the new map becomes pending (see
   the main loop) and the party's map position is updated with 0x1C82D.
   Creatures that cannot enter the new map (0x1F9FF) are destroyed or
   dropped.
4. **The party meets a creature group:** the group is pushed to a free
   adjacent square (0x4AFCD). If none is free the party takes damage, the
   hurt sound plays and the party is moved back.
5. **A creature group meets the party:** the creature is placed beside the
   party, or rescheduled for 5 ticks later (event 0x3C, via 0x4A096).
6. **Sensors:** run the floor sensors (0x4CDCC) on leaving and on entering.
7. **Place the thing** in the square's list (0x1D3DB).
8. **Moving creatures** also update their timeline event's map, x and y.

## Map transitions (code)

**Pending map.** A party move onto another map does not switch
immediately. The move routine records the destination map in 0x717F4.
At the start of the next tick the main loop runs 0x24629:

- stop interface updates (0x14C55) and run the leave pass over the old
  map (0x59785(0));
- set the party's map position (0x1C82D);
- load the new map's graphics set (0x3AB31);
- run the enter pass over the new map (0x59785(1)), then activate its
  creatures (0x34106);
- redraw the whole viewport.

It then places the party with the move routine. The leave/enter steps are
skipped while 0x7FA80 is set.

**Map leave/enter pass (0x59785, code; `map_entry.rs`).** Scans every
square of the party's current map, column by column, and on squares with
things looks only at the leading things of types 0-3 (it stops at the
first thing of a higher type):

- An actuator of type 0x21 fires its target (0x4BC4C) with a value from
  word 2. If bits 3-4 are both set, the value is 1 when bit 5 equals the
  pass (1 entering, 0 leaving) and 0 otherwise. Otherwise the actuator
  only reacts to one direction (bit 5 clear means entering) and fires
  with value bits 3-4.
- An actuator of type 0x2C with word 2 bit 0 set restarts its ornament
  animation on entry (0x56CF4 / 0x56D6A; not yet in the remake).
- A text thing in mode 2 (word 1 bits 1-2) whose type field (bits 11-15)
  is 0x15 is a first-entry creature spawn. On entry, if word 1 bit 0 is
  clear, it sets the bit, draws `rand4()` for the facing and creates a
  creature of type word 1 bits 3-10 on that square through 0x30BA6 (level
  7, so base hit points plus `random(base/8 + 1)`). The shipped dungeon
  has these on maps 2, 3, 4, 5, 6, 9 and 41.

Because the pass also runs at game start, a save must never leave the
party on a map with an unfired first-entry spawn (see `12-savegame.md`,
"Loading sequence").

**Teleporting the party (0x4BED2).** Takes (x, y, map, facing). It validates
the coordinates against the target map. If the map differs, it takes the
party off the current map, arrives on the target map (0x24629), places the
party, and finally sets the facing.

**Map edges and teleporter links (0x1D074 / 0x1D113).** A teleporter square
doubles as an edge link to a neighbouring map. 0x1D074 checks the square
holds a teleporter and an actuator of type 0x27; only then is it a link, and
its direction is the teleporter's rotation (word 1 bits 10-11) + 2. A square
action on that actuator toggles its own word 2 bit 0. The party move uses the
link when the destination square is one, the party is not standing on
stairs, and the partner's direction + 2 differs from the party's facing.
The record gives the destination:

- x from word 1 bits 0-4;
- y from word 1 bits 5-9;
- map from word 2 bits 8-15.

0x1D113 also checks that the destination is itself a valid link, which
keeps two-way seams consistent.

## Falling through pits (party, 0x4A34A)

When the party's move ends on an open pit, the move routine loops:

1. **Fall one layer:** go to the map one layer down at the same world
   position (0x1CC7E with +1) and count one more level fallen.
2. **Stop condition:** the loop stops on a non-pit square. The map set's
   attribute (8, set, 11, 0x6A) switches pits to random destinations (see
   "Random pits" below).
3. **Animation:** while falling (and not climbing down on purpose), each
   intermediate level redraws the view (0x54B3F, then 0x138D9), so the
   player sees the fall.

On landing:

- **Damage path:** the fall damage goes through the champion damage routine
  (0x4722A) with parts mask 0x30 (legs and feet) and attack type 2 (0x4AAE7).
  The remake used type 0. Checked in DOSBox with a positioned save stepping
  into the pit on map 4: both land on map 7 at (5,11) facing north. The
  apparent 31-against-37 difference was the step landing on different
  ticks: the original's draw log puts its fall roll at tick 1707. With the
  remake's step on that tick, both roll a base of 17 and deal 32 damage to
  the legs and feet (defence 7: leg armour 9, foot armour 12, ninja level
  2), and both show 51/83 health after a point of regeneration, with the
  same food and water.

- **Normal fall:** every living champion takes `(min(max health / 4,
  17) + rand4()) × levels fallen` damage through 0x4722A, to the legs and
  feet (wound mask 0x30), with attack type 2, and plays the champion
  sound (22, champion, 0x87).
- **Deliberate descent** (flag 0x8002A, set by the action code 0x414A5
  when the party climbs down into a pit, for example with a rope): no
  damage. Instead each living champion loses `current load · 25 /
  maximum load + 1` stamina (0x47707).

## Missile flight (event 0x1D / 0x1E, 0x17A7B)

Every flying missile owns one timeline event, rescheduled one tick ahead
each time it runs, so missiles move once per tick. The event's +6 holds
the missile thing; +8 packs x (bits 0-4), y (5-9), direction (10-11) and
the **step energy** (12-15). The missile record holds the kinetic energy
(byte +4), the damage energy (byte +5) and the event index (word +6).

Each tick:

1. **Launch tick:** type 0x1D only appears on the first tick. It is
   changed to 0x1E and the hit test on the launch square is skipped, so a
   missile never hits its own thrower.
2. **Hit test on the current square:**
   - If a creature is there, its type has flag 0x02 (spell-reflecting) and
     the missile is a spell (thing value ≥ 0xFF80): the missile is
     deflected. The new direction comes from a table at 0x716A4 indexed
     by direction, cell·4 and the creature's facing parity·16 (value 4
     means no change).
   - Otherwise, if the party stands there, try a party hit (0x1726B mode
     −3); then try creatures and objects (mode −1). Any hit ends the
     step; the impact code removes or explodes the missile.
3. **Energy:**
   - If kinetic energy ≤ step energy, the missile stops: it is removed
     from the flight list and lands or bursts in place (0x16C0C).
   - Otherwise kinetic energy −= step energy, and damage energy −= step
     energy (not below 0).
4. **Advance:** a square has four cells. A missile in one of the two
   cells on its leading side (cell = direction, or direction + 1) moves to
   the next square; otherwise it moves to the leading cell of the same
   square.
   - **Into a blocked square:** if the next square is a wall, a closed
     trick wall, or stairs while the current square is also stairs, the
     missile impacts on the current square (0x1726B with the square type).
   - **Bounce:** a cloud of kind 0x0E in the next square reverses the
     direction instead and the missile stays put.
   - **New cell:** `cell − 1` if direction and cell have the same parity,
     else `cell + 1`, modulo 4.
5. **Moving:**
   - **Within the same square:** a door square triggers a door hit
     (0x1726B mode 4); then the thing is relinked at its new cell.
   - **Into the next square:** the general mover (0x4B108) handles
     teleporters, pits and map changes; the final position comes back in
     0x80020-0x80026.
   - **Waking creatures:** a creature on the destination square, and one
     on the square beyond if the destination isn't a wall, closed door or
     closed trick wall, is notified (0x24E62 with code 0x2006). That is how
     creatures notice incoming missiles.
6. **Reschedule:** the event goes back on the timeline for the next tick
   and its new index is stored in the missile record.

## Pits, teleporters and stairs in a move (0x4A34A, code)

The destination resolver loops up to 50 times, starting at the target
square:

- **Active teleporter** (square bit 3). The record's word 1 bits 13-14 are a
  scope mask, tested against the mover class: party 2, creature 1 or 2 (2 if
  its info has attribute 0x1E), anything else 3. Scope 1 accepts creatures
  only. Otherwise the move passes when the class is 3 or `scope & class` is
  non-zero. Destination: word 1 bits 0-4 x, bits 5-9 y, word 2 bits 8-15
  map. Rotation is word 1 bits 10-11, absolute when bit 12 is set. The party
  turns. Items rotate their cell unless rotation is absolute. Creatures and
  missiles have their own routines. Bit 15 plays the teleport sound. A
  teleporter that targets itself ends the loop.
- **Open pit** (bit 3 set, bit 0 clear). Anything not airborne (0x49FCB)
  falls to the same global square one layer down (0x1CC7E with +1). The fall
  counter increases. Each living champion takes
  `(min(max HP / 4, 17) + rand4()) × falls` with attack type 0x30, so a
  second level in one move hurts twice as much. A fallen creature takes 20.
  **Random pits.** If the map's graphics set has attribute (8, set, 11,
  0x6A) and the faller is not a creature, the pit can send it elsewhere.
  The pit square holds a marker text thing: word 1 with bits 1-2 equal to 1,
  kind 0x0C in bits 11-15, and an id in bits 3-10. 0x4D88A in counting mode
  counts every kind-0x0B marker with the same id in the whole dungeon
  (maps in order, squares column-major, things in list order); one
  `random(count)` call picks r, and the same routine in search mode returns
  the map and square of the (r + 1)-th marker. The faller goes there, on any
  map, and the move continues from that square. This jump does not count
  as a fall level, so it adds no fall damage of its own. The shipped
  dungeon uses it (for example a pit on map 38).
- **Stairs**, for things other than the party, creatures and missiles: the
  item goes down a layer if stairs bit 2 is clear. It then moves one square
  in the stairs' exit direction (0x1CE6F) and its cell turns to match.
- Anything else ends the loop.

## Floor sensors (0x4CDCC, code)

Called by the move routine for a mover (the party or a thing) leaving or
entering a square. For a thing, the routine removes it from the square's
list before scanning when leaving, and adds it after scanning when
entering. The scan therefore sees only the other occupants:

- creatures (not airborne): "creatures present";
- items and other things of type 5-13: "items present", plus "matching
  present" if one has the mover's item number, and "other present" if one
  doesn't;
- on a party entry (unless the party is being re-placed on its own square),
  visible text things in mode 0 are shown.

If the square is a wall (things pushed into a wall alcove), only things on
the mover's cell count, and different actuator types apply.

Then every actuator on the square is checked; the walk stops at the first
thing of type 4 or above. The "sensed state" starts as "entering".

| Type | Fires when |
|------|------------|
| 1 | Nothing else counts: no party already there, no items, no creatures |
| 2 | The mover is the party or a creature, and no party or creatures remain |
| 3 | Party only, with at least one champion. With *data* 0: as type 1 for the party. With *data* = 1 + direction: the state is "entering while facing that way" (leaving reports a clear). |
| 4 | The mover's item number equals *data* and no other matching item remains |
| 7 | The mover is a creature and no creatures remain |
| 8 | Party only: the state is whether the party carries item *data* (0x4BD6C) |
| 0x29 (wall) | No items remain on the cell |
| 0x2A (wall) | The mover is item *data* and no other matching item remains |
| 0x2B (wall) | The mover is not item *data* and no other item remains |
| 0x1A (wall) | Alcove compare against an ornament attribute; not yet written up |

Firing: the state is XORed with the inverted bit. In "follow" mode the
action becomes set for a true state and clear for false. In other modes a
false state does nothing. Word 2 bit 6 plays a sound. The actuator then
fires its target (0x4BC4C).

Text things in mode 1 also act as sensors. Kind 9: when the party enters
or leaves a square it isn't already on, one `random(100)` roll is made,
and if it is below word 1 bits 3-10 and the text's bit 0 differs from
"entering", the square gets a set action next tick and a clear 5 ticks
later. Kind 10: the party can be pushed back by a delayed teleport (event
0x5D), with a chance of `min(90, 10 × load term + 25 or 50)`%; this is still
to be modelled.

## Wall sensors (0x4C134)

Clicking a wall cell, possibly holding an item, checks the actuators on
that cell:

| Type | Behaviour |
|------|-----------|
| 1 | Any click fires (not allowed in follow mode) |
| 2 | Fires when "hand empty" differs from the inverted bit, i.e. normally when holding something; follow mode sends that state |
| 3 | Fires when "holding item *data*" differs from the inverted bit; with word 2 bit 2 the item is consumed |
| 0x15 | Like 3 but only for items with charges left (0x1F606); an empty item counts as not matching |
| 0x17 | With an empty hand: toggles word 2 bit 2, and fires when that bit differs from the inverted bit |
| 0x18 | Push button with a cooldown. With an empty hand and word 2 bit 0 (busy) clear: set busy, schedule event 0x57 (re-arm, actuator in bytes 6-7) at tick + *data* + 2, and fire (set in follow mode). Inverted buttons fire with 16 extra ticks of delay. Types 0x4A and 0x46 on a door with a flag at +3 bit 0x20 take the same path. |
| 0x1A | Alcove for one item kind: the wall ornament's attribute (9, ornament, 11, 0x0E). With word 2 bit 2 clear, holding that kind puts the item into the wall on that cell. With bit 2 set and an empty hand, it hands over an item of that kind from the cell (0x4BD17), or creates a new one (0x1DE8E) with its charges set to full when there is none. No actuator fires. |
| 0x1B | Receptacle: when *data* is non-zero and the held item is the ornament's kind, the item is consumed and *data* decreases; on reaching 0 the actuator marks itself busy and fires. |
| 0x1C | With an empty hand and word 2 bit 2 clear: move the party to the target square (0x4BED2), facing word 2 bits 3-4 (absolute when inverted, otherwise added to the party's facing), then fire. |
| 0x3F | Clears the busy bit when clicked with an empty hand; it fires nothing except in follow mode (where it sends set) |

## Open questions

- The original's per-tick random draws (about 37.7 per tick when idle, tied to game ticks) and its startup draws (about 120 before the recruit and about 140 more before the first tick); see "Idle draw rate" and "Open lead: creature animation" above.

- The charges that alcove 0x1A gives a newly
  created item (the engine leaves them at the default).
- Event types 0x0E, 0x46 and 0x5A (the repeating ornament sound), and the
  meaning of door/teleporter bit 13 toggled by actuator 0x46.
- The meaning of the 512-tick (0x39044) and 64-tick (0x47CC3) periodic
  calls.
