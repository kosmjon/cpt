#!/usr/bin/env python3
"""Tiny CP/M 2.2 file-system tool for our disk images (a build helper).

Geometry (must match cpm/bios.mol's DPB and the emulator's default disk):
  77 tracks x 26 sectors x 128 bytes = 2002 blocks; 2 system tracks (52 sectors);
  1K allocation blocks (243 of them, 0..242); 64 directory entries in blocks 0-1;
  16 blocks per extent (EXM 0), 8-bit block numbers.

usage:
  cpmfs.py IMAGE mkblank           create an empty, formatted data disk (all 0E5H)
  cpmfs.py IMAGE add FILE...       copy host files into the CP/M directory (user 0)
  cpmfs.py IMAGE ls                list the directory
"""
import os
import sys

SECTOR = 128
SECTORS = 77 * 26
SYS_BYTES = 2 * 26 * SECTOR
BLOCK = 1024
NBLOCKS = 243
DIR_ENTRIES = 64
DIR_BLOCKS = 2
EXTENT_BLOCKS = 16
EOF = 0x1A
EMPTY = 0xE5


def block_off(n):
    return SYS_BYTES + n * BLOCK


def read_dir(img):
    base = block_off(0)
    return [bytearray(img[base + i * 32: base + (i + 1) * 32]) for i in range(DIR_ENTRIES)]


def write_dir(img, entries):
    base = block_off(0)
    for i, e in enumerate(entries):
        img[base + i * 32: base + (i + 1) * 32] = e


def used_blocks(entries):
    used = set(range(DIR_BLOCKS))
    for e in entries:
        if e[0] == EMPTY:
            continue
        used.update(b for b in e[16:32] if b)
    return used


def split_name(path):
    base = os.path.basename(path).upper()
    name, _, ext = base.partition(".")
    if not name or len(name) > 8 or len(ext) > 3:
        raise SystemExit("cannot make a CP/M 8.3 name from %r" % path)
    return name.ljust(8), ext.ljust(3)


def add_file(img, path):
    name, ext = split_name(path)
    data = open(path, "rb").read()
    if len(data) % SECTOR:
        data += bytes([EOF]) * (SECTOR - len(data) % SECTOR)
    entries = read_dir(img)
    if any(e[0] == 0 and e[1:9] == name.encode() and e[9:12] == ext.encode() for e in entries):
        raise SystemExit("%s.%s already exists" % (name.strip(), ext.strip()))
    used = used_blocks(entries)
    free_blocks = [b for b in range(DIR_BLOCKS, NBLOCKS) if b not in used]
    need = -(-len(data) // BLOCK)
    if need > len(free_blocks):
        raise SystemExit("disk full adding %s" % path)
    mine = free_blocks[:need]
    free_slots = [i for i, e in enumerate(entries) if e[0] == EMPTY]
    extents = max(1, -(-need // EXTENT_BLOCKS))
    if extents > len(free_slots):
        raise SystemExit("directory full adding %s" % path)
    for n, b in enumerate(mine):
        img[block_off(b): block_off(b) + BLOCK] = data[n * BLOCK:(n + 1) * BLOCK].ljust(BLOCK, bytes([EOF]))
    for x in range(extents):
        e = bytearray(32)
        e[0] = 0
        e[1:9] = name.encode()
        e[9:12] = ext.encode()
        e[12] = x & 0x1F
        e[14] = x >> 5
        chunk = mine[x * EXTENT_BLOCKS:(x + 1) * EXTENT_BLOCKS]
        remaining = len(data) - x * EXTENT_BLOCKS * BLOCK
        e[15] = min(128, max(0, remaining) // SECTOR)
        e[16:16 + len(chunk)] = bytes(chunk)
        entries[free_slots[x]] = e
    write_dir(img, entries)


def listing(img):
    seen = {}
    for e in read_dir(img):
        if e[0] == EMPTY:
            continue
        key = (e[0], bytes(e[1:9]).decode("latin-1").rstrip(), bytes(e[9:12]).decode("latin-1").rstrip())
        seen[key] = seen.get(key, 0) + e[15] * SECTOR // 1024 + (1 if e[15] * SECTOR % 1024 else 0)
    for (user, n, x), k in sorted(seen.items()):
        print("%d: %-8s.%-3s %4dK" % (user, n, x, k))


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    path, cmd, rest = argv[0], argv[1], argv[2:]
    if cmd == "mkblank":
        with open(path, "wb") as f:
            f.write(bytes([EMPTY]) * (SECTORS * SECTOR))
        print("wrote %s: blank CP/M data disk" % path)
        return 0
    img = bytearray(open(path, "rb").read())
    if len(img) != SECTORS * SECTOR:
        raise SystemExit("%s is %d bytes; expected %d" % (path, len(img), SECTORS * SECTOR))
    if cmd == "add":
        for f in rest:
            add_file(img, f)
            print("added", os.path.basename(f).upper())
        open(path, "wb").write(img)
    elif cmd == "ls":
        listing(img)
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
