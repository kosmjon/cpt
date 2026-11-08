#!/usr/bin/env python3
"""Read the filesystem of a CPT program disk image (as decoded from cpt.td0 by td0.py).

What is understood (see .kiro/specs/cpt8000-emulator/program-disk.md):
  * 77 tracks x 16 sectors x 256 bytes; linear sector = track * 16 + sector.
  * Numbers are big-endian.
  * Track 0 sector 0: volume label (total sectors at +5, sectors/track +29, bytes/sector +2A,
    tracks +2C).
  * Track 0 sectors 3 + 6: allocation map, 2 bits per sector.
  * The document directory is a B-tree.  Node = level byte ('0' leaf / '1' inner, ASCII),
    then 23-byte items: 16-char space-padded name + 4-byte field + 3-byte pointer.  The root
    is track 0 sector 13.  In an inner node the first item has an empty name.
  * Pointers are logical sectors.  Within a track the filesystem uses a skew of 3:
    physical sector = (3 * logical sector) mod 16.  This is confirmed for the directory's own
    root -> leaf pointers; it is NOT yet understood what a leaf's pointer points to.

usage:
  cptfs.py IMAGE            volume label, allocation summary, directory listing
"""
import sys

SPT = 16
SS = 256
ALLOC = {0: "free", 1: "reserved", 2: "data", 3: "system/index"}


def physical(logical):
    track, sec = divmod(logical, SPT)
    return track * SPT + (3 * sec) % SPT


def sector(img, linear):
    return img[linear * SS:(linear + 1) * SS]


def walk(img, ptr, depth=0, out=None, seen=None):
    """Collect (name, field, pointer, leaf sector) for every leaf entry under node `ptr`."""
    out = [] if out is None else out
    seen = set() if seen is None else seen
    lin = physical(ptr)
    if lin in seen or depth > 8:
        raise SystemExit("directory loop at sector %d" % lin)
    seen.add(lin)
    node = sector(img, lin)
    level = node[0]
    if level not in (0x30, 0x31, 0x32, 0x33):
        raise SystemExit("sector %d (logical %d) is not a directory node (level byte %02X)"
                         % (lin, ptr, level))
    o = 1
    while o + 23 <= SS:
        name = node[o:o + 16]
        field = int.from_bytes(node[o + 16:o + 20], "big")
        child = int.from_bytes(node[o + 20:o + 23], "big")
        if not any(node[o:o + 23]):
            # An empty slot (leaf 902's first slot is one).  Stop only when the rest of the
            # node is blank too.
            if not any(node[o:]):
                break
            o += 23
            continue
        if level == 0x30:
            out.append((name.decode("latin-1").rstrip(), field, child, lin))
        elif child:
            walk(img, child, depth + 1, out, seen)
        o += 23
    return out


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    img = open(argv[0], "rb").read()
    label = sector(img, 0)
    total = int.from_bytes(label[5:7], "big")
    print("volume label: %d sectors, %d per track, %d bytes each, %d tracks; text %r"
          % (total, label[0x29], int.from_bytes(label[0x2A:0x2C], "big"), label[0x2C],
             label[0x37:0x48].decode("latin-1").strip()))

    amap = sector(img, 3) + sector(img, 6)
    counts = {}
    for i in range(total):
        c = (amap[i // 4] >> (6 - 2 * (i % 4))) & 3
        counts[c] = counts.get(c, 0) + 1
    print("allocation:   " + ", ".join("%s %d" % (ALLOC[c], n) for c, n in sorted(counts.items())))

    root = sector(img, 13)
    print("directory:    root at track 0 sector 13 (level %r)" % chr(root[0]))
    # The root's items point at the leaves (logical numbers, skew 3 within the track).
    entries = []
    o = 1
    while o + 23 <= SS and any(root[o:o + 23]):
        child = int.from_bytes(root[o + 20:o + 23], "big")
        if child:
            entries += walk(img, child, 1)
        o += 23
    print("%d documents:" % len(entries))
    print("  %-16s %-10s %-8s %s" % ("name", "field", "pointer", "(leaf sector)"))
    for name, field, ptr, leaf in entries:
        print("  %-16s %08X   %-8d %d" % (name, field, ptr, leaf))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
