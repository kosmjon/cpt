# CPT 8000 Emulator — Requirements

## Overview

A software emulation of the CPT Corporation dedicated word processing workstation
(CPT 8000 and related models). It embeds an Intel 8080 CPU emulator that executes CPT
software segments (assembled from MOL80 via the separate `mol80asm` spec) and models the
distinctive CPT hardware: a page-oriented display, a Selectric-style fixed typing line,
keyboard, and a simple file-backed data disk.

This is the **second** of two coordinated specs; the first is the MOL80 assembler.

> **Reconstruction approach (no original media/ROM dumps available).** We do NOT have dumps
> of real CPT program/data disks or ROMs. Instead we **have the complete MOL80 source for the
> CPT word-processing system** — i.e. everything that would have been on a **Program Disk**:
> the application segments (editor, formatting, the B-tree filer, etc.) AND the low-level
> runtime segments (long-call handler, multitasking monitor, work-block heap allocator). It is
> one body of segmented source, not separate "firmware" + "apps". The plan:
> 1. Assemble every segment's source with `mol80asm`.
> 2. **Link-edit** the segments into a single **system image**: place `bank0`-tagged segments
>    in the resident bank 0, bin-pack the relocatable segments into the switchable 32K blocks,
>    resolve references, and **generate the bank-0 tables** (segment→bank/offset map, fixed-
>    address table) **in the exact format the long-call handler source expects to read**.
> 3. Load the image into simulated bank 0 + all 32K blocks.
> 4. Start the CPU at the main entry point.
> Because we have the full system source, the emulator **runs the real, complete CPT word
> processor** — not a reimplementation of any part; it only builds the image and provides the
> hardware primitives.

> Status legend: **[CONFIRMED]** (verified with user), **[ASSUMED]** (reasonable default),
> **[VERIFY]** (open question for the CPT software references / source).

## Known facts (from the user)

- Target CPU: **Intel 8080**. **[CONFIRMED]**
- **Banked memory:** the 8080's 64K address space is modeled as **two 32K banks** —
  **bank 0 = 0000–7FFF**, **bank 1 = 8000–FFFF**. **[CONFIRMED]**
- **Bank 0 is resident/fixed and is essentially all RAM.** It holds the **interrupt
  handlers**, the **multitasking monitor**, and the **long-call handler**. **[CONFIRMED]**
- Many machines had as much as **256K of RAM**. With a 32K bank window, 256K ÷ 32K = **8
  physical 32K blocks**. The exact split (whether bank 0 is one dedicated block plus 7
  swappable, or bank 0 draws from the same 8-block pool) is **[VERIFY]**, but the block count
  for a 256K machine is 8. **[CONFIRMED: 256K common]**
- We **have the complete MOL80 source for the CPT word-processing system** (everything that
  was on a Program Disk): application segments (editor, formatting, B-tree filer, …) plus the
  low-level runtime segments (long-call handler, multitasking monitor, **work-block heap
  allocator**). The runtime segments are **`bank0`-tagged**; the applications are mostly
  relocatable. **[CONFIRMED]**
- Instead of a disk-based boot loader, a **host-side link/loader** builds the system image and
  loads it into simulated memory (see Overview). Original boot-from-Program-Disk is out of
  scope. **[CONFIRMED]**
- **Segment classes:** some segments are **tagged to reside in bank 0** (the runtime segments);
  most others are **relocatable** and bin-packed into the switchable 32K blocks. **[CONFIRMED]**
- There is a **heap of 256-byte-aligned "work blocks"** that code can allocate and free; the
  allocator source is available (likely bank 0). These page-aligned work blocks are the same
  blocks the MOL80 `(HL+offset)<reg>` "using"/array idiom indexes into (see the assembler
  spec). **[CONFIRMED]**
- **Bank 1 is switchable:** the hardware provides several physical 32K blocks, and software
  selects which block is mapped into the 8000–FFFF window by **writing a bank number to a
  specific I/O port**. **[CONFIRMED]** (which port, how many blocks, and the number encoding
  are **[VERIFY]**)
- **Long-call mechanism (software overlay/segmentation on top of banking):** code running in
  bank 1 calls the **long-call routine in bank 0** with a target **segment number** and
  **subroutine name/identifier**. The handler saves all registers and the current bank-1
  block, consults a **segment → (bank, offset) table** to find the physical block and
  absolute address of the target, swaps that block into bank 1, and jumps to the subroutine.
  On return, control re-enters the long-call handler, which restores the original bank-1
  block and registers and returns to the original caller. **[CONFIRMED]**
