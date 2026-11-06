# CP/M over serial (MOL80 BIOS)

A side goal, useful because it exercises real, source-independent parts of the system while we
wait for the CPT source: boot a standard **CP/M 2.2** on the emulated machine with its console
routed over the **serial port** to a VT100 terminal (e.g. MobaXterm), using a **BIOS written in
MOL80**. Stock CP/M provides the CCP + BDOS; we write only the machine-specific BIOS.

## Why this is worthwhile
- Exercises the serial device, disk controller, boot sequence, and `mol80asm` end to end.
- Sidesteps the CPT display/font entirely — CP/M's console is a plain character stream, so the
  VT100 does the rendering. No font ROM or CPT display model needed.
- Produces a genuinely interactive, runnable machine driven by MOL80 code.

## CP/M layering (what we write vs. reuse)
- **CCP** (command processor, `A>`) and **BDOS** (file system, `CALL 5` dispatch): **stock
  CP/M 2.2**, a freely available 8080 binary — we do not write these.
- **BIOS** (machine-specific drivers): **written in MOL80**. Its jump table routes:
  - CONIN / CONOUT / CONST -> the **serial port** (USART), so the console is the VT100.
  - HOME / SELDSK / SETTRK / SETSEC / SETDMA / READ / WRITE -> the **disk controller**.
  - WBOOT / BOOT -> warm/cold boot handling.

## Boot behavior (from boot-sequence.md)
On disk insert with no WP OS running, the drive steps to track 1, reads **sector 0 (the boot
sector)**, and if bootable loads the first ~15-20 tracks, then jumps in. We reproduce the
essential path: read boot sector -> load the system image -> jump to it. (The retry-on-
unbootable cycle is cosmetic and optional.)

## Disk sectors: how much do we simulate?
Not much. The disk device is already block-addressed with the classic 8-inch IBM 3740 geometry
(77 tracks x 26 sectors x 128 bytes = 2002 blocks). A CP/M BIOS only needs a mapping from its
(track, sector) to our linear block:

    block = track * 26 + (sector - 1)        (CP/M sectors are 1-based after translation)

There is no seek time, rotational latency, head load/unload or interleave to model — those only
matter on real media. Sector skew (the CP/M translate table) is a BIOS concern and, with no
rotation, can be the identity mapping as long as the BIOS and disk-parameter block agree.

## Increments
1. **Serial sign-on (done).** Hand-assembled 8080 program writing a string out the serial port,
   run via `cpt-run --serial-demo`; verified from MobaXterm (Telnet).
2. **MOL80 serial console routines (done).** `cpm/console.mol`: CONOUT / CONIN / PRINT in MOL80
   plus a sign-on and echo loop; verified over TCP and from MobaXterm.
3. **Boot ROM + boot sector (done).** `cpm/boot.mol` is the boot ROM: from reset it reads block
   0 of drive 1 into 0100H and jumps there, retrying while no usable disk is present (as the
   hardware did, so a disk inserted later boots). `cpm/bootsector.mol` is the block-0 program
   (banner + echo for now). `cpm/mkdisk.mjs` builds a disk image with a boot sector. Covered by
   `crates/cpt-run/tests/boot.rs` (assembles the real `.mol` files; boots, echoes, no-disk
   retry, disk inserted later) and verified live over TCP.
4. **CP/M 2.2 BIOS in MOL80.** Full jump table; console -> serial, disk -> controller. Lay out
   a CP/M program-disk image (boot sector + system tracks with stock CCP/BDOS + our BIOS +
   directory/data). The boot sector becomes a loader for the system tracks (per
   boot-sequence.md the hardware loads the first ~15-20 tracks). Boot it; reach the `A>` prompt
   over the VT100.
5. **Validate** with CP/M built-ins (DIR, TYPE) and a `.COM` (e.g. the CP/M diagnostics).

## Running the boot demo
    cargo run -p mol80asm -- --bin cpm/boot.bin cpm/boot.mol
    cargo run -p mol80asm -- --bin cpm/bootsector.bin cpm/bootsector.mol
    node cpm/mkdisk.mjs cpm/boot.img cpm/bootsector.bin
    cargo run -p cpt-run -- --serial-listen 127.0.0.1:2323 --disk1 cpm/boot.img cpm/boot.bin
