# CPT 8000 Emulator — Design

## Context

The emulator runs the **complete CPT word-processing system** — the full MOL80 source we have
(everything that was on a Program Disk) — on an emulated Intel 8080 with CPT's distinctive
hardware: a two-bank 64K memory with a switchable upper half, a page-oriented 80×54 page-white
display, a Selectric-style fixed typing line, a keyboard, and a simple file-backed data disk.

**No original media/ROM dumps.** Rather than booting real disks, a host-side **link/image
step** compiles every segment with `mol80asm`, bin-packs them into bank 0 + the switchable
32K blocks, generates the bank-0 tables the long-call handler expects, and emits a **system
image** the emulator loads before starting the CPU at the main entry point (see "Link-editor
and system image" below).

A guiding principle: **the emulator models hardware; the CPT software runs on it.** The
monitor, interrupt handlers, long-call handler, heap allocator, editor, and filer are all CPT
system software compiled from our source — not reimplemented. The emulator supplies only the
hardware primitives they rely on (the bank-select port, persistent physical blocks, device
ports). This keeps the emulator faithful and avoids guessing at software behavior.

Because many hardware specifics are still `[VERIFY]` (port addresses, display
memory model, char set, key matrix), the design puts each device behind a trait/interface with
a single configuration point, so filling in a confirmed value is a local change — never a
re-architecture.

## Language & topology

**Rust core compiled to WebAssembly, React/TypeScript UI.**

- The CPU stepping loop and device model need predictable performance with no GC pauses — Rust
  fits, and the same core runs native (for headless tests/CLI) and in the browser (via Wasm).
- React renders the screen, keyboard, and machine controls; it is a thin shell over the core.
- The ISA/disassembly metadata is shared with the assembler (a common `i8080-isa` crate), so
  the debugger's disassembler and the assembler's transpiler agree by construction.

```
cpt-workspace/                     (cargo workspace; shared with mol80asm where sensible)
  crates/
    i8080-isa/      # shared opcode/mnemonic metadata (used by asm transpile + emu disasm)
    i8080-cpu/      # pure 8080 CPU core: registers, flags, step(), cycle counts
    cpt-link/       # link-editor: objects -> bin-packed banks + bank-0 tables -> system image
    cpt-machine/    # CPT hardware: banked memory, bus, bank-select, devices, image loader
    cpt-wasm/       # wasm-bindgen boundary exposing the machine to JS
  ui/               # React + TypeScript app (Vite), renders display, handles input
    src/
      Screen.tsx          # 80x54 page-white canvas renderer
      TypingLine.tsx      # fixed-typing-line document model view
      Keyboard.ts         # DOM key events -> CPT key events
      Controls.tsx        # power/reset, load system image, attach data disk, debugger
      emu/                # TS wrapper around the wasm module
```

## CPU core (`i8080-cpu`)

- Pure, hardware-agnostic 8080: `Cpu { a,b,c,d,e,h,l, sp, pc, flags, int_enabled }`.
- `flags`: carry, zero, sign, parity, auxiliary carry — set per the 8080 spec (Req 1.2).
- The CPU talks to the outside world through a `Bus` trait, so it has no knowledge of banking
  or devices:

```rust
pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, val: u8);
    fn io_in(&mut self, port: u8) -> u8;
    fn io_out(&mut self, port: u8, val: u8);
}

impl Cpu {
    pub fn step(&mut self, bus: &mut dyn Bus) -> u32; // returns cycles consumed
    pub fn interrupt(&mut self, rst_vector: u8);      // 8080 interrupt model (Req 1.4)
}
```

- `step` executes one instruction and returns its cycle count (Req 1.5). Exact
  cycle-accuracy target is [VERIFY]; cycle totals are tracked regardless so timing can be
  tightened later without API change.
- Decoding is driven by a match over the opcode; mnemated metadata for disassembly lives in
  `i8080-isa` so the debugger and the assembler share one table.

## CPT machine (`cpt-machine`)

Implements `Bus` and ties the banked memory and devices to the CPU.

### Banked memory (Req 2)

```rust
pub struct BankedMemory {
    bank0: Box<[u8; 0x8000]>,         // resident 0000-7FFF, all RAM: monitor, handlers, long-call
    blocks: Vec<Box<[u8; 0x8000]>>,   // physical 32K blocks for the 8000-FFFF window
    selected: usize,                  // index of the block currently mapped to bank 1
}
```

- `read/write` for `addr < 0x8000` → `bank0`; for `addr >= 0x8000` → `blocks[selected]` at
  `addr - 0x8000`.
- De-selected blocks are retained in `blocks`, so swapping preserves their contents — exactly
  what the long-call save/restore sequence depends on (Req 2.4).
- **Bank-select port:** an `io_out` to the bank-select port sets `selected`. The port address,
  the bit encoding of the bank number, and `blocks.len()` are configuration values ([VERIFY]),
  centralized in one `MachineConfig` so a confirmed value is a one-line change:

```rust
pub struct MachineConfig {
    bank_select_port: u8,       // [VERIFY]
    block_count: usize,         // physical 32K blocks; 8 for a 256K machine (RAM size / 32K)
    display_ports: PortRange,   // [VERIFY]
    keyboard_ports: PortRange,  // [VERIFY]
    disk_ports: PortRange,      // [VERIFY]
    // (no font-ROM field: the authentic ROM is unobtainable; the UI uses a substitute font)
    // ... other device port assignments
}
```

- Bank 0 is **all RAM**; it is populated by loading the system image, not backed by resident
  ROM, so there is no ROM write-protection to model.
- The **long-call handler is not implemented here.** It is CPT code (a `bank0` segment) in
  `bank0`. The machine only guarantees the primitives: fixed bank 0, a selectable/persistent
  bank 1, and the bank-select port. Long-call behavior emerges from executing bank-0 code.
- There is **no runtime segment loader**: segments are placed into banks by the host-side
  link/image step before the CPU starts (below), not read from disk at boot.

## Link-editor and system image (`mol80link`)

Because we have the full system source but no media, a host-side link step builds what a
Program Disk + boot loader would otherwise have produced. This is a new tool alongside the
emulator (and `mol80asm`), conceptually `mol80link`.

Inputs: the relocatable objects `mol80asm` emits for every segment (symbols, references, and a
`bank0` vs. relocatable tag per segment). Pipeline:

1. **Place bank-0 segments.** All `bank0`-tagged segments go into the resident 32K. Error if
   they overflow 32K (bank 0 must hold the runtime + the generated tables + the work-block
   heap region).
2. **Bin-pack relocatable segments** into the configured switchable 32K blocks (8 blocks for a
   256K machine). This is a bin-packing problem (items = segment sizes, bins = 32K blocks);
   a first-fit-decreasing heuristic is the starting point, with the packer reporting overflow
   if the segments don't fit the available blocks. Each segment gets a `(bank, offset)`.
3. **Resolve references.** Intra-bank references resolve to absolute addresses; inter-segment
   calls that cross banks are expected to go through the long-call mechanism, so the linker
   records them in the segment table rather than patching a raw address.
4. **Generate the bank-0 tables** in the exact binary layout the long-call handler source
   reads: the **segment→(bank, offset) map** and the **fixed-address table**. (These formats
   are [VERIFY] — they come from the handler source, not our choice.) Reserve the **work-block
   heap** region per the allocator source.
5. **Emit the system image**: a file describing bank 0's bytes, each 32K block's bytes, the
   table locations, the heap region, and the **main entry point**.

The emulator's loader reads the image, fills bank 0 and every physical block, and sets PC to
the entry point. This cleanly separates "build the memory layout" (host, one-time) from "run
the software" (emulator). The object/image formats are defined jointly with the assembler spec.

### Bus dispatch

`cpt-machine`'s `Bus::io_in/io_out` routes a port to the matching device (bank-select,
display, keyboard, disk) per `MachineConfig`. Unmapped ports log a debug diagnostic and
return a benign value, so unknown port pokes don't crash the machine while we're
still discovering the port map.

