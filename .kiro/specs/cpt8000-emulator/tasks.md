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

- [~] 7. Implement the keyboard device
  - [x] `KeyboardDevice`: FIFO of key events, STATUS/DATA ports, **interrupt-driven** (RST on key event) matching the real N-key-rollover keyboard; make/break (press/release) events via `push_key`/`push_key_up`. Verified end-to-end (key IRQ -> handler reads code). Wasm `cpt_key_down`/`cpt_key_up` wired; web UI forwards the real PC keyboard for non-special keys and the on-screen panel for special keys.
  - [ ] Replace placeholder scan/key codes, ports, and make/break encoding with the real ones ([VERIFY] from source).
  - _Requirements: 5.1, 5.2, 5.3_

- [~] 8. Implement the file-backed data disk
  - [x] 8.1 `DiskImage` backing store (block read/write/flush) over a host file or in-memory; `Drive` + two stations **A and B**, runtime mount/eject (program disk removable; A<->B copy supported).
    - _Requirements: 6.1, 6.3, 6.5_
  - [x] 8.2 `DiskController` `Device` exposes A/B via 5 I/O ports with a **placeholder** command protocol (station/block/command/data), isolated for easy replacement; driven end-to-end by the CPU. Real port map/protocol [VERIFY] from the filer source.
    - _Requirements: 6.2, 6.4_
  - [ ] 8.3 Exercise the B-tree filer (Segment 12) against the data disk once the system builds, and replace the placeholder protocol with the real one.
    - _Requirements: 6.4_

- [ ] 9. Link-editor and system image (`cpt-link`) — **BLOCKED on the MOL80 object-module
      model** (local/public symbols, intra-segment-call vs. long-call, relocation records; see
      assembler requirements "MOL80 modules, relocation, and the object format"). Do NOT build
      until those are known — the object format and relocation model that define a linker all
      derive from them. Bin-packing is the only source-independent part. The one piece of
      non-speculative groundwork is 9.0 below.
  - [x] 9.0 (groundwork, not blocked) `mol80asm` emits **relocation records** (`Relocation {
        offset, width, symbol }`) for every origin-relative 16-bit address — i.e. addresses
        resolved from in-module labels; literal addresses get none. `Assembly.relocations` +
        `--reloc` CLI dump. Tested. This is the one piece any relocatable scheme needs regardless
        of MOL80's eventual object format.
  - [ ] 9.1 Consume `mol80asm` relocatable objects (symbols, references, `bank0` tag). [BLOCKED]
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