- Therefore a **segment** is a unit of code residing in some physical 32K block, reached via
  the long-call indirection — both the CPT-library organizing concept and a runtime entity
  resolved through the segment table (e.g. Segment 12 = B-tree disk filing system).
  **[CONFIRMED]**
- **Important scope note:** the long-call handler, monitor, and interrupt handlers are **CPT
  system software (compiled from the source we have) that runs on the emulated CPU** — the
  emulator does **not** reimplement them. The emulator must provide the **hardware primitives**
  they depend on: the bank-select I/O port and the correct physical-block memory model. Their
  behavior then emerges from
  running the original code. **[CONFIRMED]**
- **Storage: dual 8-inch floppy drives.** The system boots from a **Program Disk** and users
  save/load documents from a separate **Data Disk**. **[CONFIRMED]** (floppy format, density,
  sector layout, and controller interface are **[VERIFY]**)
- Display: **80 columns × 54 lines**, showing **one page at a time** (page-oriented, not a
  scrolling terminal). **[CONFIRMED]**
- Display was **page-white** (dark text on a white background) — unusual for the era and a
  defining visual trait. **[CONFIRMED]**
- There is a **special display memory** that the current page's text is loaded into for the
  video hardware to scan. Separately, the system software is recalled to represent page text
  as a **linked-list-type structure of lines** in RAM. The likely relationship: the linked
  list is the editable document model, and the software **projects the visible window of it
  into the scanned display memory** on scroll/edit/margin changes. **[HYPOTHESIS — confirm]**
  - Implication for scope: the linked list is **system-software** behavior; the emulator models only
    the **display memory the video hardware scans** (the hardware boundary) and renders that.
    The linked-list format, if documented, is valuable to the **debugger** (to show the logical
    document), not to core emulation. **[HYPOTHESIS]**
  - Open: whether the scanned display memory is a **separate region (dedicated display RAM on
    the display board)** or a **window into main/banked RAM**. **[VERIFY]**
- The display board carries a **character-bitmap ROM** that supplies the glyph pixel patterns.
  Character shapes are therefore a **hardware asset read by the display hardware**, not stored
  in CPU-addressable memory. The system software writes **character codes** (and attributes) into
  display RAM; the display board turns each code into pixels via its font ROM. **[CONFIRMED]**
  (the actual ROM contents / font are **[VERIFY]** pending a dump)
- The screen has a **dark status/header band above the page-white area** showing
  mode/document/page/line indicators (observed on a CPT 9000: fields like `IN`, a document
  id `1 RF2025`, and page/line `1 22`). The page-white document area is a distinct region
  below it. **[CONFIRMED from CPT 9000 photo]** (exact fields/positions **[VERIFY]**)
- **No floating caret.** There is a **fixed typing line**; as the operator types, the
  document moves relative to the fixed insertion row, emulating an IBM Selectric typewriter
  to ease migration for typists. **[CONFIRMED]**
- The **typing line is a visible ruler drawn by the software** at a fixed screen row,
  separating the text that has scrolled above it from the empty area below it. The ruler
  carries **adjustable start/end margin markers** (so a section can be narrowed, e.g. for
  justified text) and **tab/tick marks**. These are "special markers" the software draws —
  i.e. glyphs/graphics in display memory, not emulator chrome. **[CONFIRMED]**
- **Editing model (CPT system-software behavior):** a page loaded from disk fills the area **below**
  the typing line; keyboard scroll keys move the document so a chosen line lands **on** the
  typing line where it can be edited; pressing **Return** moves the current line up (revealing
  a fresh empty line below) and returns the cursor to the **left margin on the typing line**,
  like a typewriter carriage return + line feed. **[CONFIRMED]**
- **Scope note:** the editor (document buffer, viewport, ruler/margins, scrolling, Return
  behavior) is CPT **system software** (compiled from our source) running on the emulated CPU.
  The emulator renders whatever that software writes to display memory (including the ruler and
  margin markers) and feeds it keystrokes; it does **not** implement the editor itself.
  **[CONFIRMED]**
