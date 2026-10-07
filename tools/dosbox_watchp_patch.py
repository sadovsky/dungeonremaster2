#!/usr/bin/env python3
"""Add the DM2_WATCHP pointer-chain watch to the hooked DOSBox source.

Usage: dosbox_watchp_patch.py PATH/TO/dosbox-0.74-3/src/cpu/core_normal.cpp

DM2_WATCHP takes comma-separated pointer chains "base:off1:...:last" (hex).
The first step reads the dword global at base (a data-object address); each
further offset reads a dword at DS:(p + off); the watched 16-bit word is at
DS:(p + last). At each random draw the hook logs "P tick index value" when a
watched word changed. Examples: creature record 155's status word
"7f298:9ba"; the square at (x, y) on map m "7f3c8:<4m>:<4x>:<y>".
"""
import sys
from pathlib import Path

ANCHOR = """\tPhysPt sp = SegBase(ss) + reg_esp;
\t/* 0x7F548: the thing reference of the creature whose AI context is loaded. */"""

BLOCK = r'''	/* DM2_WATCHP: pointer chains "base:off1:...:last" (hex), see
	 * tools/dosbox_watchp_patch.py. */
	{
		static Bit32u dm2_pc[16][6];
		static int dm2_plen[16];
		static Bit32u dm2_pval[16];
		static int dm2_np = -1;
		if (dm2_np < 0) {
			dm2_np = 0;
			const char * w = getenv("DM2_WATCHP");
			while (w && *w && dm2_np < 16) {
				int n = 0;
				char * end;
				for (;;) {
					Bit32u v = (Bit32u)strtoul(w, &end, 16);
					if (n < 6) dm2_pc[dm2_np][n++] = v;
					if (*end == ':') { w = end + 1; continue; }
					break;
				}
				dm2_plen[dm2_np] = n;
				dm2_pval[dm2_np++] = 0xFFFFFFFFu;
				w = (*end == ',') ? end + 1 : 0;
			}
		}
		for (int i = 0; i < dm2_np; i++) {
			int n = dm2_plen[i];
			if (n < 2) continue;
			Bit32u ptr = LoadMd(dm2_pc[i][0] + data_delta);
			for (int k = 1; k < n - 1 && ptr; k++) ptr = LoadMd(SegBase(ds) + ptr + dm2_pc[i][k]);
			Bit32u v = ptr ? LoadMw(SegBase(ds) + ptr + dm2_pc[i][n - 1]) : 0xFFFFu;
			if (v != dm2_pval[i]) {
				fprintf(dm2_log, "P %u %d %x\n", LoadMd(0x7F22C + data_delta), i, v);
				dm2_pval[i] = v;
			}
		}
	}
'''


def main():
    p = Path(sys.argv[1])
    s = p.read_text()
    if 'DM2_WATCHP' in s:
        print('already patched')
        return
    assert ANCHOR in s, 'anchor not found'
    p.write_text(s.replace(ANCHOR, BLOCK + ANCHOR))
    print('patched')


if __name__ == '__main__':
    main()
