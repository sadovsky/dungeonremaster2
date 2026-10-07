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


# DM2_IPWATCH: one pointer chain in the same form, checked before every
# executed instruction (slow); logs "I tick ip value" with the code address
# of the instruction that follows the write, so the writer can be found.
IP_ANCHOR = "\tif (dm2_code_delta) {\n"

IP_BLOCK = r'''	if (dm2_code_delta) {
		static Bit32u dm2_ic[6];
		static int dm2_ilen = -1;
		static Bit32u dm2_ival = 0xFFFFFFFFu;
		if (dm2_ilen < 0) {
			dm2_ilen = 0;
			const char * w = getenv("DM2_IPWATCH");
			while (w && *w && dm2_ilen < 6) {
				char * end;
				dm2_ic[dm2_ilen++] = (Bit32u)strtoul(w, &end, 16);
				w = (*end == ':') ? end + 1 : 0;
			}
		}
		if (dm2_ilen >= 2) {
			Bit32u ptr = LoadMd(dm2_ic[0] + dm2_data_delta);
			for (int k = 1; k < dm2_ilen - 1 && ptr; k++) ptr = LoadMd(SegBase(ds) + ptr + dm2_ic[k]);
			Bit32u v = ptr ? LoadMw(SegBase(ds) + ptr + dm2_ic[dm2_ilen - 1]) : 0xFFFFu;
			if (v != dm2_ival) {
				fprintf(dm2_log, "I %u %x %x\n", LoadMd(0x7F22C + dm2_data_delta), ip - dm2_code_delta, v);
				dm2_ival = v;
			}
		}
	}
'''


def main():
    p = Path(sys.argv[1])
    s = p.read_text()
    if 'DM2_WATCHP' not in s:
        assert ANCHOR in s, 'anchor not found'
        s = s.replace(ANCHOR, BLOCK + ANCHOR)
        print('patched DM2_WATCHP')
    if 'DM2_IPWATCH' not in s:
        assert IP_ANCHOR in s, 'hook anchor not found'
        s = s.replace(IP_ANCHOR, IP_BLOCK + IP_ANCHOR, 1)
        print('patched DM2_IPWATCH')
    p.write_text(s)


if __name__ == '__main__':
    main()