- **Keyboard layout (from the CPT 8000 keyboard photo, labels [CONFIRMED]; the key
  codes/scan matrix each key sends are [VERIFY]):** four clusters, left to right —
  1. **Left function column:** `CODE`, `CLEAR/TAB ADJ`, `HOLD`, `MAR SET` (margin set),
     `MAR REL` (margin release). The margin keys set/release the ruler's start/end margin
     markers; `TAB ADJ` relates to the ruler tab stops.
  2. **Main typewriter block:** QWERTY with number/symbol row, `TAB`, `LOCK`, dual `SHIFT`,
     spacebar, large `RETURN`, `BACK SPACE`, and a set of special symbol/format keys at the
     block's top-left (exact glyphs [VERIFY]).
  3. **Center editing/navigation cluster:** labels read approximately as
     `CODE STOP CHAR IN` / `INSERT SKIP WORD OUT` / `PROG ADJ LINE PRINT` /
     `MOVE UP PARA` / `ERASE DOWN PAGE`. These group into:
     - **unit selectors:** `CHAR`, `WORD`, `LINE`, `PARA`, `PAGE` (granularity for operations);
     - **navigation:** `UP`, `DOWN` (and `PAGE`/`LINE`) — the scroll keys that move the
       document onto the fixed typing line;
     - **edit ops:** `INSERT`, `ERASE`, `MOVE`, `SKIP`, `ADJ`;
     - **document/print/program:** `IN`, `OUT`, `PRINT`, `STOP`, `PROG`.
     The `IN` key plausibly corresponds to the `IN` indicator seen in the CPT 9000 status band.
     **[CONFIRMED labels; groupings partly inferred — VERIFY]**
  4. **Numeric keypad:** `0`–`9`, `-`, `.`/`,`, and a tall `ENTER`.
- UI should be built with **React** if practical. **[CONFIRMED]**

## Open items to resolve [VERIFY]

1. Exact 8080 vs. 8085 details and clock speed (cycle timing fidelity target).
2. **Bank-select I/O port:** which port address and the bank-number encoding (e.g. a 3-bit
   field selecting one of 8 blocks for a 256K machine). The **block count follows from RAM
   size** (256K → 8 × 32K); the remaining detail is the bank-0-vs-swappable-pool split.
3. Bank-switch timing semantics: whether a write to the bank port takes effect on the next
   instruction fetch, and any hardware guard against swapping out currently executing code
   (presumably safe because swaps are driven from resident bank 0).
4. The **segment → (bank, offset) table** format and the long-call calling convention — now
   read directly from the **long-call handler source** (the linker must generate the table in
   exactly that format). Also useful to the debugger for resolving segment numbers.
5. Which segments are **`bank0`-tagged** vs. relocatable; the tagging mechanism in source;
   total bank-0 footprint (must fit one 32K block); the **work-block heap** location/size from
   the allocator source.
6. Display hardware: the **character-bitmap ROM contents** on the display board (a font-ROM
   dump would give exact glyphs), character cell size, refresh model, and any attributes
   (bold/underline/inverse).
7. The **character-code encoding** the system software writes into display RAM (i.e. which
   code maps to which font-ROM glyph).
