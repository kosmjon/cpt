# Segment map: cpt2489 (CPT 8100, 8515/25 with TTY)

The segment map of the second program disk, `cpt2489.td0`. Its TeleDisk comment reads
"Program disk for 8100, 8515/25 CPT word processor w/TTY", and it was imaged 1997-03-26. It is
compared here with the first disk, `cpt.img`, which is the same disk as `cpt2365.td0` ("Program
disk for CPT 8100 word processor"; the two decode byte for byte the same).

Segment numbers are given in hex and decimal. In a far call (`CALL 0046H`, then seg, func), both
bytes are the hex values. How segments, banks and the dispatcher work is in
[program-disk.md](program-disk.md), "Segments, banks and functions".

## How it was made

```
python tools/td0.py .kiro\specs\cpt8000-emulator\cpt2489.td0 --raw disk\cpt2489.img
cpt-run --cpt-boot disk\cpt2489.img 82000000 - 40000 disk\mem2489.bin
python tools/cptsegs.py disk\mem2489.bin [--calls]
```

The dump is taken after start-up (82M cycles), with nothing typed. The present flags are as the
system set them at start-up [CONFIRMED by reading the dump]. Whether any segment is loaded later,
on demand, hasn't been checked [VERIFY].

## Summary

- **33 segments present** out of 128 slots (cpt.img: 32): 7 in bank 0, 12 in block 0, 8 in
  block 1 and 6 in block 2.
- **415 far-call sites** (237 distinct functions); cpt.img has 423 (242).
- **New on this disk:** 17 (11H), 38 (26H) and **43 (2BH), the serial communications segment**.
- **Missing here, present on cpt.img:** 55 (37H) and 59 (3BH).
- **On the disk but not loaded:** segment **30 (1EH)**, the **message module for the disk
  utilities** (the text for erase, duplicate, backup, restore, reconstruct, sort and SRS). It's
  a module at logical 655 that operation 0 doesn't include; nothing calls it, and the
  utilities' own code isn't on this disk. See `program-disk.md`, "Disk utilities".
- **Moved to another block:** 14 (0EH) and 26 (1AH) from block 2 to block 1, 49 (31H) from
  block 1 to block 0, and 58 (3AH) from block 0 to block 2.
- Most other segments are where they are on cpt.img, but their function addresses have shifted
  by a few bytes (the code was rebuilt). For example, the editor loop 23.00 is C75FH here and
  C765H on cpt.img. Addresses found on one disk must be looked up again on the other.

## The map

"Sites" counts the far-call sites that call the segment, across all banks.

| Hex | Dec | Where | Table | Funcs | Sites | On cpt.img | What it is |
|---|---|---|---|---|---|---|---|
| 00 | 0 | bank 0 | 037FH | 1 | 17 | same | |
| 01 | 1 | bank 0 | 0615H | 5 | 8 | same | disk and system services: drive table, start/wait for a disk request, far-function lookup |
| 02 | 2 | bank 0 | 1513H | 3 | 5 | same | display pages (02.02 allocates a 256-byte page) |
| 04 | 4 | bank 0 | 16A6H | 1 | 1 | same | |
| 05 | 5 | bank 0 | 16C5H | 22 | 4 | same | |
| 07 | 7 | bank 0 | 1CEEH | 1 | 1 | same | |
| 08 | 8 | block 1 | 800AH | 8 | 4 | same | |
| 09 | 9 | block 2 | F3ABH | 1 | 1 | same | |
| 0A | 10 | block 0 | 800AH | 1 | 2 | same | |
| 0B | 11 | block 0 | 844CH | 3 | 3 | same | |
| 0C | 12 | block 2 | 800AH | 50 | 54 | same | **the filer** (0C.29 finds a document / probes a drive; 0C.00 loads one for the staple key) |
| 0E | 14 | block 1 | ED43H | 7 | 4 | block 2 | |
| 11 | 17 | bank 0 | 1DD5H | 1 | 0 | **not present** | probably the **serial driver** in bank 0: the 8251 code at 1DDDH–1F03H follows its table [HYPOTHESIS]. Its one function slot is empty, and nothing far-calls it |
| 12 | 18 | block 0 | 8668H | 5 | 16 | same | |
| 13 | 19 | block 1 | 8458H | 23 | 23 | same | |
| 14 | 20 | block 0 | 936DH | 2 | 10 | same | |
| 1A | 26 | block 1 | F509H | 2 | 2 | block 2 | |
| 23 | 35 | block 2 | B238H | 39 | 56 | same | **the editor** (23.00 is the editor loop) |
| 25 | 37 | block 0 | 995CH | 6 | 6 | same | |
| 26 | 38 | block 0 | 9F4BH | 1 | 1 | **not present** | **the communications and code-set prompt tables** [CONFIRMED by disassembly]. 26.00 returns the address of prompt table 5–12 (list at 9F5DH), for 2A.01. Table 8 (9F75H) is the code-set prompts `C0-9:` … `C250-255:` used by control pages such as C-ASCII.T (see `program-disk.md`, "How prompts and string tables are found") |
| 28 | 40 | block 0 | AA92H | 7 | 13 | same | on cpt.img, STOP (3FH) calls 28.02 |
| 29 | 41 | block 0 | B2C4H | 4 | 32 | same | messages (29.01 shows a message) |
| 2A | 42 | block 0 | B6DFH | 4 | 11 | same | title-line display (2A.02 shows the unit name) |
| 2B | 43 | block 1 | D3DDH | 6 | 6 | **not present** | **serial communications (TTY)**: drives an Intel 8251 USART at 4AH/4BH (below). CODE + I with the control page `CTTY` turns it on |
| 2E | 46 | block 1 | AFFAH | 7 | 8 | same | |
| 2F | 47 | block 1 | CBFCH | 10 | 9 | same | |
| 30 | 48 | block 0 | F4BAH | 2 | 1 | same | |
| 31 | 49 | block 0 | EC7BH | 1 | 2 | block 1 | the staple key's nested level (31.00) |
| 36 | 54 | block 0 | DF17H | 2 | 2 | same | |
| 37 | 55 | — | — | — | 7 | block 1, 9 funcs | **not present here**; the editor still calls it from 7 sites |
| 3A | 58 | block 2 | E23EH | 12 | 20 | block 0 | the screen: 3A.00 builds the display list, 3A.0B swaps the status row's jump |
| 3B | 59 | — | — | — | 1 | block 1, 1 func | **not present here**: key 5CH and CODE + N (`.00`, decimal alignment?) on cpt.img |
| 3D | 61 | block 2 | D729H | 14 | 21 | same | **the keyboard**: 3D.0A reads a key, 3D.00 gets the next key and plays PROG back |
| 3E | 62 | block 2 | EAFEH | 5 | 2 | same | |
| 3F | 63 | block 1 | D202H | 3 | 3 | same | |

"Same" means present in the same bank on cpt.img (the table address may differ by a few bytes).

**Called but not present** on this disk, so those calls do nothing: 03 (3), 19 (25), 1C (28),
1F (31), 20 (32), 21 (33), 22 (34), 24 (36), 27 (39), 33 (51), 37 (55), 38 (56), 3B (59),
41 (65), 42 (66), 43 (67), 44 (68), 46 (70), 47 (71), 48 (72), 49 (73), 66 (102), 69 (105),
78 (120), 7A (122).

Two of these are tied to keys (from cpt.img, where they're also absent):
- **56 (38H):** keys 01H, 1CH and 4CH, and CODE + 01H. Possibly math [HYPOTHESIS].
- **65 (41H):** key 0DH, probably WORDPAK [HYPOTHESIS].

## Serial communications: segment 43 (2BH) **[CONFIRMED by disassembly; user's recollection]**

The user remembers segment 43 as serial communications using the serial port. The code agrees:

- **An Intel 8251 USART** at port **4AH** (control/status) and **4BH** (data):
  - **E8CFH** (block 1) writes `00 00 00 40` to 4AH. That's the 8251's standard reset: three
    zeros to reach the command register, then 40H, the internal-reset command. It then writes 0
    to port **4CH** (role unknown; perhaps the baud rate or modem control [HYPOTHESIS]).
  - **DF8DH** waits for status bit 0 (TxRDY, transmitter ready). It then sets command bit 3
    (SBRK, send break), waits 25 ticks through the delay entry 005EH, and clears it again. So
    it sends a **break**.
  - More 8251 writes at E92DH, E93EH, E944H (4CH), E9E6H and E9FEH.
- **Bank 0, 1DDDH–1F03H**, reads the status and moves data through 4BH (IN 4AH, IN 4BH,
  OUT 4AH, OUT 4BH). It's probably the receive and transmit driver, perhaps
  interrupt-driven [HYPOTHESIS]. It sits right after segment 17's (11H) table.
- **The editor reaches it:** on cpt.img the editor's indices 27H and 33H (CODE + I and
  CODE + K) far-call segment 2B, which is absent there. On this disk, **CODE + I shows
  `CONTROL` on the title line** with a field after it [CONFIRMED by running it]. The disk also
  has documents named `CONTROL` and `CTTY`, probably the stored communications settings
  [HYPOTHESIS]. CODE + K showed nothing visible in a first try.
- **The emulator models it**: `crates/cpt-machine/src/cpt_usart.rs` (below).

### Turning it on, and what it does [CONFIRMED by running it]

**CODE + I, then `ctty`, then RETURN** loads the control page `CTTY`. The title line then shows
**`TTY COMMUNICATIONS ENABLED`**. `CTTY` reads:

```
SAMPLE ASYNC CONTROL PAGE   1200 BAUD
COMMUNICATIONS.
TRANSMISSION CODE SET:  1  C-ASCII.T
RECEPTION CODE SET:     1  C-ASCII.R
RATE:   1200
LINES PER PAGE:    70
EVEN PARITY
```

- The system loads both code tables, C-ASCII.T and C-ASCII.R. They're documents of
  `CODES n-m: ...` lines mapping each byte to a CPT code, identity for most of ASCII.
- **The chip set-up:** reset `00 00 00 40`, then `00` to 4CH, mode `4EH` (async ×16, 8 data
  bits, no parity, 1 stop bit) and command `37H` (TxEN, DTR, RxE, error reset, RTS).
- **The CPT sends `04 4E`** (EOT, `N`). It then sets its "finish" flag (ctl+38H), and the TX
  handler turns the transmitter off with command `16H`. It writes `09` to 4CH, probably the
  baud-rate select for 1200 [HYPOTHESIS].
- **Even parity is done in software.** The chip has no parity, the receive path can strip
  bit 7 (`ANI 7FH` when ctl+09H is set), and the flow-control bytes it compares against
  include their parity bit.
- **Flow control is XON/XOFF.** The control block (pointer at 0572H, here 2D00H) holds `93H`
  at +15H, which is XOFF (13H) with even parity. The RX handler checks for it and for XON
  (11H), and queues an XOFF itself when its buffer fills (1E99H–1EA4H).
- **Interrupts:**
  - The receiver is **RST 1** (1DD8H). It acknowledges `02H` on port 00H.
  - The transmitter is **RST 4** (1EB1H). It sends a priority byte (ctl+37H) first, then a
    4-byte ring (pointer at ctl+2EH).
  - Code that queues a byte writes `10H` to port 00H. The TX handler writes `10H` only after
    it has sent a byte. So a port-00H write is an end-of-interrupt that re-arms the level
    [HYPOTHESIS for the controller chip]. **The CPT depends on it** [CONFIRMED by running]:
    without the hold-off, TTY mode corrupts its own control block (see "The transmit
    interrupt storm" below). Not every handler writes its bit, though: the display retrace
    (RST 6) writes `40H` only a few times per boot, so the hold-off is modelled for the 8251's
    transmitter only.
- **Received characters reach the receive buffer at 3200H**, still with their parity bits
  (`48 C5 CC CC CF` for `HELLO`).
- **TTY works in the emulator** [CONFIRMED by running, 2026-11-03]: reception to the screen and
  to a document, and transmission from the keyboard. See "Using TTY mode" below.

### The emulator's 8251

`Usart8251` (in `CptSystem`, ports 4AH–4CH, RST 1 receive, RST 4 transmit):

- It follows the control sequence: mode, sync characters, command, internal reset.
- Status bits are the 8251's: TxRDY, RxRDY, TxEMPTY, PE, OE, FE, and DSR (on while a terminal
  is attached).
- Characters take their line time at 1200 baud by default.
- RxRDY is a plain level. TxRDY (masked by TxEN and CTS, as on the real chip) requests RST 4
  once, then is held off until `10H` is written to port 00H (`Usart8251::cpt`, through the
  new `Device::observe_out` hook, which lets a device see writes to other devices' ports).
  `Usart8251::new` keeps a plain level.
- Received characters wait until the CPU has read the previous one, so a pasted burst
  doesn't overrun. Characters that arrive while the receiver is off are lost.

`cpt-run --cpt-boot` connects it to a terminal or to scripted input:

```
cpt-run --cpt-boot disk\cpt2489.img 0 - 40000 --serial-listen 127.0.0.1:2323 --live --keys "{+77}i{-77}{ms 1500}ctty{14}" --keys-at 80000000
```

- **`--serial-listen <addr>`** waits for a Telnet client (e.g. PuTTY or MobaXterm, Telnet to
  127.0.0.1:2323). Add `--serial-raw` for a plain TCP connection.
- **0 cycles** runs until the terminal disconnects. Start-up runs as fast as it can; from
  80M cycles on, the run is paced to real time.
- **`--live`** redraws the CPT's screen in the console as it changes.
- **`--serial-in "<script>"`** (with `--serial-in-at <cycle>`) feeds the line without a
  terminal: text, `{XX}` hex bytes, `{ms N}` pauses.
- The report gets a `serial 8251` section: mode, command, control writes, bytes sent and
  received.
- Because of the software parity, a terminal should send 7 data bits with even parity for
  the CPT to see clean characters. Telnet clients send 8-bit data, so bit 7 is clear
  [VERIFY whether the CPT rejects bad parity or only strips it].
- The browser build has the device too, but nothing attached to it.

### How received characters flow **[CONFIRMED with `cpt-run --watch 3200-32FF`]**

Run with `--serial-in` sending `48 C5 CC CC CF 8D 0A` after `CODE + I`, `ctty` enabled the
port, and `--watch 3200-32FF`. For each byte:

1. The 8251 interrupts on RST 1; the handler at **1DD8H** reads the byte (4BH).
2. It calls **2042H** (bank 0), which stores the byte at 3200H, 3201H, … (seven bytes, in
   order, from the instruction at 206EH). 2042H is the driver's **put** entry, also in the
   jump table at 0130H.
3. About 2,500 cycles later the instruction at 2000H (inside **1FDEH**, the driver's
   **get** entry, jump-table slot **012AH**) reads the same byte back.
4. The only caller of 012AH is a service routine at **DE42H** (block 1, inside the address
   range of segment 43's function 01, D52EH–DF6CH). It checks flags in the control block
   (ctl+3CH, 3DH, 3FH), calls driver slot 0127H (what that does isn't read yet), and then,
   if 012AH returned a character, hands it to a per-character handler at **DEDAH**.

**The driver's jump table** (bank 0), the comm segment's way into the serial code:

| Slot | Jumps to | Used by |
|---|---|---|
| 0121H | 1F0AH | block 1 code at EBB0H and EC68H |
| 0124H | 1F66H | nothing seen |
| 0127H | 1F9EH | DE69H, just before the get (purpose not read yet; the receive interrupt also calls 1F9EH at 1E8BH) |
| 012AH | 1FDEH | DE8BH (get a character) |
| 012DH | 200FH | nothing seen |
| 0130H | 2042H | the receive interrupt (1E65H calls 2042H directly) |

Slots 0118H and 011EH (to 18F1H and 18F5H) belong to something else and are called from
block 1 code around 8348H–9669H.

### Using TTY mode [CONFIRMED by running, 2026-11-03]

1. **CODE + I, `ctty`, RETURN** loads the control page. Wait about 8 s: the title shows
   `TTY COMMUNICATIONS ENABLED`. The transmitter is left on (command 37H) with nothing to
   send. Nothing is sent at start-up.
2. **Reception defaults to the screen.** Text from the line appears below the typing line,
   parity stripped, one line per CR: `HELLO` CR LF `second` CR LF shows as two lines.
3. **CODE + K** opens `COMMUNICATIONS:  Stop Select Status`. A CODE + K pressed while the set-up
   is still running (3 s after `ctty`) is ignored.
4. Menus show on the title line. **Typing an item's letter does nothing. SPACE moves the
   highlight and RETURN picks it.** Where the highlight starts:
   - COMMUNICATIONS starts on Select: RETURN gives SELECTION. (Once a reception is running,
     it reopens on the item used last.)
   - SELECTION starts on Transmission: RETURN gives TRANSMISSION. SPACE then RETURN gives
     RECEPTION.
   - TRANSMISSION (`KB KB+Screen Screen Disk`) starts on KB.
   - RECEPTION (`None Screen Disk`) starts on None.
5. **Transmission, KB:** what is typed afterwards is sent as 7-bit ASCII with even parity made
   by the software, RETURN as CR. `hello` RETURN sends `E8 65 6C 6C 6F 8D` (`h` and CR get the
   parity bit). The text is also typed into the document.
6. **Reception, Disk** (SPACE RETURN twice from SELECTION) asks `FILE NAME:` with the station
   digit, then shows `RECEPTION IN PROCESS`. What arrives is filed as that document. LF is
   dropped and CR kept as the CPT's new line: `HELLO` CR LF `second` CR LF became
   `{01}RXTEST{02}HELLO{0D}second{0D}{0D}{03}` (23 bytes, the directory's length field 17H).
7. **Disk reception ends by itself** about 25 s after the last character: the title becomes
   `RECEPTION STOPPED` and the document is on the disk [HYPOTHESIS: an idle-line timeout;
   whether a control-page field sets it is unknown].

Boot tests: `tty_communications_set_up_the_8251_and_receive`,
`tty_transmission_from_the_keyboard_sends_what_is_typed`, `tty_reception_to_disk_files_a_document`.

### The transmit interrupt storm (the cause of the earlier wrong glyphs)

Before the fix, the emulator's 8251 requested RST 4 for as long as TxRDY was set, as a plain
level. The CPT's TX handler (1EB1H) returns without writing `10H` when it has nothing to send,
expecting the request to stay off. With the plain level the request came back after every
`EI; RET`, so the interrupted code never moved on. Traced with `--watch` and breakpoints:

1. The timer handler (116FH) enables interrupts at its first instruction. It was interrupted
   at 1171H by RST 4 again and again, so it never finished.
2. Each new timer tick nested another timer handler on top, and the stack grew by one frame
   per tick (frames `1171 / 0046 / 0972 / 094E` repeating).
3. That stack was the TTY task's, in page 2E00H (SP about 2E02H when the storm began, 2D51H
   0.2 s later). It ran down into page 2D00H, which the comm segment had just allocated (via
   the page allocator at 0F72H, cleared at EC10H) as its control block (pointer at 0572H).
4. The overwritten control block explained every symptom:
   - ctl+5DH holds the reception code-set pointers, indexed by the shift state at ctl+FBH
     (E6A3H). Both were stack words (state 1FH, pointer 0146H), so each received byte went
     through a table at 0146H: `A` (41H) became 16H. Those were the wrong glyphs.
   - The send ring pointer (ctl+2EH) pointed at garbage: the start-up "sent" `04` (and
     `04 4E` before the EI fix) was stack data, not an EOT handshake.
   - The overflow and break messages (11.23, 11.20) and the stray `8` on the typing line.
5. With the request held off until `10H` on port 00H, the reception tables at 2F00H/3100H are
   the ones used, and all of these are gone.

The code-set tables, for reference [CONFIRMED in memory after enabling]:

- The `CTTY` page (`SAMPLE ASYNC CONTROL PAGE 1200 BAUD`) names `TRANSMISSION CODE SET: 1
  C-ASCII.T`, `RECEPTION CODE SET: 1 C-ASCII.R`, `RATE: 1200`, `LINES PER PAGE: 70` and
  `EVEN PARITY`. CODE + I loads both tables. Nothing else needs to load them.
- **3100H is the transmission table** (`C-ASCII.T`: its `CODES 90-99` line
  `090,091,000,093,095,196,096…` matches 3100H+5AH). **2F00H is the reception table**,
  presumably `C-ASCII.R` [VERIFY: only its lines for 0–29 were read from the disk]. Both
  pass 00H–8FH through unchanged, except a few punctuation codes.
- Table documents are lines `CODES  lo- hi:  ddd,ddd,…` (decimal, ten per line; `000` means no
  code). `C-ASCII.T1` is a modified copy (code 13 → 000; 250–255 → `000,000,000,000,254,127`).
  `C-ASCII.T2` is a *string* table (`CODES 0-9: 013,005,013,010,000,…`), probably CR → CR ENQ
  CR LF [HYPOTHESIS]. Using them would mean editing the code-set lines of a control page
  [HYPOTHESIS].

### The communications menus

Prompts in the resident table (block 0, B6E8H) and their code in block 1:

| Prompt | Id | Code (block 1) |
|---|---|---|
| COMMUNICATIONS: =Off =On | 044D | D45B |
| COMMUNICATIONS: =Stop =Select =Status | 0457 | D54A |
| SELECTION: =Transmission =Reception | 0458 | D55E |
| TRANSMISSION: =KB =KB+Screen =Screen =Disk | 0461 | D571 |
| RECEPTION: =None =Screen =Disk | 0462 | D628, DED2 |
| DISK TRANSMISSION: =Reselect =Continue | 0463 | D58F, DEBF |
| PROCEDURE: =Begin =Wait for signal | 0464 | D606 |
| STOP: =Transmission =Reception | 0459 | D65F |
| DESTINATION: =CPU =Printer =Punch | 046D | EA42 |
| RATE | 046E | D5D7, D63C |
| STOP BITS | 046F | D5B0 |
| SCREEN SIZE | 0472 | block 0 F6CF |

CHARACTER LENGTH/PARITY, WRAPAROUND, AUTOMATIC NEW LINE, BLACK-ON-WHITE and TEXT UP CR
TRANSLATION (0470, 0471, 0473–0476) are not referenced by any code found.

The receive path, for reference: DE42H gets a character (B) through 012AH; DEDAH masks it
with ctl+08H (7FH) and checks parity by ctl+09H; E1F6H → E0A1H → E12EH; E37BH handles the
control codes (0EH/0FH shift out/in, 10H, 1BH, 05H, 07H) and translates through E6A3H; E24CH
puts it on the line.

### Communications: open questions

**The CPT side**

1. **Stop, Status, and the other transmissions** (KB+Screen, Screen, Disk) haven't been tried.
   Driving the menus blind is fiddly because COMMUNICATIONS reopens on the last item used.
2. **What ends Disk reception after 25 s**, and whether the control page sets it.
3. **Parity errors.** DEDAH computes a parity-error flag (E) from ctl+09H. What the CPT does
   with a bad character hasn't been traced. Telnet clients send 8-bit data with no parity bit.
4. **4CH's meaning is a guess.** `00` is written at reset, then `09` with `RATE: 1200`,
   probably a baud-rate select. The model records it but always runs at 1200 baud.
5. **The rest of the interrupt controller.** The hold-off is modelled for the 8251's
   transmitter only, and device priority is the order devices are attached (the 8251 last).
   The other handlers' port-00H writes (20H, 80H, 08H, 04H, 02H) are only logged.
6. **The other control-page fields** (`LINES PER PAGE: 70`, the code-set numbers) haven't been
   tried with other values. Nor have the other control documents: `CONTROL`
   (`CONTROL PAGE FOR MAILBOX`, `TRANSMISSION FROM KP/SCREEN`, `TRANSMISSION FILE: 2 MAILBOX`)
   and `WN` (`WORDNET. RATE: 9600`).

**The emulator side**

1. **No interactive CPT keyboard in a terminal session.** `cpt-run` only types its `--keys`
   script. During a `--serial-listen` session you can type on the terminal only.
2. **The Telnet server suppresses local echo.** It sends `WILL ECHO`, so the client doesn't
   echo locally. `--serial-raw` avoids the negotiation, but most clients then need local echo
   turned on by hand.
3. **No parity or CR/LF translation for the terminal.** The terminal's bytes arrive as sent,
   with no even-parity bit and Enter as CR (or CR LF).
4. **`--serial-listen` blocks before boot** until a client connects, and the run ends when
   the client disconnects.
5. **Receiver pacing hides overruns.** A character starts only after the CPU has read the
   last one, so the CPT's XOFF path is never exercised the way a fast host would exercise it.
6. **Modem signals are simplified.** DSR is on whenever a terminal or script is attached, and
   CTS is always on. DTR and RTS aren't passed to the host.
7. **`--live`** redraws at most twice a second, only while the serial port is active.
8. **The browser has no serial connection.** The device runs in the wasm build with nothing
   attached.

**Suggested next steps**

1. **Try Stop and Status**, and the Screen and Disk transmissions (DISK TRANSMISSION asks
   Reselect/Continue; PROCEDURE asks Begin/Wait for signal).
2. **Terminal comfort in `cpt-run`:** `--serial-parity even` (add the parity bit to the
   terminal's bytes), `--serial-echo`, CR / CR LF options, and forwarding console keystrokes
   to the CPT keyboard. These answer emulator issues 1–3.
3. **Find where `09` for 4CH comes from**, likely a table indexed by the `RATE:` value, so the
   model can run at the rate the CPT chose.
4. **A browser serial connection:** a WebSocket-to-TCP bridge in `run-web.bat`, or a terminal
   pane in the page attached to the wasm `Usart8251`.

The editor's far calls to 2B: 2B.07 (D61DH), 2B.04 (D647H), 2B.00 (D665H), 2B.01 (D67DH),
2B.05 (D6A7H) and 2B.03 (D6ADH), all from segment 23 in block 2.

## Function addresses

Function number: address. Empty slots are left out.

| Hex | Dec | Functions |
|---|---|---|
| 00 | 0 | 00:0388 |
| 01 | 1 | 00:0CDD 01:07DD 02:087C 04:1374 |
| 02 | 2 | 02:1632 |
| 04 | 4 | 00:16A9 |
| 05 | 5 | 0B:1B42 14:1830 15:1BB3 |
| 07 | 7 | 00:1CF1 |
| 08 | 8 | 00:801B 01:824B 02:8117 03:8370 05:80D5 06:8373 07:83D1 |
| 09 | 9 | 00:F3AE |
| 0A | 10 | 00:800D |
| 0B | 11 | 00:8453 01:8654 02:8657 |
| 0C | 12 | 00:806F 01:80D3 02:8442 03:8596 04:8D82 05:8BEE 06:92C8 07:9221 08:9A6C 09:8B32 0A:8EB6 0B:9037 0C:918D 0D:90A6 0E:8E01 0F:8E6A 10:8E5B 11:8E44 12:8E2D 13:8E78 14:8E17 15:A334 16:8A9E 17:8AF1 18:A29A 19:A0C9 1A:9B46 1B:8BAA 1E:81E3 1F:97FE 20:98AF 21:98E9 22:9932 23:9630 24:9F17 25:9E9C 26:9EAF 27:9456 28:B0EC 29:8CE1 2A:8953 2B:B135 2C:B131 2D:A358 31:A6DA |
| 0E | 14 | 00:ED52 01:EF77 02:EFE2 03:F327 04:F40E 06:F4D3 |
| 11 | 17 | (none) |
| 12 | 18 | 00:8B87 01:8673 02:91E2 03:9259 04:8AEE |
| 13 | 19 | 00:8F27 01:88FF 02:906A 03:A787 04:8897 05:93EA 06:A686 07:A8A7 08:8487 09:87EC 0A:895B 0B:A317 0C:90BB 0D:883D 0E:902E 0F:9170 10:AF5B 11:96A3 12:9520 14:A31B 15:A322 16:8978 |
| 14 | 20 | 00:991A 01:94AD |
| 1A | 26 | 00:F50E 01:F6BE |
| 23 | 35 | 00:C75F 01:CCF3 02:C561 03:B5CD 04:B68D 05:B420 06:D074 07:D225 08:B2D0 09:C345 0A:C143 0B:B873 0C:B85B 0D:B75B 0E:B74B 0F:B9B0 11:C290 12:C2E1 13:C415 14:C3B0 15:C302 16:C314 17:C6CD 18:C4A1 19:B6F4 1A:B651 1B:B44D 1C:B3FB 1D:BE01 1E:D330 1F:D46C 20:B46C 21:B6D6 22:C8F7 26:C3DB |
| 25 | 37 | 00:9969 01:9DBD 05:9E61 |
| 26 | 38 | 00:9F4E |
| 28 | 40 | 00:AB67 01:AC88 02:B017 03:ACF5 04:B1FD 05:B125 06:AAA1 |
| 29 | 41 | 01:B34B 02:B2CD 03:B595 |
| 2A | 42 | 01:BFB8 02:DE22 03:DEB5 |
| 2B | 43 | 00:D3EA 01:D52E 03:DF6D 04:E052 05:E6F8 |
| 2E | 46 | 00:B31E 01:BD49 02:BCEC 03:BD15 04:C1A9 05:B009 06:B5EF |
| 2F | 47 | 00:CC61 01:CEF7 02:CD8A 03:CCB5 04:CC11 05:CD34 06:CCFD 07:CC57 08:CD81 09:D1AA |
| 30 | 48 | 00:F86F 01:F9F7 |
| 31 | 49 | 00:EC7E |
| 36 | 54 | 00:DF1C 01:E0D6 |
| 3A | 58 | 00:E257 01:E576 02:E457 03:E8AC 04:E52A 05:E8DA 06:E8CE 07:EA61 08:EA89 0B:E441 |
| 3D | 61 | 00:D9A1 01:DBE7 02:DBD7 03:DD24 04:DCC3 05:DDEE 06:DE06 07:DDB4 08:DDAA 09:DDA0 0A:D746 0B:DEAC 0C:E198 0D:E1D3 |
| 3E | 62 | 00:EC86 01:EB09 02:F0F1 03:EDAD 04:ED6B |
| 3F | 63 | 00:D209 01:D27A 02:D296 |

## The disk's documents

28 documents (`python tools/cptfs.py disk\cpt2489.img`): ACTIV.RPT, C-ASCII.R, C-ASCII.T,
C-ASCII.T1, C-ASCII.T2, CONTROL, CTTY, D, DIRECTIONS, ITIN.GB3, JJ2, JP-M24, JT8D200.10,
KELLENBERG, OP, OP.00.0, PH-L1, PH-L8, RK-L2.4, RP-M1, RR.DERBY, TOP20, TP, TP.50045,
TP.50055, TRT, TRT1, WN. `CONTROL`, `CTTY` and the `C-ASCII` set look like communications
settings and character tables [HYPOTHESIS].
