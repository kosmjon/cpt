#!/usr/bin/env python3
"""Decode a Sydex TeleDisk (.td0) floppy image.

Format (reverse-engineered; see Dave Dunfield's TeleDisk notes):
  image header (12 bytes): "TD" (normal) or "td" (advanced/LZHUF compression), volume
      sequence, check signature, version, data rate (bit 7 = FM / single density), drive
      type, stepping (bit 7 = a comment block follows), DOS-allocation flag, sides, CRC16.
  optional comment block: CRC16, length, 6-byte date/time, text.
  tracks until one with sector count 255:
      track header: sectors, cylinder, head (bit 7 = FM), CRC (low byte of CRC16).
      per sector: C, H, R, N (size = 128 << N), flags, CRC (low byte, over the data),
          then unless flags have 0x30 (no data): length (2), method, payload where
          method 0 = raw, 1 = repeated 2-byte pattern, 2 = run-length blocks.
  CRC16: polynomial 0xA097, initial value 0, no reflection.

usage:
  td0.py IMAGE.td0                     summary + per-track table, CRC-checked
  td0.py IMAGE.td0 --raw OUT.img       also write a flat image (tracks in cylinder/head
                                       order, each track's sectors sorted by sector number)
  td0.py IMAGE.td0 --json OUT.json     also write the full sector map
"""
import json
import sys

SECTOR_FLAGS = {
    0x01: "duplicate",
    0x02: "crc-error",
    0x04: "deleted-data",
    0x10: "skipped(dos)",
    0x20: "no-data",
    0x40: "data-without-id",
}
DRIVE_TYPES = {1: "5.25 360K", 2: "5.25 1.2M", 3: "3.5 720K", 4: "3.5 1.44M", 5: "8-inch", 6: "3.5"}
RATES = {0: "250 kbps", 1: "300 kbps", 2: "500 kbps"}


def crc16(data, crc=0):
    for b in data:
        crc ^= b << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0xA097) if crc & 0x8000 else (crc << 1)
            crc &= 0xFFFF
    return crc


class TD0Error(Exception):
    pass


def decode_sector_data(method, payload, size):
    if method == 0:
        out = bytes(payload)
    elif method == 1:
        count = payload[0] | payload[1] << 8
        out = bytes(payload[2:4]) * count
    elif method == 2:
        out = bytearray()
        i = 0
        while i < len(payload):
            kind, count = payload[i], payload[i + 1]
            i += 2
            if kind == 0:
                out += payload[i:i + count]
                i += count
            else:
                n = 2 * kind
                out += bytes(payload[i:i + n]) * count
                i += n
        out = bytes(out)
    else:
        raise TD0Error("unknown sector encoding method %d" % method)
    if len(out) != size:
        raise TD0Error("sector decoded to %d bytes, expected %d" % (len(out), size))
    return out