### Devices (behind traits so `[VERIFY]` specifics are isolated)

```rust
pub trait Device {
    fn io_in(&mut self, port: u8) -> u8;
    fn io_out(&mut self, port: u8, val: u8);
    fn tick(&mut self, cycles: u32);     // advance device time for timing/IRQ
    fn take_irq(&mut self) -> Option<u8>; // interrupt request vector, if any
}
```

- **Display device:** owns the **scanned display memory** — the region the video hardware
  reads each frame — into which the CPT software loads the current page's bytes. This is the
  hardware boundary the emulator models. Two subtleties:
  - **In-band control codes.** Not every byte is a character. Some byte values are
    **control/attribute codes the video hardware interpreted** — notably selecting
    **black-on-white vs. white-on-black (inverse) per line**. The display device / renderer
    must interpret these as rendering-state changes rather than emit a glyph. The exact codes
    are [VERIFY] (user will provide); the model is an attribute layer walked alongside the
    character bytes. The `page_snapshot()` the UI consumes should therefore expose per-cell (or
    per-line) attributes, not just raw codes.
  - **Font.** The display board's character-bitmap ROM is **not obtainable**, so a **faithful
    substitute monospaced font is the permanent renderer**, not a placeholder awaiting a dump.
    The character-code→glyph mapping is modeled; only the pixel shapes are approximated.
  - **Software vs. hardware boundary:** the CPT software is believed to keep the page as a
    **linked list of lines** in RAM and project the visible window into this scanned display
    memory on scroll/edit. The emulator does **not** model the linked list — it renders only
    the scanned display memory. [HYPOTHESIS]
  - Whether scanned display memory is a dedicated display-board region or a window into
    main/banked RAM is [VERIFY]; modeled here as a device-owned region behind the display
    ports so it can be re-pointed into banked RAM without API change.
  - Display RAM layout, cell size, attributes, and the status band/ruler encoding are [VERIFY].