8. **Display memory model:** confirm the "special display memory" — is it a dedicated region
   on the display board or a window into main/banked RAM? Confirm the system software's
   **linked-list-of-lines** page representation and its node format (confirm/deny the
   hypothesis; valuable for the debugger's logical-document view).
9. **Display geometry details:** whether the 80×54 count includes or excludes the status band
   and the ruler row; the exact **status-band fields and layout** (the observed `IN`,
   document id, page/line indicators); and the **ruler/margin/tab marker encoding** in display
   memory.
10. Keyboard **key codes / scan matrix**: the per-key code each labeled key sends (the layout
    and labels are known from the keyboard photo — four clusters: left function column with
    margin set/release, typewriter block, center editing/navigation cluster, numeric keypad —
    but the actual codes, including the `CODE`-shifted meanings, come from the source).
11. **Data disk interface** (from the B-tree filer source): the I/O ports/commands the filer
    uses, and its sector/block model — which determines the backing host-file layout. (Full
    8-inch floppy hardware emulation is out of scope; only a simple file-backed data disk.)
12. **System-image build**: the relocatable object format `mol80asm` emits, the entry-point
    convention, and the fixed-address table contents the handler expects — i.e. everything the
    link/image step needs, derived from the system source.
13. Which physical row is the typing line, and how scrolling-by-document is realized in the
    system software.
14. Printer interface (Selectric-based or otherwise), if in scope.

---

## Requirements

### Requirement 1: Emulate the Intel 8080 CPU

**User Story:** As a preservationist, I want a faithful 8080 CPU emulator, so that original
CPT segments execute correctly.

#### Acceptance Criteria

1. WHEN a program is loaded and run THEN the emulator SHALL execute all documented 8080
   instructions with correct register, flag (PSW), and memory effects.
2. WHEN an instruction affects the flags THEN the carry, zero, sign, parity, and auxiliary
   carry flags SHALL be set per the 8080 specification.
3. WHEN the stack is used (PUSH/POP/CALL/RET) THEN SP behavior SHALL match the 8080.
4. WHEN an interrupt occurs AND interrupts are enabled THEN the emulator SHALL service it per
   the 8080 interrupt model. *(Interrupt sources from CPT hardware [VERIFY].)*
5. WHEN the emulator steps instructions THEN it SHALL track cycle counts for timing. *(Exact
   cycle-accuracy target [VERIFY].)*

### Requirement 2: Model banked memory and I/O

**User Story:** As a user running CPT software, I want the machine's two-bank memory, the
bank-select port, and the I/O ports modeled, so that the monitor, long-call handler, and
segments behave as on real hardware.

#### Acceptance Criteria

1. WHEN the machine initializes THEN it SHALL present a 64K address space as **bank 0
   (0000–7FFF, resident)** and **bank 1 (8000–FFFF, switchable)**.
2. WHEN bank 0 is accessed THEN it SHALL always map to the resident 32K holding the interrupt
   handlers, monitor, and long-call handler, regardless of the selected bank-1 block.
3. WHEN code **writes the bank-select I/O port** THEN the emulator SHALL map the chosen
   physical 32K block into the 8000–FFFF window for subsequent accesses. The number of blocks
   SHALL be configurable to match RAM size (e.g. **8 blocks for a 256K machine**). *(Port
   address and number encoding [VERIFY]; bank-0-vs-pool split [VERIFY].)*
4. WHEN bank 1 is read or written THEN the access SHALL target the **currently selected
   physical block**, and switching blocks SHALL NOT disturb the contents of de-selected
   blocks (they persist for later re-selection, as the long-call mechanism requires).
5. WHEN code reads/writes a device I/O port THEN the emulator SHALL route it to the modeled
   device (display, keyboard, disk, bank-select). *(Port map [VERIFY].)*
6. WHEN bank 0 is accessed THEN it SHALL behave as **RAM** (bank 0 is essentially all RAM;
   it is populated by loading the system image, not resident ROM).
7. WHEN a system image is loaded THEN its bank-0 contents and physical bank-1 blocks SHALL be
   placed in the correct banks/blocks and be executable (see Req 7).

> Note: the long-call handler itself is CPT software executing on the emulated CPU. This
> requirement only provides the **hardware primitives** it relies on (the two-bank model,
> persistent de-selected blocks, and the bank-select port). Long-call behavior emerges from
> running the original bank-0 code.

### Requirement 3: Render the display (status band + page-white area + ruler)

**User Story:** As a user, I want the authentic CPT screen — a status band, the page-white
document area, and the software-drawn typing-line ruler — so that the experience matches the
original workstation.

#### Acceptance Criteria

1. WHEN the display renders THEN it SHALL present the **80 columns × 54 lines** page-white
   document area as a single page. *(Whether the 54 lines include or exclude the status band
   and ruler rows is [VERIFY].)*
2. WHEN characters are shown in the document area THEN rendering SHALL be **page-white**: dark
   characters on a white background.
3. WHEN the machine renders THEN it SHALL show the **dark status/header band** above the
   page-white area, reflecting the mode/document/page/line indicators the system software maintains.
   *(Exact fields and layout [VERIFY].)*
4. WHEN the emulated program writes to display memory/ports THEN the rendered display SHALL
   reflect those writes, **including the ruler and margin/tab markers** the software draws (the
   emulator renders them as ordinary display content, not as emulator chrome). *(Display
   memory model [VERIFY].)*
5. WHEN a character code in display RAM is drawn THEN the emulator SHALL resolve it to a glyph
   via a **character-bitmap font** that stands in for the display board's font ROM, so changing
   to a real font-ROM dump is a drop-in replacement. *(ROM contents / cell size [VERIFY].)*
6. WHEN the page updates THEN the display SHALL remain page-oriented (one page shown; no
   terminal-style continuous scroll).

### Requirement 4: Fixed typing-line editing model (Selectric behavior)

**User Story:** As a typist migrating from an IBM Selectric, I want a fixed typing-line ruler
with the document moving beneath it, so that the machine feels like a typewriter.

> Scope: the editing behavior below is implemented by **CPT system software** running on the
> emulated CPU. The emulator's obligation is to **render what that software draws** (ruler,
> margins, text positions) and **deliver keystrokes**, so this behavior is reproduced
> faithfully rather than reimplemented. These criteria describe the behavior the emulator must
> not obstruct and must render correctly.