def lzhuf_decode(data):
    """Expand TeleDisk "advanced compression": Okumura's LZHUF (LZSS with a 4K window, matches
    of 3-60 bytes, an adaptive Huffman code for literals/lengths and a fixed code for the
    upper 6 bits of the match position). The stream has no length prefix; it runs to the end
    of the file, so decoding stops when the input bits run out."""
    N, F, THRESHOLD = 4096, 60, 2
    N_CHAR = 256 - THRESHOLD + F
    T = N_CHAR * 2 - 1
    R = T - 1
    MAX_FREQ = 0x8000

    # Fixed code for the position's upper 6 bits: canonical, lengths 3..8 for 1, 3, 8, 12, 24
    # and 16 values. d_code/d_len are indexed by the next 8 input bits.
    d_code, d_len = [], []
    value = 0
    for length, count in ((3, 1), (4, 3), (5, 8), (6, 12), (7, 24), (8, 16)):
        for _ in range(count):
            d_code += [value] * (1 << (8 - length))
            d_len += [length] * (1 << (8 - length))
            value += 1

    freq = [0] * (T + 1)
    prnt = [0] * (T + N_CHAR)
    son = [0] * T
    for i in range(N_CHAR):
        freq[i] = 1
        son[i] = i + T
        prnt[i + T] = i
    i, j = 0, N_CHAR
    while j <= R:
        freq[j] = freq[i] + freq[i + 1]
        son[j] = i
        prnt[i] = prnt[i + 1] = j
        i += 2
        j += 1
    freq[T] = 0xFFFF
    prnt[R] = 0

    def reconst():
        j = 0
        for i in range(T):
            if son[i] >= T:
                freq[j] = (freq[i] + 1) // 2
                son[j] = son[i]
                j += 1
        i, j = 0, N_CHAR
        while j < T:
            f = freq[j] = freq[i] + freq[i + 1]
            k = j - 1
            while f < freq[k]:
                k -= 1
            k += 1
            freq[k + 1:j + 1] = freq[k:j]
            freq[k] = f
            son[k + 1:j + 1] = son[k:j]
            son[k] = i
            i += 2
            j += 1
        for i in range(T):
            k = son[i]
            if k >= T:
                prnt[k] = i
            else:
                prnt[k] = prnt[k + 1] = i

    def update(c):
        if freq[R] == MAX_FREQ:
            reconst()
        c = prnt[c + T]
        while True:
            freq[c] += 1
            k = freq[c]
            l = c + 1
            if k > freq[l]:
                while k > freq[l + 1]:
                    l += 1
                freq[c] = freq[l]
                freq[l] = k
                i = son[c]
                prnt[i] = l
                if i < T:
                    prnt[i + 1] = l
                j = son[l]
                son[l] = i
                prnt[j] = c
                if j < T:
                    prnt[j + 1] = c
                son[c] = j
                c = l
            c = prnt[c]
            if c == 0:
                break

    total_bits = len(data) * 8
    bitpos = 0

    def bits(n):
        nonlocal bitpos
        v = 0
        for _ in range(n):
            byte = bitpos >> 3
            b = (data[byte] >> (7 - (bitpos & 7))) & 1 if byte < len(data) else 0
            v = (v << 1) | b
            bitpos += 1
        return v

    text = bytearray(b" " * N)
    r = N - F
    out = bytearray()
    while bitpos < total_bits:
        c = son[R]
        while c < T:
            c = son[c + bits(1)]
        c -= T
        update(c)
        if c < 256:
            out.append(c)
            text[r] = c
            r = (r + 1) & (N - 1)
        else:
            b = bits(8)
            pos = (d_code[b] << 6) | (((b << (d_len[b] - 2)) | bits(d_len[b] - 2)) & 0x3F)
            start = (r - pos - 1) & (N - 1)
            for k in range(c - 255 + THRESHOLD):
                ch = text[(start + k) & (N - 1)]
                out.append(ch)
                text[r] = ch
                r = (r + 1) & (N - 1)
    return bytes(out)


def parse(blob):
    if blob[:2] == b"td":
        # Advanced compression: the 12-byte header is stored as is, the rest is LZHUF.
        blob = blob[:12] + lzhuf_decode(blob[12:])
    elif blob[:2] != b"TD":
        raise TD0Error("not a TeleDisk image (signature %r)" % blob[:2])
    hdr = blob[:12]
    want = hdr[10] | hdr[11] << 8
    if crc16(hdr[:10]) != want:
        raise TD0Error("image header CRC mismatch")
    info = {
        "signature": hdr[:2].decode(),
        "volume_seq": hdr[2],
        "check_sig": hdr[3],
        "version": "%d.%d" % (hdr[4] >> 4, hdr[4] & 15),
        "data_rate": RATES.get(hdr[5] & 0x7F, "code %d" % (hdr[5] & 0x7F)),
        "fm_image": bool(hdr[5] & 0x80),
        "drive_type": DRIVE_TYPES.get(hdr[6], "code %d" % hdr[6]),
        "stepping": hdr[7] & 0x7F,
        "has_comment": bool(hdr[7] & 0x80),
        "dos_alloc": hdr[8],
        "sides": hdr[9],
        "comment": None,
    }
    pos = 12
    if info["has_comment"]:
        clen = blob[pos + 2] | blob[pos + 3] << 8
        block = blob[pos + 2:pos + 10 + clen]
        if crc16(block) != (blob[pos] | blob[pos + 1] << 8):
            raise TD0Error("comment block CRC mismatch")
        y, mo, d, h, mi, s = blob[pos + 4:pos + 10]
        text = blob[pos + 10:pos + 10 + clen].replace(b"\0", b"\n").decode("latin-1").strip()
        info["comment"] = {"date": "%04d-%02d-%02d %02d:%02d:%02d" % (1900 + y, mo + 1, d, h, mi, s),
                           "text": text}
        pos += 10 + clen

    tracks = []
    while True:
        nsec = blob[pos]
        if nsec == 0xFF:
            break
        th = blob[pos:pos + 4]
        if crc16(th[:3]) & 0xFF != th[3]:
            raise TD0Error("track header CRC mismatch at offset %#x" % pos)
        track = {"cyl": th[1], "head": th[2] & 0x7F, "fm": bool(th[2] & 0x80), "sectors": []}
        pos += 4
        for _ in range(nsec):
            c, h, r, n, flags, scrc = blob[pos:pos + 6]
            pos += 6
            size = 128 << n
            sec = {"c": c, "h": h, "r": r, "size": size, "flags": flags, "data": None}
            if not flags & 0x30:
                dlen = blob[pos] | blob[pos + 1] << 8
                method = blob[pos + 2]
                payload = blob[pos + 3:pos + 2 + dlen]
                pos += 2 + dlen
                data = decode_sector_data(method, payload, size)
                if crc16(data) & 0xFF != scrc:
                    raise TD0Error("sector data CRC mismatch: cyl %d head %d sec %d" % (c, h, r))
                sec["data"] = data
                sec["method"] = method
            track["sectors"].append(sec)
        tracks.append(track)
    return info, tracks, pos