then Telnet to 127.0.0.1:2323. `--disk2` mounts a second image in drive 2. Disk images are
file-backed; writes are persisted when the guest issues the flush command or at a clean exit
(not on Ctrl+C), so the BIOS should flush after writes.

## Open items / risks
- `cpm/*.mol` now use `NAME EQU value` (our-convention placeholder) for port numbers and status
  bits. There is no INCLUDE yet, so each file repeats the equates it needs.
- Writing a real BIOS will surface `mol80asm` gaps (ORG, DB/DW/DS data directives, maybe more);
  we extend the assembler as needed — useful work regardless.
- Stock CP/M 2.2 CCP+BDOS binary must be fetched (freely available / effectively public domain).
- CP/M disk layout must match the BIOS sector translation (reserved tracks, sector skew).
- Serial framing for a VT100: 8-bit data, CR/LF handling; MobaXterm connects raw TCP or telnet.
- Boot sector location: boot-sequence.md says the drive steps to "Track 1" and reads "Sector 0".
  We read linear block 0; whether the real hardware's first sector is block 0 under our
  numbering is [VERIFY]. It is one constant in `cpm/boot.mol`.
- The boot ROM is run as an ordinary program loaded at reset by `cpt-run`; the emulator does not
  yet auto-start a boot when a disk is inserted into a running, idle machine (the ROM's retry
  loop gives the same effect from the guest side, as the test shows).
- Port addresses / status bits for serial and disk are still [VERIFY] placeholders — fine for
  CP/M (we control both the BIOS and the emulator config, so they just have to agree).

## Increment 4 status: CP/M 2.2 boots to `A>` (done)
- **Stock CCP + BDOS** from Digital Research source (`cpm/vendor`, Eric Smith's reformatted
  copy), assembled by `cpm/tools/asm80.py` (a small build-only 8080 cross assembler) at
  CCP `0E400H`, BDOS `0EC00H` (entry `0EC06H`), BIOS `0FA00H`.
- **BIOS in MOL80**: `cpm/bios.mol` (17-entry jump table, serial console, disk via the
  placeholder controller, no sector skew, `block = track*26 + sector`, flush after every
  write). DPB: IBM 3740 (SPT 26, BSH 3, BLM 7, EXM 0, DSM 242, DRM 63, AL0 C0, CKS 16, OFF 2).
- **Loader**: `cpm/loader.mol` is the boot sector; it reads blocks 1..49 to `0E400H` and enters
  the BIOS. `cpm/mkcpm.mjs` lays out the disk; `cpm/build-cpm.ps1` builds everything into
  `cpm/build/cpm.img` (gitignored).
- **Run**: `cargo run -p cpt-run -- --serial-listen 127.0.0.1:2323 --disk1 cpm/build/cpm.img cpm/build/boot.bin`,
  then Telnet from MobaXterm. `cpm/tools/serial-drive.ps1` scripts a session for testing.
- **Verified**: boots to `A>`, `DIR` works, `SAVE 1 T.COM` creates a file that persists across
  a restart. `crates/cpt-run/tests/cpm.rs` covers boot, DIR and SAVE (skips if the CCP/BDOS
  haven't been built).
- **Utilities**: the DRI CP/M 2.2 distribution .COM files (`cpm/dist`, from the Xerox 1800 CP/M 2.2 distribution disk at cpm.z80.de) are copied onto A: by `cpm/tools/cpmfs.py`; STAT, PIP, DIR verified. `cpmfs.py IMAGE mkblank` makes an empty data disk (all 0E5H); `build-cpm.ps1` writes `cpm/build/data.img` for drive 2. PIP A:->B: and running the copy from B: verified.
- **Sector numbering**: with no skew table the BDOS passes 0-based sectors through SECTRAN, so `block = track*26 + sector` (0-based), not `sector-1`.
- **Not yet**: no VT100 handling beyond what the terminal does itself; ED/ASM/DDT not exercised.
