# CPT 8000 Emulator — Implementation Plan

- [x] 1. Set up the cargo workspace and shared ISA crate
  - Workspace created with `mol80asm`, `crates/i8080-isa`, `crates/i8080-cpu` (link/machine/wasm to come).
  - `i8080-isa`: opcode-indexed 256-entry table (mnemonic + length) with `lookup`, `disassemble`, `disassemble_all`. Shared by both tools: `mol80asm`'s transpiler now sources its mnemonics from it, and it provides the emulator's disassembly. Cross-check test proves the table's lengths match the CPU's actual PC advance for every opcode.
  - _Requirements: 9.4_

- [x] 2. Implement the 8080 CPU core (`i8080-cpu`)
  - [x] 2.1 `Cpu` state (registers, SP, PC, `Flags`, interrupt-enable, halted, cycles) and the `Bus` trait (+ `FlatMemory` for tests).
    - _Requirements: 1.1, 1.3_
  - [x] 2.2 Decode + execute all 256 opcodes (incl. documented NOP/JMP/CALL/RET aliases) with correct register/memory effects.
    - _Requirements: 1.1_
  - [x] 2.3 Flag computation (S/Z/AC/P/CY), including DAA and the ANA aux-carry quirk.
    - _Requirements: 1.2_
  - [x] 2.4 Stack ops (PUSH/POP/CALL/RET/XTHL/RST) and SP behavior.
    - _Requirements: 1.3_
  - [x] 2.5 Interrupt model (`interrupt(rst_vector)`, HLT wake) and per-instruction cycle counts from `step`.
    - _Requirements: 1.4, 1.5_
  - [x] 2.6 Conformance: 8080PRE, TST8080, CPUTEST all pass; **8080EXM exhaustive exerciser passes every CRC group** (release, ~15s). Plus 15 focused unit tests.
    - _Requirements: 1.1, 1.2, 1.3_

- [x] 3. Implement banked memory (`cpt-machine`)
  - [x] 3.1 `BankedMemory` with resident bank 0 and a vector of physical 32K blocks for bank 1.
    - _Requirements: 2.1, 2.2_
  - [x] 3.2 Address routing: <0x8000 → bank 0; ≥0x8000 → selected block at offset.
    - _Requirements: 2.1, 2.2, 2.4_
  - [x] 3.3 Bank-select via `io_out` to the configured port; de-selected blocks persist (verified end-to-end with the CPU executing an OUT).
    - _Requirements: 2.3, 2.4_
  - [x] 3.4 Bank 0 is all RAM, so no ROM write-protection is modeled (per the confirmed memory model).
    - _Requirements: 2.6_
  - [x] 3.5 Tests: remap on bank write, persistence of de-selected blocks across swaps, CPU-driven OUT, encoding mask, initial selection (8 tests).
    - _Requirements: 2.3, 2.4_

- [x] 4. Define `MachineConfig` and the bus/device dispatch
  - [x] 4.1 `MachineConfig` centralizing the bank-select port, encoding, block count, and initial block ([VERIFY] device ports to be added with each device).
    - _Requirements: 2.3, 2.5_
  - [x] 4.2 `Bus` implemented on `Machine`, routing the bank-select port; unmapped ports recorded (not crashed) for later discovery.
    - _Requirements: 2.5_
  - [x] 4.3 `Device` trait (ports/io_in/io_out/tick/take_irq + as_any_mut downcast). `Machine` owns devices, routes ports, panics on port collisions, records unmapped ports.
    - _Requirements: 2.5, 1.4_

- [x] 5. Implement the executive loop and interrupts
  - `Executive` steps the CPU, ticks devices with elapsed cycles, and delivers a pending IRQ each step. `run_until_halt`/`run_cycles` with a terminal-halt (HLT + interrupts disabled) stop condition. Verified: timer device raises an IRQ that wakes a halted CPU and vectors to the RST handler.
  - _Requirements: 1.4, 1.5_

- [~] 6. Implement the display device and page buffer
  - [x] 6.1 `DisplayDevice` with memory-mapped character RAM (80×~60) in bank 0; CPU writes route to it; `snapshot() -> PageSnapshot` with region layout.
    - _Requirements: 3.3, 3.1_
  - [x] 6.2 Region-structured buffer: title / top-half text / typing-line ruler / small (~8-line) bottom-half text / status lines (exact splits [VERIFY]).
    - _Requirements: 3.5_
  - [x] 6.3 **Vertical-retrace interrupt** via `tick`/`take_irq` — essential so CPT's retrace handler drains queued scroll/screen actions; verified end-to-end (handler scrolls display driven only by the IRQ).
    - _Requirements: 1.4, 3.3_
  - [ ] 6.4 Placeholder char set/font until the real char ROM is known ([VERIFY]) — deferred to the UI/font layer.
    - _Requirements: 3.4_