def flag_names(f):
    return ",".join(v for k, v in SECTOR_FLAGS.items() if f & k) or "-"


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    path, rest = argv[0], argv[1:]
    raw_out = json_out = None
    while rest:
        opt = rest.pop(0)
        if opt == "--raw":
            raw_out = rest.pop(0)
        elif opt == "--json":
            json_out = rest.pop(0)
        else:
            raise SystemExit("unknown option %s" % opt)
    blob = open(path, "rb").read()
    info, tracks, end = parse(blob)

    print("file      %s (%d bytes, %d parsed)" % (path, len(blob), end + 1))
    for k in ("signature", "version", "drive_type", "data_rate", "fm_image", "sides", "stepping",
              "dos_alloc"):
        print("%-9s %s" % (k, info[k]))
    if info["comment"]:
        print("comment   %s  %r" % (info["comment"]["date"], info["comment"]["text"]))
    print("tracks    %d   (all CRCs verified)" % len(tracks))

    # Group tracks with an identical layout so the table stays short.
    def layout(t):
        secs = t["sectors"]
        return (t["fm"], tuple(s["size"] for s in secs), tuple(s["r"] for s in secs),
                tuple(flag_names(s["flags"]) for s in secs))
    groups = []
    for t in tracks:
        key = layout(t)
        if groups and groups[-1][0] == key:
            groups[-1][1].append(t)
        else:
            groups.append((key, [t]))
    print()
    print("cyl/head range        enc  sectors  size   sector order (as recorded)")
    for key, ts in groups:
        fm, sizes, order, flags = key
        a, b = ts[0], ts[-1]
        span = "%2d/%d .. %2d/%d" % (a["cyl"], a["head"], b["cyl"], b["head"])
        size = sizes[0] if len(set(sizes)) == 1 else "mixed"
        print("%-21s %-4s %7d  %-5s  %s" % (span, "FM" if fm else "MFM", len(order), size,
                                              " ".join(str(r) for r in order)))
        odd = sorted({f for f in flags if f != "-"})
        if odd:
            print("%21s flags: %s" % ("", "; ".join(odd)))

    if raw_out:
        out = bytearray()
        for t in sorted(tracks, key=lambda t: (t["cyl"], t["head"])):
            for s in sorted(t["sectors"], key=lambda s: s["r"]):
                out += s["data"] if s["data"] is not None else bytes(s["size"])
        open(raw_out, "wb").write(out)
        print("\nwrote %s (%d bytes, sectors sorted by number within each track)" % (raw_out, len(out)))
    if json_out:
        js = {"info": info, "tracks": [
            {"cyl": t["cyl"], "head": t["head"], "fm": t["fm"],
             "sectors": [{"r": s["r"], "c": s["c"], "h": s["h"], "size": s["size"],
                          "flags": flag_names(s["flags"]),
                          "method": s.get("method")} for s in t["sectors"]]}
            for t in tracks]}
        json.dump(js, open(json_out, "w"), indent=1)
        print("wrote %s" % json_out)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except TD0Error as e:
        sys.exit("error: %s" % e)
