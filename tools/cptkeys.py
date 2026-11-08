#!/usr/bin/env python3
"""Decode how the CPT editor maps keys to its function handlers, from a memory dump.

Make a dump with  cpt-run --cpt-boot disk/cpt.img 82000000 - 40000 disk/mem.bin  then:
  python tools/cptkeys.py disk/mem.bin

The path [CONFIRMED by tracing]: the keyboard reader returns a class and an index;
"get the next key" (D9A7H, segment 3D function 00, block 2) remaps the index through
  * DBF6H: a byte per plain function code (A0H + n -> index; 0 = ignore),
  * DC59H: (value, index) pairs for CODE + a function or keypad key,
  * DC14H: pairs for CODE + a character key (upper-cased),
  * DC84H: pairs for a character after CODE + key 76H (a third layer),
and the editor (C765H, segment 23 function 00) jumps through C916H (indices below 3FH) or
CB9EH (indices 51H and up). CCF1H is the error beep. See program-disk.md, "The editor's key
dispatch". The "key" column is the first key whose table value matches.
"""
import sys

if len(sys.argv) < 2:
    print(__doc__)
    sys.exit(2)
d = open(sys.argv[1], 'rb').read()
b2 = d[3 * 0x8000:4 * 0x8000]
r = lambda a: b2[a - 0x8000]
w = lambda a: r(a) | r(a + 1) << 8

# The key table (DEFFH, block 2): unshifted value at (k & 0F) * 8 + (k >> 4), shifted +7FH.
T = 0xDEFF
n = r(T)


def idx(k):
    a = (k + (k & 0x70)) & 0xFF
    for _ in range(3):
        a = ((a << 1) | (a >> 7)) & 0xFF
    return a


key_of_code = {}
for k in range(1, 0x80):
    i = idx(k)
    if i <= n:
        v = r(T + i)
        key_of_code.setdefault(v, k)

# Editor tables: index -> handler.
t1 = {i: w(0xC916 + 2 * i) for i in range(0x3F)}
t2 = {i: w(0xCB9E + 2 * (i - 0x51)) for i in range(0x51, 0x7E)}


def handler(i):
    if i < 0x3F:
        return t1.get(i, 0)
    if 0x51 <= i < 0x7E:
        return t2.get(i, 0)
    return None


def keyname(code):
    k = key_of_code.get(code)
    return f'key {k:02X}' if k is not None else 'no key'


print('Plain function keys: code (A0H + n) -> index via DBF6H')
for nn in range(0x1E):
    i = r(0xDBF6 + nn)
    code = 0xA0 + nn
    if i:
        print(f'  code {code:02X} ({keyname(code)}) -> index {i:02X} -> {handler(i) or 0:04X}H')

for name, table in (('CODE + function key (DC59H)', 0xDC59), ('CODE + character key (DC84H)', 0xDC84),
                    ('other class (DC14H)', 0xDC14)):
    print(f'{name}: value -> index')
    a = table
    while r(a):
        v, i = r(a), r(a + 1)
        h = handler(i)
        ch = chr(v) if 0x20 <= v < 0x7F else f'{v:02X}H'
        print(f'  {ch:5} ({keyname(v) if v >= 0x80 else keyname(v) if v in key_of_code else keyname(v + 0x20)}) -> index {i:02X} -> {h or 0:04X}H')
        a += 2
