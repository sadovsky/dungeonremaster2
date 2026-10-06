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

| Code | Substitution (global it reads) |
|------|--------------|
| 0 | Number from the word at 0x7F214 |
| 1 | Number (0x3FF − packed value at 0x760C1) >> 10 |
| 2 | Text (1, 0xFE, 5, 0) |
| 3, 4 | Fixed strings through pointers at 0x70536 and 0x7053A (drive or disk names) |
| 7 | Name of the champion whose index is at 0x7F988 (nothing when it is −1) |
| 8, 9 | Fixed strings through 0x7053E and 0x70542 |
| 10 | Number from the word at 0x7F996 |
| 11 | Number from the word at 0x7F98A |
| 12 | Number from the word at 0x7F994 |
| 13 | Number from the word at 0x7F992 |
| 14 | Number from the word at 0x7F986 |
| 15 | Code 3 when the word at 0x7F98E is 1, code 4 when it is 2, else nothing |
| 17 | Text (7, 0, 5, n) with n the byte at 0x7F990 (an interface word such as a class name) |
| 20, 26, 28 | Data-directory prefixes (28 picks 26 or 20 depending on the second graphics file) |
| 22 | Save-directory prefix, which depends on a flag at 0x7054A |
| 23 | A number from 0x7F998 formatted with a stored prefix |
| 24 | An entry from the pointer table at 0x7574A, indexed by the word at 0x75748 |
| 25 | Number from the word at 0x760C3 |
| 27 | Text (1, 0xFE, 5, 6) |
| others | An empty string |

Callers fill the context words before fetching the text. Two traced examples:

- **Item details (0x3962A):** the item's weight in kilograms goes in
  0x7F996 (code 10) and its tenths in 0x7F98A (code 11).
- **Load line (0x48890):** the champion's load in kilograms goes in 0x7F994
  (code 12), its tenths in 0x7F992 (code 13) and the maximum load in whole
  kilograms in 0x7F986 (code 14); then text (7, 0, 5, 0x2A) is drawn at
  layout id 0x22B, in one colour above the maximum, another above 5/8 of
  it, and a third otherwise.

The engine's `font::TextContext` mirrors these words (codes 0, 7, 10-14,
17 and 25, plus the fixed texts 2 and 27). The drive, disk, directory and
save-slot codes only appear in the DOS file dialogs, which the remake does
not have, so they expand to nothing.

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
- Type 14 strings and the 16-byte type 13 entry.
