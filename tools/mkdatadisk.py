#!/usr/bin/env python3
"""Make a blank CPT data disk image (77 tracks x 16 sectors x 256 bytes = 315,392 bytes).

What a data disk is, as the real CPT system writes it [CONFIRMED: the system itself
formatted an all-zero disk on the first OUT to it, and this tool reproduces what it wrote]:

  * track 0 sector 0, the label: only the first 16 bytes are used
        00 01 01 02 00 04 D0 00 00 01 00 00 0F 00 00 00
    (the program disk's label has 05 at +1 and 01 at +0FH instead; +5..6 = 04D0H = 1232 sectors)
  * the allocation map, 2 bits per logical sector (0 free, 1 reserved, 2 data, 3 system/index),
    in track 0 sectors 3 and 6. Logical sectors 0-15 (track 0) are reserved; the rest are free.
    The system allocates the first document's block as logical 16, marking it 3.
  * the directory root, track 0 sector 13: a leaf node ('0', ASCII 30H) with no entries. The
    system fills it in; the first 23-byte item slot stays empty, as on the program disk.
  * everything else is zero.

An all-zero disk (--zero) is accepted too: the system formats it on the first write.
Sectors here are physical; the filesystem's logical sectors are skewed (see cptfs.py).

usage:
  mkdatadisk.py OUT.img            a formatted, empty data disk
  mkdatadisk.py OUT.img --zero     an unformatted disk (all zero)
Then, for example:
  cpt-run --cpt-boot disk/cpt.img 130000000 - 40000 --disk2 OUT.img --save-disk2 NEW.img ...
  python tools/cptfs.py NEW.img
"""
import sys

TRACKS, SPT, SS = 77, 16, 256
SIZE = TRACKS * SPT * SS

LABEL = bytes.fromhex("00 01 01 02 00 04 d0 00 00 01 00 00 0f 00 00 00")


def sector(track, s):
    return (track * SPT + s) * SS


def make(zero=False):
    img = bytearray(SIZE)
    if zero:
        return img
    img[sector(0, 0):sector(0, 0) + len(LABEL)] = LABEL
    # Allocation map: logical sectors 0-15 reserved (01 in each 2-bit field = 55H per 4).
    img[sector(0, 3):sector(0, 3) + 4] = b"\x55" * 4
    # Directory root: an empty leaf.
    img[sector(0, 13)] = 0x30
    return img


def main(argv):
    args = [a for a in argv if not a.startswith("--")]
    flags = {a for a in argv if a.startswith("--")}
    if len(args) != 1 or flags - {"--zero"}:
        print(__doc__)
        return 2
    img = make(zero="--zero" in flags)
    assert len(img) == SIZE
    with open(args[0], "wb") as f:
        f.write(img)
    print("wrote %s (%d bytes, %s)" % (args[0], len(img),
                                       "unformatted, all zero" if "--zero" in flags else "formatted, empty"))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
