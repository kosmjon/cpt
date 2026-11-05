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

## Increments
1. **Serial sign-on (current step).** A small MOL80 program that writes a string out the serial
   port, run via `cpt-run --serial-listen`. Point MobaXterm (raw/telnet) at it and see the text
   on the VT100. Proves the serial + `mol80asm` path with no CP/M or disk yet.
2. **MOL80 serial console routines.** CONIN/CONOUT/CONST as reusable MOL80 (poll STATUS, read/
   write DATA). Echo test: type in MobaXterm, see it echoed back.
3. **Boot loader.** Emulator reads the boot sector from an inserted disk and jumps in (models
   the hardware boot).
4. **CP/M 2.2 BIOS in MOL80.** Full jump table; console -> serial, disk -> controller. Lay out
   a CP/M program-disk image (boot sector + system tracks with stock CCP/BDOS + our BIOS +
   directory/data). Boot it; reach the `A>` prompt over the VT100.
5. **Validate** with CP/M built-ins (DIR, TYPE) and a `.COM` (e.g. the CP/M diagnostics).

## Open items / risks
- Writing a real BIOS will surface `mol80asm` gaps (ORG, DB/DW/DS data directives, maybe more);
  we extend the assembler as needed — useful work regardless.
- Stock CP/M 2.2 CCP+BDOS binary must be fetched (freely available / effectively public domain).
- CP/M disk layout must match the BIOS sector translation (reserved tracks, sector skew).
- Serial framing for a VT100: 8-bit data, CR/LF handling; MobaXterm connects raw TCP or telnet.
- Port addresses / status bits for serial and disk are still [VERIFY] placeholders — fine for
  CP/M (we control both the BIOS and the emulator config, so they just have to agree).