- [ ] 7. Implement the keyboard device
  - Queue key events from the UI; present to CPU via ports/IRQ ([VERIFY] scan codes/matrix).
  - _Requirements: 5.1, 5.2, 5.3_

- [ ] 8. Implement the file-backed data disk
  - [ ] 8.1 Data-disk device over a backing host file (read-block/write-block/flush).
    - _Requirements: 6.1, 6.3, 6.5_
  - [ ] 8.2 Expose it at the I/O ports/commands the B-tree filer source uses ([VERIFY] interface/layout).
    - _Requirements: 6.2, 6.4_
  - [ ] 8.3 Exercise the B-tree filer (Segment 12) against the data disk once the system builds.
    - _Requirements: 6.4_

- [ ] 9. Link-editor and system image (`cpt-link`)
  - [ ] 9.1 Consume `mol80asm` relocatable objects (symbols, references, `bank0` tag).
    - _Requirements: 7.1_
  - [ ] 9.2 Place `bank0`-tagged segments in the resident 32K; error on overflow. Reserve the work-block heap region ([VERIFY] from allocator source).
    - _Requirements: 7.2, 7a.1_
  - [ ] 9.3 Bin-pack relocatable segments into the switchable 32K blocks (first-fit-decreasing; report overflow).
    - _Requirements: 7.3_
  - [ ] 9.4 Resolve references; record cross-bank calls in the segment table.
    - _Requirements: 7.7_
  - [ ] 9.5 Generate the bank-0 segment→(bank,offset) map and fixed-address table in the handler's expected format ([VERIFY]).
    - _Requirements: 7.4_
  - [ ] 9.6 Emit the system image (bank 0 + blocks + table locations + heap region + entry point).
    - _Requirements: 7.5_
  - [ ] 9.7 Tests: packing fits/overflows, bank-0 placement, byte-exact tables once format known.
    - _Requirements: 7.2, 7.3, 7.4_

- [ ] 9b. System-image loader in `cpt-machine`
  - Load an image into bank 0 + all 32K blocks; set PC to the main entry point.
  - _Requirements: 7.6_

- [ ] 10. Wasm boundary (`cpt-wasm`)
  - Expose `Machine`: power_on/reset, load_image, attach_data_disk, run_frame, page_snapshot, key_down/up, debug accessors.
  - _Requirements: 8.3_

- [ ] 11. React UI shell (`ui/`)
  - [ ] 11.1 Vite + React + TS project wrapping the wasm module.
    - _Requirements: 8.1, 8.3_
  - [ ] 11.2 `Screen.tsx`: canvas rendering the 80×54 page-white page from `page_snapshot()`.
    - _Requirements: 3.1, 3.2, 8.1_
  - [ ] 11.3 `TypingLine.tsx`: honor the fixed-typing-line convention (document moves, row fixed).
    - _Requirements: 4.1, 4.2, 4.3, 4.4_
  - [ ] 11.4 `Keyboard.tsx`/`Keyboard.ts`: render the four CPT key clusters as clickable keys, map DOM key events to `CptKey` → `key_down/up`.
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 8.2, 8.5_
  - [ ] 11.5 `Controls.tsx`: power/reset, load system image, attach data disk.
    - _Requirements: 8.4_
  - [ ] 11.6 Accessibility pass: contrast, keyboard operability.
    - _Requirements: 8.5_
  - [ ] 11.7 Drive `run_frame()` on requestAnimationFrame and repaint.
    - _Requirements: 8.2_

- [ ] 12. Debugger
  - [ ] 12.1 Expose registers/flags/PC/SP; single-step; memory view.
    - _Requirements: 9.1, 9.2_
  - [ ] 12.2 Address breakpoints that pause on PC match.
    - _Requirements: 9.3_
  - [ ] 12.3 Disassembly view reusing `i8080-isa`.
    - _Requirements: 9.4_
  - [ ] 12.4 Bank-aware memory view (selected block + view any physical block).
    - _Requirements: 9.5_
  - [ ] 12.5 Optional segment resolution from the in-memory segment table ([VERIFY] format).
    - _Requirements: 9.6_

- [ ] 13. Cross-spec validation loop
  - Assemble a small MOL80 program with `mol80asm`, load the binary, run headless, assert state.
  - _Requirements: 7.4_

- [ ] 14. Resolve [VERIFY] hardware details against CPT references
  - Fill `MachineConfig` (bank-select port + encoding + block count, device ports), bank0 ROM/RAM split,
    bank-switch timing, floppy geometry + controller interface, display memory model + char ROM,
    key matrix, IRQ sources, IPL sequence, segment-table format. Replace placeholders; extend tests.
  - _Requirements: 2.3, 2.6, 3.3, 3.4, 5.1, 6.2, 6.3, 7.1, 9.6_