#### Acceptance Criteria

1. WHEN the system software draws the typing line THEN the emulator SHALL render it as a **ruler at a
   fixed screen row**, including its **adjustable start/end margin markers** and tab/tick
   marks, from display memory. *(Row and marker encoding [VERIFY].)*
2. WHEN a page is loaded from disk THEN its content SHALL appear in the area **below** the
   typing-line ruler, as the system software populates display memory.
3. WHEN scroll keys are pressed THEN keystrokes SHALL be delivered so the system software can move a
   chosen document line **onto** the typing line for editing; the emulator SHALL render the
   resulting display movement (document moves, ruler row stays fixed).
4. WHEN **Return** is pressed THEN the keystroke SHALL be delivered so the system software can move the
   current line up (revealing a new empty line below) and return the cursor to the **left
   margin on the typing line**; the emulator SHALL render the result.
5. WHEN document movement or margin changes occur THEN the rendered page SHALL stay consistent
   with the display memory the system software maintains.

### Requirement 5: Keyboard input

**User Story:** As a user, I want the full CPT keyboard — typewriter block, margin/tab keys,
the editing/navigation cluster, and the numeric keypad — so that I can operate CPT software
the way an original operator did.

#### Acceptance Criteria

1. WHEN a typewriter-block key is pressed in the UI THEN the emulator SHALL deliver the
   corresponding input to the emulated keyboard device. *(Scan codes / matrix [VERIFY].)*
2. WHEN a dedicated CPT function key is pressed (e.g. `MAR SET`/`MAR REL`, `CODE`, `HOLD`,
   `INSERT`, `ERASE`, `MOVE`, `SKIP`, `ADJ`, the unit selectors `CHAR`/`WORD`/`LINE`/`PARA`/
   `PAGE`, `IN`/`OUT`, `PRINT`/`STOP`/`PROG`) THEN its CPT-specific code SHALL be delivered.
   *(Per-key code mapping [VERIFY] — labels known from the keyboard photo, codes from source.)*
3. WHEN the **navigation keys** (`UP`, `DOWN`, and `PAGE`/`LINE`) are pressed THEN their codes
   SHALL be delivered so the system software can move the document relative to the fixed typing line
   (Req 4.3).
4. WHEN **Return** or **numeric-keypad** keys are pressed THEN their codes SHALL be delivered.
5. WHEN the emulated program polls or is interrupted for keyboard input THEN it SHALL receive
   queued keystrokes in order.

### Requirement 6: Simple file-backed data disk

**User Story:** As a user, I want a simple data disk so CPT software (including the Segment 12
B-tree filer) can save and load documents, without needing original 8-inch floppy media.

> Scope change: original dual 8-inch floppy emulation and program-disk booting are out of
> scope (no media dumps). The program side is delivered as a linked system image (Req 7); only
> a simple writable **data disk** remains, backed by a host file.

#### Acceptance Criteria

1. WHEN the machine is configured THEN it SHALL provide a single **file-backed data disk** for
   documents.
2. WHEN emulated code accesses the disk device THEN the emulator SHALL model it at the I/O
   ports the filer source expects. *(Port/command interface taken from the filer source
   [VERIFY].)*
3. WHEN the data disk is read or written THEN the operation SHALL target a backing host file,
   persisting writes.
4. WHEN the B-tree filer (Segment 12) runs THEN it SHALL perform its file operations against
   this data disk. *(The filer's sector/block model determines the backing-file layout
   [VERIFY from filer source].)*
5. WHEN no backing file exists THEN the emulator SHALL present an empty/initialized data disk.

### Requirement 7: Build and load the system image