- **Keyboard device:** accepts queued key events from the UI and presents them to the CPU via
  its ports/IRQ. The layout is known (four clusters from the keyboard photo: left function
  column incl. `MAR SET`/`MAR REL`, typewriter block, center editing/navigation cluster with
  unit selectors `CHAR`/`WORD`/`LINE`/`PARA`/`PAGE` and `UP`/`DOWN` navigation, numeric
  keypad). The emulator uses an abstract `CptKey` enum per physical key; the **byte each key
  sends** (and `CODE`-shifted meanings) is a single lookup table marked [VERIFY] until the
  source confirms the codes/matrix — so filling it in is a local change.
- **Data-disk device:** a single **file-backed data disk** (no 8-inch floppy hardware
  emulation). It models whatever I/O ports/commands the B-tree filer source uses, over a
  backing host file. The filer's own sector/block model (from its source) determines the
  file layout [VERIFY]; the device abstraction (read-block/write-block/flush) is stable above
  those details. Documents persist to the backing file.
- **Bank-select "device":** trivial — captures writes to the bank port and updates
  `BankedMemory.selected`.

### Interrupts & scheduling

A simple executive drives the system:

```
loop {
    let cycles = cpu.step(&mut machine);
    machine.tick(cycles);                 // advance devices
    if let Some(vec) = machine.take_irq() // display/keyboard/disk/timer IRQs
        { if cpu.int_enabled { cpu.interrupt(vec); } }
}
```

Interrupt sources (timer for the multitasking monitor, keyboard, disk completion) are
[VERIFY]; the executive shape accommodates them without change.

## Display & the fixed typing line (Req 3, 4)

This is the defining UX and gets explicit modeling.

- **Page buffer:** the display device yields an 80×54 grid of characters (plus any attributes
  once known). The UI renders it **page-white**: dark glyphs on a white background, one page
  at a time — no terminal-style scroll (Req 3.1, 3.2, 3.5).
- **Fixed typing line:** the real machine keeps the insertion row fixed and moves the document
  past it (Selectric metaphor). How this is realized is primarily a property of the CPT
  software writing to display memory; the emulator reproduces it faithfully simply by
  rendering what that software puts in display memory. The UI's `TypingLine` component
  additionally honors the fixed-row convention for any emulator-side cursor affordance so the
  experience reads as "document moves, row stays" (Req 4.1–4.4). Which physical row is the
  typing line is [VERIFY].
