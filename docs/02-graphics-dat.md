# GRAPHICS.DAT

An archive of numbered entries plus a metadata index that maps four-part
keys to entries or to small numbers. Reader: `tools/gdat.py`. The Rust
port will live in `crates/dm2-formats/src/gdat.rs`.

Loader functions in SKULL.EXE (Ghidra names; see `01-executable.md`):

| Address | Role |
|---------|------|
| 0x3D111 | Open GRAPHICS.DAT (and GRAPHIC2.DAT if the main file is short), reference-counted. Error 0x29 or 0x1F if the open fails. |
| 0x3D1C8 | Read the header and size table, build the entry table, load the index, read a few global settings |
| 0x3CE0A | Parse the metadata index (entry 0) |
| 0x3C9AA | Build the in-memory lookup tables from the index |
| 0x3C4F2 | Look up a key and return a pointer to its 4-byte record |
| 0x3C5D3 | Look up a key and return its value (0xFFFF or 0 if missing) |
| 0x3CD67 | Preload entries flagged as resident (G = 0xFF) |
| 0x3C4A5 | Read field *n* of index row *r*, big-endian |
| 0x3BEF6 | Read bytes at an archive offset, splitting across the two files when needed |

## File layout (version 3 and later; this file is version 5)

All header integers are little-endian.

| Offset | Size | Field |
|--------|------|-------|
| 0 | u16 | Signature: bit 15 set; bits 0-14 are the version. Accepted versions are 2, 4 and 5; this file has 0x8005. |
| 2 | u16 | Entry count N (5624) |
| 4 | u32 | Size of entry 0 (the metadata index), 94,852 |
| 8 | u16 × (N−1) | Sizes of entries 1 to N−1 |
| 8 + 2(N−1) | ... | Entry data, back to back in entry order, starting with entry 0 |

