# Text in GRAPHICS.DAT

DUNGEON.DAT carries almost no text (28 words). Nearly all strings the
player sees, plus several small data languages, are type-5 entries in
GRAPHICS.DAT. Inspect them locally with
`tools/gdat.py text [en|de|fr|editor|all] [category]`. The output is
for inspection only and must not be committed.

## Storage

- Key: (category, index, 5, sub-index). There can be up to four
  records with the same key that differ only in the high nibble of F:
  0x10 English, 0x30 German, 0x40 French, 0xF0 editor label. Records
  with a high nibble of 0 are language-neutral. The index builder keeps
  only the neutral records and those matching the configured language,
  so at runtime each key resolves to exactly one string.
- Obfuscation: when bit 0x08 of the archive flags (0,0,11,0) is set
  (it is, since the flags are 0x7B), each stored byte at position *i*
  must be decoded as `plain = (~stored - i) & 0xFF`. The decode runs
  over the whole entry, NUL included.
- Strings are NUL-terminated, upper-case ASCII. A line break is `\n`
  (0x0A). The German and French texts fold accented letters to plain
  ASCII.
- Fetch routine: 0x3A921 `(category, index, sub-index, out_buffer)`. It
  checks that the entry exists, copies it, de-obfuscates it and then
  expands escapes with 0x3A6AB into the caller's buffer.

## Escape codes (expander at 0x3A6AB)

There are two spellings that do the same job:

- `0x01, c`: code = c − 0x20, so `0x01 '''` is code 7 and `0x01 '/'` is code 15.
- `.Z` followed by three decimal digits. Data-file names use this form
  (`.Z020GRAPHICS.DAT`).

Each code is replaced by a string, which is itself expanded recursively.
What the codes substitute, as far as is known:

| Code | Substitution |
|------|--------------|
| 0 | A number (decimal) from a global, e.g. a count in the current message |
| 1 | A number derived from a 10-bit packed global |
| 2 | Another text entry: (1, 0xFE, 5, *n*), where *n* comes from the current context |
| 3, 4 | Fixed strings (pointers at 0x70536 and 0x7053A), probably drive or disk names |
| 7 | The current champion's name (champion records are 0x107 bytes, starting at 0x7FBD0) |
| 8, 9 | Fixed strings (0x7053E and 0x70542) |
| 10 to 14, 25 | Numbers from various context globals |
| 15 | Chooses code 3 or code 4 depending on a mode flag |
| 17 | Text from category 7 (interface words) with sub-index from context, e.g. a class name |
| 20, 22, 26, 28 | Directory prefixes for data and save files (`DATA\` and the like) |
| 23 | A number formatted with a stored prefix |
| 24 | An entry from a pointer table at 0x7574A (probably the save-slot or drive name) |
| 27 | Fixed text (1, 0xFE, 5, 6) |
| others | An empty or default string |

The code numbers are reliable. The descriptions of the context globals
are working guesses until the callers are traced.

## Data languages stored as text

Several language-neutral text entries are small data formats, not
messages.

### Action strings (items and champions, sub-indices 8 to 11)

Each weapon, item or bare-hand slot has up to four actions. Each is
`NAME:` followed by a run of two-letter codes, each with a signed
decimal number, for example (made up) `SLASH:CM5SK7LV2BZ9TR3TA4EX6PB40DM30`.

These codes appear across categories 16 to 22 (count of uses):

| Code | Uses | Probable meaning (to be confirmed from the parser) |
|------|------|----------------------------------------------------|
| CM | 234 | Command or action ID: which game routine runs |
| BZ | 199 | Busy time (ticks before the champion can act again) |
| EX | 167 | Experience awarded |
| SK | 166 | Skill trained or used |
| TR | 156 | Required or trained? |
| TA | 143 | Target modifier (often negative) |
| PB | 106 | Probability / to-hit |
| DM | 106 | Damage |
| LV | 72 | Minimum skill level to show the action |
| SD | 43 | Sound to play |
| ST | 36 | Stamina cost |
| WH | 32 | Which slot or pouch (containers that hang on the belt) |
| NC | 32 | Number of charges used |
| PA | 15 | Unknown |
| RP, HN, AT | 3 each | Unknown |

Some action names start with letters separated by spaces. These look
like rune symbols in the game font that spell the spell an item casts.

### Digit strings (wall and floor ornaments, sub-index 13)

Strings of decimal digits, which look like animation frame sequences
for animated ornaments.

### Environment commands (category 23)

Lower-case two-letter codes, each with a number, e.g. (made up)
`cd6000xl48yl32`: probably a draw or animation command per environment
image: `cd` an image or command id, `xl`/`yl` a position, `mv`, `fd`
and `fw` movement and fade parameters. Not yet confirmed.

### Creature attribute strings (category 15, sub-indices 16 to 46)

Short neutral strings such as `C0` and `C0-60`. Their purpose is unknown.

### Container contents (category 20, sub-index 64)

Space-separated tokens such as `J26-28` (made up), probably item-generation
rules for a container (the letter selects an item class, then an index
or a range).

## Open questions

- Find the action-string parser (search for the two-letter codes as
  16-bit constants) and confirm the meaning of each code.
- Where the language byte at 0x7576C is set (probably from a config
  file or the installer).
- Confirm the context globals behind the escape codes.
- Type 14 strings and the 16-byte type 13 entry.