- **Rendering:** a `<canvas>` in `Screen.tsx` draws the page from the `PageBuffer` snapshot the
  core exposes each frame. A monospaced bitmap font approximates the CPT glyphs until the real
  character ROM is available ([VERIFY]); contrast and keyboard operability satisfy basic
  accessibility (Req 8.5).

## Wasm boundary (`cpt-wasm`) and UI (Req 8)

- `wasm-bindgen` exposes a `Machine` handle to JS:
  - `new(config)`, `power_on()`, `reset()`
  - `load_image(bytes)` — load a linked system image into bank 0 + the 32K blocks
  - `attach_data_disk(bytes)`, `data_disk_bytes()` — the file-backed data disk
  - `run_frame()` — step a batch of cycles (e.g. one video frame's worth) for smooth UI
  - `page_snapshot()` — returns the 80×54 buffer for rendering
  - `key_down(code)`, `key_up(code)`
  - debug: `registers()`, `read_mem(block, addr, len)`, `selected_block()`,
    `set_breakpoint(addr)`, `step_instruction()`
- React calls `run_frame()` on `requestAnimationFrame`, then repaints from `page_snapshot()`.
  Input handlers forward key events via `key_down/up` (Req 8.2, 8.3).
- Native builds (no Wasm) run the same `cpt-machine` headless for integration tests and a CLI
  runner, so correctness can be validated without the browser.

## Debugging (Req 9)

- Register/flag/PC/SP inspection, single-step, memory view, address breakpoints.
- **Bank-aware memory view:** shows the selected bank-1 block and lets you view any physical
  block (Req 9.5) — essential for inspecting bank-switched code.
- Disassembly reuses `i8080-isa` (shared with the assembler's transpiler).
- Optional **segment resolution:** if the segment → (bank, offset) table can be located in
  memory, the debugger annotates addresses with segment numbers to help trace long-calls
  (Req 9.6). Table format is [VERIFY].
- Optional **logical-document view:** if the CPT software's linked-list-of-lines page structure
  is documented in the source, the debugger can walk it to display the logical document
  (distinct from the flattened scanned display memory). Node format is [VERIFY]/[HYPOTHESIS].

## Validation strategy

- **CPU conformance:** run an established 8080 test suite (e.g. the CP/M diagnostic programs
  like `8080PRE`/`CPUTEST` style ROMs) headless and assert expected results. This validates
  the CPU core independently of CPT specifics.
- **Banking tests:** unit tests that write the bank port, confirm the upper window remaps, and
  confirm de-selected blocks retain contents across swaps.
- **Assembler ↔ link ↔ emulator loop:** assemble small MOL80 segments with `mol80asm`, link
  them into a system image with `cpt-link` (exercising bin-packing + table generation), load
  the image, run headless, and assert memory/registers — closing the loop across all three
  tools.
- **Link tests:** bin-packing fits/overflows, bank-0 placement, and byte-exact generated
  tables once the handler's expected format is known.
- **Device tests:** per-device unit tests against the (confirmed) port behavior as values land.

## Risks & open questions

- `[VERIFY]` values (bank-select port + encoding, bank-0-vs-pool split, bank-switch timing,
  display memory model + **in-band control/attribute codes** incl. inverse-video, key matrix,
  IRQ sources, data-disk interface, and — from the system source — the
  segment-table/fixed-address-table formats, `bank0` tagging, entry point, and heap region)
  are isolated behind `MachineConfig`, the device traits, and the link step. (The font ROM is
  not an open item — it is unobtainable, so a substitute font is the permanent plan.)
- The ultimate test is **running the linked system image** of the real CPT source. Since we
  have that source (no dumps needed), the critical dependencies are the link step producing
  correct bank-0 tables and the assembler's Layer-2 coverage. Until the full system builds,
  validation leans on the CPU conformance suite and the assembler↔emulator loop.
- **Bin-packing** must respect any cross-segment assumptions (e.g. segments that expect to
  share a bank, or alignment requirements); first-fit-decreasing is a starting heuristic that
  may need constraints once the source reveals them. [VERIFY]
- Cycle-accuracy fidelity target is undecided; the API tracks cycles so it can tighten later.
