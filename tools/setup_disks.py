#!/usr/bin/env python3
"""Make the disk images the emulator and its tests use, from what is in the repository.

The program disks are kept in the repository as TeleDisk files (.kiro/specs/cpt8000-emulator/
*.td0). The emulator wants flat images (77 tracks x 16 sectors x 256 bytes = 315,392 bytes),
so this decodes each one (td0.py checks every CRC) into disk/ (gitignored), and makes a blank,
formatted data disk too (mkdatadisk.py).

  disk/cpt.img         from cpt.td0     program disk (CPT 8100)
  disk/cpt2365.img     from cpt2365.td0 the same disk, saved with TeleDisk's compression
  disk/cpt2489.img     from cpt2489.td0 program disk with TTY communications
  disk/blank-data.img  a blank, formatted data disk (the same bytes as
                       crates/cpt-wasm/web/blank-data.img, which is kept in the repository)

usage:
  setup_disks.py [--out DIR]     DIR defaults to disk/ in the repository
(setup-disks.bat in the repository root runs this.)
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
SPECS = os.path.join(ROOT, ".kiro", "specs", "cpt8000-emulator")
SIZE = 77 * 16 * 256

sys.path.insert(0, HERE)
import mkdatadisk  # noqa: E402
import td0  # noqa: E402

PROGRAM_DISKS = [("cpt.td0", "cpt.img"), ("cpt2365.td0", "cpt2365.img"), ("cpt2489.td0", "cpt2489.img")]


def decode(src, dst):
    with open(src, "rb") as f:
        _info, tracks, _end = td0.parse(f.read())
    img = td0.raw_image(tracks)
    if len(img) != SIZE:
        raise SystemExit("%s decoded to %d bytes, not %d" % (src, len(img), SIZE))
    with open(dst, "wb") as f:
        f.write(img)
    return len(img)


def main(argv):
    out = os.path.join(ROOT, "disk")
    args = list(argv)
    while args:
        a = args.pop(0)
        if a == "--out" and args:
            out = args.pop(0)
        else:
            print(__doc__)
            return 2
    os.makedirs(out, exist_ok=True)
    for name, img in PROGRAM_DISKS:
        src = os.path.join(SPECS, name)
        if not os.path.exists(src):
            print("missing %s; skipped" % src)
            continue
        try:
            n = decode(src, os.path.join(out, img))
        except td0.TD0Error as e:
            raise SystemExit("%s: %s" % (src, e))
        print("%-18s %s (%d bytes)" % (name, os.path.join(out, img), n))
    blank = os.path.join(out, "blank-data.img")
    with open(blank, "wb") as f:
        f.write(mkdatadisk.make())
    print("%-18s %s (%d bytes)" % ("(blank data disk)", blank, SIZE))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
