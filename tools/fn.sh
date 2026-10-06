#!/bin/sh
# Print decompiled functions by address (re/skull_decomp.c), dropping noise lines.
for a in "$@"; do
  a=$(printf '%08x' $((0x$a)))
  awk -v A="$a" '$0 ~ "^//==== .* @ "A {p=1; print; next} p&&/^\/\/==== /{exit} p' "$(dirname "$0")/../re/skull_decomp.c" |
    grep -vE 'uStack_[0-9a-f]+ = 0x[0-9a-f]+;$|= 0x[0-9a-f]{5};$|^\s+(undefined|int|uint|short|ushort|byte|char|bool|code)[0-9]* \**[a-zA-Z_0-9]+( \[[0-9]+\])?;$|^$'
done
