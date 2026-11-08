#!/usr/bin/env python3
"""Decode the CPT system's segment / bank / function map from a memory dump.

Make a dump with:
  cpt-run --cpt-boot disk/cpt.img 82000000 - 40000 disk/mem.bin
(bank 0, then each 32K block, as cpt-run writes it), then:
  python tools/cptsegs.py disk/mem.bin [--calls]

How the system calls across banks [CONFIRMED by disassembly]:
  * A far call is  CALL 0046H  followed by two inline bytes: segment, function.
    0046H jumps to the dispatcher at 12DEH.
  * The segment table is at 0174H in bank 0, with the segment count at 0173H.  Each entry is
    4 bytes: present flag, bank, function-table address (little-endian).  The bank byte b
    selects block b-1 at 8000H-FFFFH (OUT 03H, b-1); b = 0 means bank 0 only.
  * A function table is a count byte followed by a 2-byte address per function (0000 = none).
  * The dispatcher returns at once if the segment number is past the count or the segment is
    not present; otherwise it selects the segment's bank, calls the function, and returns
    through 134EH, which restores the caller's bank.
  * 1374H looks a function's address up the same way (L = function, H = segment).
See .kiro/specs/cpt8000-emulator/program-disk.md, "Segments, banks and functions".
"""
import sys
from collections import Counter

# Functions identified so far (segment, function) -> what they do.
KNOWN = {
    (0x01, 0x00): "drive table lookup (0CDDH)",
    (0x01, 0x01): "start a disk request (07DDH)",
    (0x01, 0x02): "wait for a disk request, with retries (087CH)",
    (0x01, 0x04): "look up a far function's address (1374H)",
    (0x02, 0x02): "allocate a 256-byte display page (1632H)",
    (0x0C, 0x29): "filer: find a document / probe a drive (used at start-up)",
    (0x3A, 0x00): "build the screen's display list (E4D0H)",
    (0x3A, 0x0B): "swap the optional status row's jump C2/C3 (E6BAH)",
    (0x3D, 0x0A): "keyboard: take the next key from the ring at 058BH (D74CH)",
}


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    d = open(argv[0], "rb").read()
    show_calls = "--calls" in argv
    bank0 = d[:0x8000]

    def block(b):
        return d[0x8000 * (1 + b):0x8000 * (2 + b)]

    def read(bank, a):
        a &= 0xFFFF
        if a < 0x8000:
            return bank0[a]
        return block(bank - 1)[a - 0x8000] if bank > 0 else 0

    def word(bank, a):
        return read(bank, a) | read(bank, a + 1) << 8

    count = bank0[0x173]
    segs = {}
    absent = []
    print(f"{count} segment slots (count at 0173H, table at 0174H)")
    for s in range(count):
        e = bank0[0x174 + 4 * s:0x178 + 4 * s]
        present, bank, table = e[0], e[1], e[2] | e[3] << 8
        if not present:
            absent.append(s)
            continue
        n = read(bank, table)
        funcs = [word(bank, table + 1 + 2 * i) for i in range(n)]
        segs[s] = funcs
        where = "bank 0" if bank == 0 else f"block {bank - 1}"
        print(f"  seg {s:02X}: {where:8} table {table:04X}H, {n:2} functions")
        for i, f in enumerate(funcs):
            if f and (s, i) in KNOWN:
                print(f"           {i:02X}: {f:04X}H  {KNOWN[(s, i)]}")
        rest = [f"{i:02X}:{f:04X}" for i, f in enumerate(funcs) if f and (s, i) not in KNOWN]
        for i in range(0, len(rest), 8):
            print("           " + "  ".join(rest[i:i + 8]))
    print(f"  not present now: {len(absent)} ({' '.join(f'{s:02X}' for s in absent)})")

    calls = Counter()
    for b in range(-1, (len(d) // 0x8000) - 1):
        m = bank0 if b < 0 else block(b)
        for i in range(len(m) - 4):
            if m[i] == 0xCD and m[i + 1] == 0x46 and m[i + 2] == 0x00:
                calls[(m[i + 3], m[i + 4])] += 1
    missing = sorted({s for (s, _) in calls if s not in segs})
    print(f"\nfar calls (CALL 0046H, seg, func): {sum(calls.values())} sites, {len(calls)} distinct")
    print(f"  to segments not present now: {' '.join(f'{s:02X}' for s in missing)}")
    if show_calls:
        for (s, f), n in sorted(calls.items()):
            tgt = (f"-> {segs[s][f]:04X}H" if s in segs and f < len(segs[s])
                   else "(function out of range)" if s in segs else "(segment not present)")
            print(f"  {s:02X}.{f:02X} x{n:<3} {tgt}  {KNOWN.get((s, f), '')}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
