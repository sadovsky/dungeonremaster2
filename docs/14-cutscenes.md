# Cutscenes, CD image and other distribution files

## FTL, INTRO, END, CREDITS: Interplay MVE movies

Each of these extensionless files is the same small program with a movie
appended:

| Byte range | Contents |
|------------|----------|
| 0 to 1330 | Real-mode DPMI stub (needs XMS/A20, 386+, 32-bit DPMI host) |
| 1330 to 100206 | The embedded 32-bit **Interplay MVE player** (identical in all four files). Its usage string advertises `[videomode] [HALF\|BLEND\|WIDE\|HOLD\|LOOP\|NOABORT\|LOG] file.mve [audio [track]]`; it uses HMI drivers for sound. |
| 100206 to end | A standard **Interplay MVE** movie (`Interplay MVE File\x1A\0` signature) |

`tools/audio.py mve` carves the movies into `re/audio/mve/`. ffprobe
identifies each one as `interplayvideo` at 432×320 with `pcm_u8` audio at
22,050 Hz in stereo, so the MVE container and codec are fully documented
elsewhere and ffmpeg can decode them.

| File | Movie bytes | Role |
|------|-------------|------|
| FTL | 0.39 MB | Developer logo |
| INTRO | 1.65 MB | Opening |
| END | 4.98 MB | Ending (win) |
| CREDITS | 5.35 MB | Credits roll |

The CD also carries `FTL.VGA`, `INTRO.VGA` and `END.VGA`, which have the
same layout but are slightly smaller movies, probably the 320×200 VGA
versions installed when SVGA isn't selected (see the README's SVGA/VGA
install note). There is no `CREDITS.VGA`. Instead there is a
`CREDITS.PCX`, presumably a static VGA fallback.

**For the remake**, at first run, carve the movie from the user's files
(the offset is fixed at 100206; check the signature) and decode it either
with a Rust MVE decoder or by shelling out to ffmpeg. Either way the
movie is played straight from the user's copy, never bundled.

## SPLASH

A small 16-bit Borland C program (`SPLASH.C`, `LOAD_PCX`). It shows a
PCX image and exits. About 7.8 KB of data is appended to the MZ image,
presumably the picture, though it doesn't start with a plain PCX header;
not yet examined. `INTRPLAY.PCX` (320×200, 8-bit, PCX version 3.0) is
the Interplay publisher screen in the game directory. Either can be shown
with any PCX decoder.

## CD image (`cd/DM2.img`, `.cue`, `.ccd`, `.sub`)

- One track only: MODE1/2352 data. **No Red Book audio tracks**, so no CD
  music.
- An ISO 9660 volume labelled `DM2`, 15,119 blocks. `tools/audio.py cd`
  lists it, and `--extract` writes it to `re/cd/`.
- Everything in the installed directory is also on the CD (same sizes;
  `FTL` is byte-identical). Extra files on the CD only:
  - `FTL.VGA`, `INTRO.VGA`, `END.VGA` and `CREDITS.PCX`: the VGA cutscene variants (see above)
  - `INSTALL.EXE`: the installer, which chooses between SVGA and VGA
  - `PATCH/`: `DM2UP.EXE`, `VESA.EXE`, an updated `INSTALL.EXE`,
    `IBMIOP.EXE`, `SETUP.INI`, a README (the same text as the main one
    plus support contacts) and **a different `SKULL.EXE` build** (522,641
    bytes, against 522,637 installed; about 418,000 bytes differ, so it
    was relinked and isn't just a byte patch).
- **Nothing on the CD is needed for gameplay** beyond `DATA/`. The only
  extra content is the VGA movie variants.

### Open question: which SKULL.EXE to reverse

The installed build and `PATCH/SKULL.EXE` differ. The patch is likely a
bug-fix release (`DM2UP.EXE` sounds like an updater). It is worth
diffing the two at the function level to find out what changed, and
possibly reversing the patched build as the reference. The current
analysis (`docs/01-executable.md`) uses the installed build.

## Other files

| File | Notes |
|------|-------|
| `EREGCARD.EXE` / `.INI` | Electronic registration card; ignore |
| `IBMIOP.EXE` | Launcher run as `IBMIOP SKULL.EXE +VS` |
| `SETUP.EXE` / `.INI` | HMI sound setup (writes `SKULL.CFG`) |
| `DOS4GW.EXE` | DOS extender |
