# SKULL.EXE

## Format

The file is a 32-bit Linear Executable (`LE`) run under DOS/4GW. The LE
header is at file offset 0x2E10, after the MZ stub. Page size is 0x1000.

| Object | Base | Virtual size | Flags | Contents |
|--------|------|--------------|-------|----------|
| 1 | 0x10000 | 0x595AC | 0x2005 (read, exec, 32-bit) | code |
| 2 | 0x70000 | 0x128F0 | 0x2003 (read, write, 32-bit) | data and BSS; the stack top is at 0x828F0 |

- Entry point: 0x62284, in object 1 (Watcom runtime startup).
- Fixups: about 11,000, all internal. The types used are 32-bit offset
  (0x07), 32-bit self-relative (0x08) and selector (0x02).
- Compiler: Watcom C/C++32 (runtime copyright strings for 1988-1994),
  using the Watcom register calling convention: the first arguments go in
  EAX, EDX, EBX and ECX, the rest on the stack, and the return value comes
  back in EAX. Ghidra's default cdecl guess will be wrong for most
  functions.

## Unpacking

`tools/le_unpack.py` places each object at its base address, applies all
fixups and writes `objN.bin` plus `layout.json` (bases, entry point and
fixup sites). `re/skull/flat.bin` is one image covering 0x10000 to
0x828F0, with the gap after the code filled with zeros.

## Ghidra (headless)

Install locations: `~/tools/ghidra_12.1.4_PUBLIC` and JDK 21 in `~/tools/jdk-21*`.

```
export JAVA_HOME=~/tools/jdk-21.0.12.1+1 PATH=$JAVA_HOME/bin:$PATH
~/tools/ghidra_12.1.4_PUBLIC/support/analyzeHeadless re/ghidra_proj skull \
  -import re/skull/flat.bin -loader BinaryLoader -loader-baseAddr 0x10000 \
  -processor x86:LE:32:default -cspec gcc -scriptPath re/ghidra_scripts \
  -preScript Setup.java -postScript ExportAll.java $PWD/re/skull_decomp.c -overwrite
```

`Setup.java` splits the image into code and data blocks and seeds
disassembly at the entry point. `ExportAll.java` writes every function's
decompilation to `re/skull_decomp.c` (gitignored).

## Address map (in progress)

| Address | What |
|---------|------|
| 0x62284 | Runtime entry point |
| 0x70019 | Literal path string for `SONGLIST.DAT` (referenced from 0x10396) |
| 0x73000 | Table of data-file name pointers (dungeon, graphics, save) |

## IBMIOP.EXE (launcher)

A 16-bit Borland C++ program packed with LZEXE 0.91. Unpack it with
`python3 tools/unlzexe.py original/dumast2/IBMIOP.EXE re/ibmiop.bin`.
It provides the display, input and timer services the game reaches
through `int 0xFC`, and sets the timer to 240 Hz (see 05-timeline, "Timer
rate").