- [ ] 9c. System build driver (`cpt-build`) — planned. Builds the dozens of `.mol` modules
      into a bootable system disk image with one command, replacing ad-hoc scripts like
      `cpm/build-cpm.ps1`. A Rust binary in the workspace rather than GNU Make: Make isn't on
      the Windows dev machine, and the link / bin-pack / segment-table / image steps have
      multi-output dependencies a makefile expresses poorly. The skeleton is buildable now;
      the link step is **BLOCKED** on the same object-module facts as task 9.
  - [ ] 9c.1 Project manifest (e.g. `system.toml`): segments, the `.mol` modules in each,
        fixed placements (bank 0 resident code, interrupt vectors, entry point), output image
        path. Mirror the original CPT build/link control files if they turn up with the source.
  - [ ] 9c.2 Incremental assembly: call `mol80asm` as a library (no process per file);
        re-assemble a module only when its source hash changes; cache objects under
        `target/cpt-build/`; assemble modules in parallel; report all diagnostics.
  - [ ] 9c.3 Interim pipeline (not blocked): treat each segment as one assembly unit (its
        modules concatenated, one symbol table), so no linker is needed yet; then pack
        segments into blocks and write the image. Gets an end-to-end build running before the
        object format is known; the manifest stays the same when the real linker drops in.
  - [ ] 9c.4 Link step: invoke `cpt-link` (task 9) per segment, then the bin-pack and
        segment-table generation (9.2-9.5). [BLOCKED on task 9]
  - [ ] 9c.5 Image writer: emit the system disk image in the layout the CPT boot path expects
        ([VERIFY] from source), reusing the image code from the CP/M work (`cpm/mkcpm.mjs`,
        `cpm/tools/cpmfs.py`) ported to Rust.
  - [ ] 9c.6 Single entry point `cargo run -p cpt-build -- system.toml`, plus tests: rebuild
        skips unchanged modules, a changed module triggers only its segment, and the image is
        byte-identical across clean and incremental builds.
  - [ ] 9c.7 Build flavors (e.g. minimal vs full, CPT 8000 vs Phoenix). Design the manifest
        for them from the start:
    - A shared base segment list plus `[flavor.<name>]` sections that `remove` segments,
      `replace` a segment's module list (e.g. Phoenix's bitmapped display driver), and set
      `defines` injected as `NAME EQU value` constants. Selected with
      `cpt-build system.toml --flavor <name>`; each flavor writes its own image.
    - In-module differences: injected EQU constants work today; choosing code paths needs
      conditional assembly (IF/ELSE/ENDIF), which MOL80 may or may not have had ([VERIFY]
      against the manual; add to the assembler checklist).
    - Object cache keyed by source hash **and** the flavor's defines, so switching flavors
      never reuses an object built with different constants.
    - The image records which flavor (and machine profile) built it, for the emulator check
      in task 15.
    - Mirror however CPT actually produced its model variants (per-model source files,
      conditional assembly, or per-product build lists) once the source shows it ([VERIFY]).
    - Tests: two flavors from one manifest give different images; a flavor's removed segment
      is absent; a define changes only the modules that use it.
  - _Requirements: 7.1-7.6_

- [x] 8b. Bidirectional serial port (USART) — `SerialDevice` with DATA + STATUS ports, RX/TX
      queues, optional RX interrupt; a `Transport` trait with `LoopbackTransport` (wasm-safe) and
      a native `TcpTransport` (listen/connect, threaded, non-blocking) wired into `cpt-run` via
      `--serial-listen`/`--serial-connect`. Verified: loopback round-trip, CPU-receives-via-IRQ,
      and a real TCP socket round-trip. Ports/status-bits/vector are config [VERIFY].
  - Note: the **UI never sees the serial port**. It is a CPU-facing byte primitive; any serial
    data that appears on screen or on disk is CPT software interpreting those bytes (terminal
    emulation / comm segments). So there is no "browser serial" feature. A WebSocket transport
    (to let a browser-hosted emulator reach an external peer) is a *maybe-if-needed*, not a
    planned gap — native `cpt-run` with TCP covers the real use.
  - _Requirements: (serial port)_

- [x] 8c. Timer interrupt device — done (100 Hz, non-programmable). The CPT monitor was **multitasking**, driven by a
      periodic timer interrupt (recalled ~100 Hz) used to pre-empt/switch tasks. Unlike the
      display retrace (cosmetic scrolling), this tick is load-bearing: the monitor's scheduler
      depends on it.
  - [x] 8c.1 `TimerDevice` (`cpt-machine/src/timer.rs`): a periodic IRQ on a configured RST
        vector via the existing `tick`/`take_irq` mechanism. **100 Hz, not programmable**
        (user-specified): no ports, no registers, no enable bit. `MachineConfig` gains
        `timer_period_cycles` (20,000 = 100 Hz at the placeholder 2 MHz clock) and
        `timer_vector` (RST 4 placeholder). Available in `cpt-run` with `--timer`; deliberately
        not attached in the wasm demo (the demo enables interrupts with no RST-4 handler).
        Frequency/vector remain [VERIFY].
  - [x] 8c.1b Executive fix found by 8c.3: interrupts are now level-like. The executive polls
        devices only while the CPU has interrupts enabled, so a request that arrives while
        masked (inside a handler, or after DI) stays pending instead of being consumed and
        dropped. Without this, retrace (33,333 cycles) and timer (20,000) coincide every ~100,000
        cycles and the timer lost 20% of its ticks. Same-source ticks coalesce into one request.
  - [ ] 8c.2 The task-switch timer is a **separate** interrupt from the display vertical
        retrace (task 6.3) — **[CONFIRMED]**. Two independent periodic sources on distinct RST
        vectors; both coexist. Only the timer's frequency and vector remain [VERIFY].
  - [x] 8c.3 Tests (`cpt-machine/tests/timer.rs`, 8): fires exactly each period and not
        before; 100 per second; pile-ups coalesce; no ports; a CPU handler counts ~100 ticks in
        one second; timer (100 Hz) and retrace (60 Hz) fire independently on their own
        vectors; a masked tick stays pending until EI; simultaneous sources are both serviced.
  - _Requirements: 1.4, 1.5 (to be extended for the multitasking timer)_