Every entry is under 64 KB except entry 0. The end of the last entry
equals the file length exactly (verified). If the file is shorter than
the total, the loader takes the remaining entries from `GRAPHIC2.DAT`
(the first file's length acts as the split point). This release has a
single file.

In versions below 3 the table holds N u16 sizes, with entry 0 included.

In memory the game keeps one u32 per entry: the size with bit 31 set
while the entry is on disk, replaced by a pointer once it is loaded.

## Metadata index (entry 0)

The index is big-endian. The loader checks for a 0x8001 marker and
byte-swaps the two counts if it finds it reversed, which suggests the
same index format was shared with big-endian (Amiga/Mac-era) tools.

| Offset | Size | Field |
|--------|------|-------|
| 0 | u16 | Marker 0x8001 |
| 2 | u16 | Record count R (11,854) |
| 4 | u16 | Field count K (7) |
| 6 | 2 × K | Field descriptors: an ASCII letter, then a width in bytes |
| 6 + 2K | R × width | Records; each field is a big-endian integer |

The field letters in this file, in order: `T`1 `I`1 `D`1 `S`1 `F`1 `G`1 `P`2,
giving 8-byte records. The game reads fields by letter, not by position,
using the order string `TIDSPFG`, so the layout in the file is self-describing.

| Letter | Meaning (working names) |
|--------|-------------------------|
| T | Category (0 to 28), e.g. wall set, creature, item, interface |
| I | Index within the category |
| D | Type of data: 1 = image (most common), 11 and 12 = numeric attribute, others still to be identified |
| S | Sub-index (frame, view distance, variant ...) |
| F | Unknown; common values 0, 8, 16, 48, 64, 240 |
| G | 0 normally; 0xFF = keep resident (preloaded at startup); 1 = sets bit 15 of the value |
| P | Value: an entry number (masked with 0x7FFF) unless D is 11 or 12, which store a raw 16-bit number |

The lookup table built at startup covers categories below 0x1D (29) and
types below 0x0F (15). For each (category, type) pair it holds the 4-byte
items (I, S, value) sorted by (I, S), and it is binary-searched.

### Records per (category, type)

Generated with `tools/gdat.py stats`:

```
cat\type      0     1     2     3     4     5     7     8     9    11    12    13    14
       0      1                                                     2
       1           81                 1    28     4           1                 1
       3                  7                30                       1
       4                       29
       5            2
       6            1
       7           64                      93
       8          369     1                       8                44    21
       9          696    24                 4           1         402    92
      10          468     8                 2                     130    83
      11           12                                               2                 1
      12            2
      13           93    16                                        25    12
      14           32     3                                        54                 3
      15         3095   192               165   228    76         352  1551
      16          197     6               468                     277
      17          132     2               228                     284
      18            4     2                 3                       5
      19           50     2                75                      55     1
      20           85     2                39                      24     5
      21          145     2               249                     233
      22           32    21               208          16
      23           78     3                38
      24            8     1
      26           30                     231
```

## Global settings read at load

- Key (0,0,8,0): if it exists and the archive version is 2 or more (but
  not 4), it gives the size of a resident table, which is loaded and
  stored in 4-byte units. Meaning not yet known.
- Key (0,0,11,0): a flags word. Bit 0x20 is copied to a global; bit 0x40
  selects a cache-slot limit (31 instead of 1000) and runs a hook.
  Probably about memory or EMS.

## Open questions

- Encoding of the remaining non-image types (4, 9, 13). Images are done: see "Image encoding".
- What the categories are (to be found by tracing callers of 0x3C5D3 with constant keys).
- What F means.

## Categories and types

Worked out from the constant keys passed to the lookup functions, from
how their results are used, and from the entries themselves. Run
`tools/gdat.py categories` for the labelled table with counts.

### Types (D)

| D | Meaning | Notes |
|---|---------|-------|
| 0 | Archive tag | One record (0,1,0,0): an ASCII build stamp from the tool that packed the archive (dated 1995). |
| 1 | Image | Header is width and height (low 10 bits each). See "Image encoding". |
| 2 | Digital sound | 6-byte header: u16 sample rate (11127 in every entry), u8 bits (8), u8 channels (1), u16 0; then unsigned PCM. 292 entries. |
| 3 | Music | HMI HMP files (start with `HMIMIDIP`). Category 4 holds all 29; probably indexed through `SONGLIST.DAT`. |
| 4 | Offset-indexed table | A single entry (1,0,4,0) of about 17 KB: a count followed by a table of u16 offsets. Not decoded yet. |
| 5 | Text | Obfuscated, NUL-terminated, with escape codes. See `13-text.md`. F carries the language. |
| 7 | Raw data blob | Read as untyped bytes. Examples: (1,0,7,0) is a 768-byte VGA palette (6-bit components); (1,0,7,1) and (8,5,7,n) are 256-byte colour remap tables; creature sub-indices 252, 253 and 254 are per-creature animation or offset tables; (5,0,7,4) is the title screen's hotspot data. |
| 8 | u16 table | Small arrays of little-endian words. Creatures: sub-index 251 holds frame or sequence lists. Champions: sub-index 0 is a 52-byte record of starting stats (26 words). Map sets: images 0 and 1 also have a key under type 8. |
| 9 | Data | (1,0,9,254): 1,024 bytes, apparently 256 × 4-byte entries (a colour lookup?). |
| 11 | Number | Not an entry reference. The value is a raw 16-bit attribute, keyed by (category, object index, 11, attribute number). Missing keys read as 0. This is how the game stores per-object properties for creatures, items, ornaments, doors, missiles and so on. |
| 12 | Number | Same as 11, but a second, separate attribute namespace (bit 15 is kept). Used, for example, as the fallback when an image's header holds no size. |
| 13 | Data | (1,0,13,254): 16 bytes, not decoded. |
| 14 | Short string table | 16 characters plus NUL, made of digits and letters: look-up strings, e.g. for missiles (14,10,14,112-114). |

Types 6 and 10 aren't used in this archive.

### Categories (T)

| T | Contents | Evidence |
|---|----------|----------|
| 0 | Archive information | Build stamp; (0,0,11,0) is the global flags word (0x7B here; bit 0x08 turns on text obfuscation, bit 0x20 and bit 0x40 control memory and caching); (0,0,8,0) is a resident table loaded at startup. |
| 1 | Interface and global resources | Main palette and remap tables (type 7), interface images, the generic text "message" group at index 0 (level gained, champion awakened ...). Index 0xFE holds fallback text used by escape code 2. |
| 3 | Wall writing | Texts with `\n` line breaks, shown on walls; index 0, sub-index = message number. |
| 4 | Music | 29 HMP songs, index = song number. |
| 5 | Title screen | 320×200 image (sub-index 1) plus hotspot data (type 7, sub-index 4); used by the start-menu loop at 0x386F5. |
| 6 | Another full-screen 320×200 image | Not traced yet. |
| 7 | Interface strings and panels | Class names, skill-level titles and other UI words (text, index 0); 224×136 viewport-sized panels. |
| 8 | Map graphics set | Indexed by the current map's graphics-set number (global at 0x75BFE). Walls, floor and ceiling pieces, colour tables, and many type 11/12 attributes (sub-index 0x64 and up) that describe the set. |
| 9 | Wall ornaments | About 108 ornaments (alcoves, switches, fountains, etc.). Attribute numbers 0x0A, 0x0B, 0x0C, 0x0E, 0x13 and 0x63 come up in the actuator and drawing code. Some have digit-string texts (sub-index 13) that look like animation frame sequences. |
| 10 | Floor ornaments | Same pattern as 9 (attributes 4, 5, 7, 0x11, 0x60, 0x61, 0x63). |
| 11 | Door ornaments (tentative) | 12 images of different sizes. Attributes 4 and 8 are read together with category 14 in 0x5346E. |
| 12 | Unknown | Two 8×9 images; attribute 8 (× 5) feeds a sound or effect call at 0x530D1. |
| 13 | Doors | 13 door types; attributes 0, 1, 0x41 and 0x42; door sounds. |
| 14 | Missiles and spell effects | Images shrinking with distance (96×88, 64×61, 44×38 ...). Attributes 0x0D to 0x11 are read by small wrappers at 0x1FE50 to 0x1FEB5. |
| 15 | Creatures | 76 creature types (index 0 to 87). Images (frames by sub-index), sounds, animation tables (type 7, sub-indices 252 to 254), sequence lists (type 8, sub-index 251) and about 1,900 attributes. The name text has F = 0xF0 (editor labels, never shown). |
| 16 | Weapons | Name (text sub-index 24), action strings (sub-indices 8 to 10), icons and in-hand images, attributes. |
| 17 | Clothing and armour | Same layout as 16. |
| 18 | Scrolls | Same layout. |
| 19 | Potions | Same layout. |
| 20 | Containers | Same layout, plus extra text (sub-index 64) and images for the open container panel. |
| 21 | Miscellaneous items | Same layout. Key (21, 0xFE, 1, 0xFE) is the generic fallback image used when any image lookup fails (wrappers at 0x3EC2B, 0x3EE1D, 0x3F422, 0x3F49D). |
| 22 | Champions | 16 champions: portraits (sub-indices 0 and 1), names (text sub-index 24), bare-hand action strings (sub-indices 8 to 11), voice sounds, starting stats (type 8). |
| 23 | Map environment set | Indexed by the same map graphics-set number as category 8. Images (e.g. 224×29 strips), an ambient sound, and short command strings (see `13-text.md`) at sub-indices 0 to 5 and 99 to 108. Probably sky and outdoor or weather effects. |
| 24 | Unknown | 8 images (36×49, 83×49, 8×52 ...) and a sound. |
| 26 | Dialogs and system messages | Disk, save and load messages, menu captions, and 224×136 dialog backgrounds. Text index selects the dialog; sub-index selects the line. |

Categories 2 and 25 are empty.

### F and G fields

- **F, high nibble: language filter.** When the index is built, a
  record is kept only if `F & 0xF0` is 0 (language-neutral) or equals the
  current language byte (global at 0x7576C). This is done by the callback
  at 0x3CD2F passed to 0x3C9AA. Values: 0x10 English, 0x30 German,
  0x40 French. 0xF0 never matches, so those records are editor-only
  labels (creature names, debug map names). Text has three translations
  for most strings (540 each); images use F = 8 or 0.
- **F, low nibble.** Usually 8 on images and 0 on attributes. Not used
  by the filter. Meaning unknown (maybe a "has data" or "compressed"
  marker).
- **G.** 0 normally. 0xFF means the entry is loaded at startup and stays
  in memory (0x3CD67; 262 records). 1 sets bit 15 of the value in the
  lookup table.

### Lookup helpers (SKULL.EXE)

| Address | Purpose |
|---------|---------|
| 0x3C5D3 | Look up a value. Images and other references return the entry number, or 0xFFFF if missing. Types 11 and 12 return the number, or 0. |
| 0x3C92E | Check whether a key exists (and, for references, whether the entry is usable) |
| 0x3ED8A / 0x3ECC7 | Get a pointer to an entry's data, loading it if needed. 0x3ECC7 falls back to (21,0xFE,1,0xFE) for missing images. |
| 0x3EDF0 | Copy an entry into a caller's buffer |
| 0x3F573 | Get an entry's size |
| 0x3F5B0 | Get an image's width and height |
| 0x3EC2B, 0x3EE1D | Get a decoded image (cached) |
| 0x3F422, 0x3F49D | Play a sound entry (via 0x3BE1A) |
| 0x3A921 | Fetch and expand a text entry (see `13-text.md`) |
| 0x3AADD | Read an attribute word (category, index, 11, n) holding two packed byte-sized effect codes |
| 0x1F12F, 0x1FCEC, 0x1FD74, 0x1FDBB, 0x1FDF0 | Wall-ornament attributes (9, x, 11, n) |
| 0x1FE50 to 0x1FEB5 | Missile attributes (14, x, 11, 0x0D to 0x11) |
| 0x4DB9B | Map-set attribute (8, set, 11, 0x65) bit 0x20 |

## Image encoding

Decoders: `tools/gimg.py` (Python, used by `tools/gdat.py export`) and
`crates/dm2-formats/src/image.rs` (Rust). Both decode all 4,031 distinct
image entries in this archive.

### Header

Every type-1 entry starts with two little-endian words:

| Word | Bits 0-9 | Bits 10-15 ("tag") |
|------|----------|--------------------|
| 0 | width | Width tag. Ignored by the decoder. Usually 0; 32 (bit 15) on 708 images (687 compressed 8-bit, 20 4-bit, 1 uncompressed); a few odd values on 4-bit images. Meaning unknown, perhaps a drawing or mirroring hint. |
| 1 | height | Height tag, which selects the encoding (below). |

The game masks both words with 0x3FF wherever it needs the size (0x3F5B0).

| Height tag | Encoding | Count here | Game decoder |
|------------|----------|------------|--------------|
| 31 | Compressed 8-bit image | 3,167 | 0x3F71C dispatches on byte 6 |
| 32 | Uncompressed image with a 10-byte header | 6 | none (used in place) |
| anything else | Nibble-RLE 4-bit image | 858 | 0x12E4F |

The tag value that means "8-bit" is not a constant. It is the global at
0x7FAC8, which is set to 31 when bit 0x40 of the archive flags word
(0,0,11,0) is set (it is here) and to 1000 otherwise. A value of 1000
can't occur in a 6-bit tag, so archives without that flag hold only 4-bit images.

The load-and-cache routine is 0x3E867. It picks the decoder, allocates
the decoded buffer (4-bit images stay packed two pixels per byte, with
rows rounded up to an even pixel count) and, for 4-bit images, copies
the entry's last 16 bytes after the pixels as the image's colour map.

### Nibble-RLE 4-bit images (0x12E4F)

The pixel stream is read one nibble at a time, high nibble first,
starting at byte 4 (just after the header).

1. Read six nibbles: a local table of the six most common colours.
2. Repeat until `width × height` pixels have been written:
   - Read a command nibble. Bits 0-2 are the operation; bit 3 says a run
     count follows (otherwise the count is 1).
   - Operations 0 to 5: draw the colour at that position in the local table.
   - Operation 6: copy pixels from the row above (offset −width).
   - Operation 7: the colour is the next nibble. **That colour nibble
     comes before the run count.**
   - Run count (0x12DDB): read nibble *n*. If *n* < 15 the count is *n* + 2.
     Otherwise read two nibbles as a byte *b*; if *b* < 0xFF the count is
     *b* + 17, otherwise read four nibbles as a 16-bit count (used as is).
   - Runs continue across row ends. The game's output buffer has an even
     row stride, so for odd widths it skips the padding pixel at each row
     end (0x12E4F has a second loop for this case); the logical stream
     contains only visible pixels.
3. The entry's last 16 bytes are the colour map: nibble value → palette
   index. In every 4-bit entry here the stream ends exactly where the
   colour map begins (verified for all 858).

Run helpers: 0x12BD7 sets one pixel, 0x12C1D fills a run, 0x12CE3 copies
a run from an earlier position, and 0x12D7A reads one nibble.

**Delta variant (0x13258).** The format is the same, except the table has
five colours and operation 5 means "keep the pixel from a base image".
The game uses it when entry 0's resident table (key (0,0,8,0)) maps an
entry to a base entry (lookup 0x3C636, a binary search over pairs of
(entry, base) words). That key is missing from this archive, so no
delta images exist here; the decoders support them anyway.

### Compressed 8-bit images (tag 31)

| Byte | Meaning |
|------|---------|
| 0-3 | Width and height words |
| 4-5 | Two bytes; non-zero in 1,514 of 3,167 images, mostly creatures (1,115), wall and floor ornaments and map-set pieces. Probably a signed x/y origin offset used when positioning the image; not traced yet. |
| 6 | Compression format, an index into the function table at 0x75798 |
| 7 | Always 0 here |
| 8- | Compressed data (length = entry size − 8). It decompresses to exactly width × height palette indices, row-major, with no padding. |

| Format | Algorithm | Game routine | Used here |
|--------|-----------|--------------|-----------|
| 1 | LZW in Unix `compress` style (9- to 12-bit codes packed LSB-first; code 256 resets the dictionary; codes are read in groups of eight, and the rest of a group is dropped when the code width changes or the dictionary resets), then 0x90 run-length expansion (0x90 *n* repeats the previous byte to make *n* copies; 0x90 0 is a literal 0x90) | 0x5AADA, code reader 0x5A95B, buffers set up by 0x5ACF4 | 0 images |
| 2 | LZSS. A flag byte is read LSB-first: 1 = copy a literal byte, 0 = two bytes *b1* *b2* giving length (*b1* & 0x0F) + 3 and distance (*b2* << 4) + (*b1* >> 4) back in the output | 0x5AD2B | 190 |
| 3 | LZSS as format 2, but with length (*b1* & 0x1F) + 3 and distance (*b2* << 3) + (*b1* >> 5) | 0x5AD8D | 2,977 |

The format 1 to 3 routines are hand-written assembly, not found as
functions by Ghidra; disassemble them with capstone. Format 1 is probably
used for other data elsewhere; it isn't used by any image.

### Uncompressed images (tag 32)

| Bytes | Meaning |
|-------|---------|
| 0-3 | Width and height words (the height tag is 32) |
| 4-5 | Bits per pixel: 4 or 8 |
| 6-9 | Width and height again |
| 10- | Pixels. 8-bit: width × height bytes. 4-bit: rows of ceil(width/2) bytes, high nibble first, followed by the 16-byte colour map. |

The game uses these in place (0x3E867 returns a pointer to byte 10).

### Transparency

The decoders produce plain palette indices. Which colour is transparent
is chosen by the drawing call, not stored in the image. Creature frames
use a solid background colour (blue in the master palette) as the key.
The blitter and its colour-key argument still need to be traced (see `04-rendering.md`).

## Palettes

- **Master palette, (1,0,9,254), entry 206.** 1,024 bytes: 256 records of
  (index, R, G, B), where the first byte always equals the record number
  and the components are 8-bit (0-255; index 255 is white). Rendering
  every image with this palette gives correct colours (checked visually on
  walls, floors, interface panels, creatures, items, portraits and the
  credits screen). The game presumably shifts the components down to the
  6-bit VGA DAC range when it uploads them. The upload code wasn't found by
  searching for port 0x3C8/0x3C9 constants, so it may go through the video
  driver layer.
- **(1,0,7,0), entry 203, 768 bytes.** Values are 0-31 only, so this is
  *not* the display palette, even though its size matches one. It may be a
  5-bit RGB table used for colour matching or fades. Unresolved.
- **(1,0,7,1), entry 204, 256 bytes**, and **(8,set,7,n), 256 bytes each.**
  Palette-index remap tables. The map-set ones come in sub-indices 1-4
  and 10-13, which looks like light levels (two series of four). They are
  probably applied when drawing to darken the view; to be confirmed in the
  renderer.
- **(1,0,7,2), entry 202, 1,041 bytes.** Starts with ramps
  (0, 4, 8 ... 0x3F) in 6-bit DAC units. Probably fade or brightness tables.
- **4-bit colour maps.** Each 4-bit image carries its own 16-byte nibble →
  palette map (see above). Creatures also have 16- and 48-byte type-7
  entries (category 15), likely alternative colour maps for recoloured
  creature variants. Not yet confirmed.

## Tools

- `python3 tools/gdat.py export [out_dir] [cat]` decodes every image to an
  indexed PNG (default `re/png/`, gitignored), named
  `cCC_iIII_sSSS_eENTRY.png`. About 6 s for the whole archive.
- `cargo test -p dm2-formats` runs the Rust archive and image tests against
  `original/dumast2/DATA/GRAPHICS.DAT` when it is present, and skips them otherwise.