**User Story:** As a developer, I want to compile and link the CPT segment source into a
loadable system image, so that the emulator can run the real CPT software I have the source
for, without original media.

#### Acceptance Criteria

1. WHEN the segment sources are assembled THEN each SHALL be produced as a relocatable object
   by `mol80asm`, carrying its symbols and references. *(Object format defined jointly with
   the assembler spec.)*
2. WHEN the link/image step runs THEN it SHALL place every **`bank0`-tagged** segment in the
   resident bank 0 and SHALL fail with a clear error if the bank-0 segments do not fit.
3. WHEN the link/image step runs THEN it SHALL **bin-pack the relocatable segments** into the
   available switchable 32K blocks, assigning each a (bank, offset), and SHALL report if they
   do not fit the configured number of blocks.
4. WHEN packing completes THEN the linker SHALL **generate the bank-0 tables** — the
   segment→(bank, offset) map and the fixed-address table — **in the exact binary format the
   long-call handler source expects to consume**. *(Format derived from that source [VERIFY].)*
5. WHEN all references are resolved THEN the linker SHALL emit a single **system image** file
   describing bank 0 and every 32K block's contents plus the entry point.
6. WHEN the emulator loads a system image THEN it SHALL populate bank 0 and the 32K blocks
   accordingly and begin execution at the recorded **main entry point**.
7. WHEN a reference cannot be resolved THEN the linker SHALL report it and SHALL NOT emit an
   image.

### Requirement 7a: Work-block heap

**User Story:** As CPT software, I need the 256-byte-aligned work-block heap available at
runtime, so that allocation/free and the array-indexing idiom work as designed.

#### Acceptance Criteria

1. WHEN the system image is built THEN the memory region used by the **work-block heap** SHALL
   be reserved and positioned as the heap-allocator source requires. *(Location/size from the
   allocator source [VERIFY].)*
2. WHEN the heap allocator (from CPT source) runs THEN alloc/free SHALL operate over
   **256-byte-aligned blocks**, consistent with the MOL80 `(HL+offset)<reg>` array idiom.
3. WHEN the emulator runs THEN it SHALL NOT reimplement the allocator; it SHALL execute the
   CPT allocator code and only guarantee the reserved region exists in simulated RAM.

### Requirement 8: React-based UI shell

**User Story:** As a user, I want a React UI hosting the emulated screen and controls, so
that the emulator runs in a modern, accessible environment (including the browser).

#### Acceptance Criteria

1. WHEN the UI loads THEN it SHALL render the 80×54 page-white display driven by the emulator
   core.
2. WHEN the user interacts (keyboard, controls) THEN input SHALL be forwarded to the core and
   the display SHALL update.
3. WHEN the emulator core runs in the browser THEN it SHALL communicate with the React UI via
   a defined boundary (the Rust core compiled to WebAssembly).
4. WHEN the UI provides machine controls (power/reset, attach disk, load segment) THEN they
   SHALL invoke the corresponding core operations.
5. WHEN the UI renders the keyboard THEN it SHALL present the CPT layout's four clusters (left
   function column, typewriter block, center editing/navigation cluster, numeric keypad) so
   the dedicated CPT keys are clickable, and SHALL map physical-keyboard keys to them where a
   sensible correspondence exists.
6. WHEN the display renders THEN it SHALL remain legible and the UI SHALL meet basic
   accessibility practices (keyboard operability, sufficient contrast).

### Requirement 9: Debug and inspection facilities

**User Story:** As a developer validating the emulator, I want to inspect machine state, so
that I can verify correct execution.

#### Acceptance Criteria

1. WHEN requested THEN the emulator SHALL expose CPU registers, flags, PC, and SP.
2. WHEN requested THEN it SHALL allow single-stepping and viewing memory regions.
3. WHEN a breakpoint address is set THEN execution SHALL pause when the PC reaches it.
4. WHEN running THEN it SHALL provide a disassembly view (reusing the 8080 mnemonic data
   shared with the assembler's ISA table where practical).
5. WHEN inspecting memory THEN the debugger SHALL indicate the **currently selected bank-1
   block** and allow viewing any physical block, so bank-switched code can be examined.
6. WHEN the segment → (bank, offset) table can be located in memory THEN the debugger SHOULD
   resolve addresses to segment numbers to aid tracing long-calls. *(Table format [VERIFY].)*