- [ ] 8d. Speaker / tone output device — planned. An output port drove a speaker for beeps and
      tones, and could be programmed to play simple melodies.
  - [ ] 8d.1 `SpeakerDevice` on an output port ([VERIFY] port). Capture the write stream with
        timestamps (CPU cycle counts) so the waveform can be reconstructed regardless of
        generation method.
  - [ ] 8d.2 Determine the generation method from the source ([VERIFY]): CPU **bit-banging** a
        1-bit output in a timed loop (melodies imply toggling the bit under software timing), vs.
        a programmable timer/divisor chip given a frequency. Model whichever it was; if it is a
        divider, expose the divisor register.
  - [ ] 8d.3 Web UI: turn the captured bit-stream / tone events into audio via the Web Audio
        API. Headless/`cpt-run`: no audio, just record events (for tests and trace).
  - [ ] 8d.4 Tests: writes are captured with correct timing; a square-wave toggle at a known
        rate produces the expected event stream.
  - _Requirements: (speaker — to be added to requirements)_

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

- [ ] 15. Machine profiles and the Phoenix bitmapped display — planned. A Phoenix program disk
      (task 9c.7) needs Phoenix hardware to run on; minimal/full flavors run on the same
      hardware and need nothing here. **BLOCKED** on Phoenix hardware details ([VERIFY]).
  - [ ] 15.1 Turn `MachineConfig` into named profiles (`cpt8000`, `phoenix`, ...) covering the
        device mix, memory map, and ports; select with `cpt-run --machine <profile>` and the
        equivalent wasm/web option. Default stays `cpt8000`.
  - [ ] 15.2 Make the display device pluggable behind a common interface: the existing
        character-cell `DisplayDevice` and a new bitmapped framebuffer device for Phoenix
        (framebuffer address/banking, resolution, pixel format, retrace interrupt [VERIFY]).
  - [ ] 15.3 Web UI renders either a character page snapshot or a pixel framebuffer,
        depending on the profile.
  - [ ] 15.4 Check that the loaded image's recorded flavor/profile matches the running machine
        profile; refuse with a clear message instead of running a mismatched image.
  - [ ] 15.5 Tests: profile selection wires the right devices; framebuffer writes appear in the
        snapshot; mismatched image is rejected.
  - _Requirements: 3.1-3.5, 8.1-8.4 (to be extended for Phoenix)_

- [ ] 14. Resolve [VERIFY] hardware details against CPT references
  - Fill `MachineConfig` (bank-select port + encoding + block count, device ports), bank0 ROM/RAM split,
    bank-switch timing, floppy geometry + controller interface, display memory model + char ROM,
    key matrix, IRQ sources, IPL sequence, segment-table format. Replace placeholders; extend tests.
  - _Requirements: 2.3, 2.6, 3.3, 3.4, 5.1, 6.2, 6.3, 7.1, 9.6_
