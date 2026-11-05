//! A built-in 8080 demo program that exercises the CPT display: it writes a title, fills the
//! top-half text region, draws a ruler on the typing line, writes a status line, and installs a
//! vertical-retrace handler that scrolls the top-half text up by one row on each retrace.
//!
//! The program is assembled here programmatically (a few tiny opcode helpers) rather than as a
//! hex blob, so the intent is readable. It is loaded at 0x0100; the retrace handler sits at the
//! RST-1 vector (0x0008), matching `MachineConfig::retrace_vector`.
//!
//! Addresses assume the default config: display base 0x4000, 80 columns, typing row 45,
//! status row 54, retrace = RST 1.

const DISPLAY_BASE: u16 = 0x4000;
const COLS: u16 = 80;
const TYPING_ROW: u16 = 45;
const STATUS_ROW: u16 = 54;
const TOP_FIRST: u16 = 1; // first top-half text row
const TOP_LAST: u16 = 44; // last top-half text row
const BOTTOM_FIRST: u16 = 46; // first bottom-half row (where disk blocks are shown)

// Disk controller ports (must match MachineConfig::default().disk_base_port = 0x50).
const DISK_BASE: u8 = 0x50;
const DISK_STATION: u8 = DISK_BASE; // +0 select station (0=A)
const DISK_BLOCK_LO: u8 = DISK_BASE + 1;
const DISK_BLOCK_HI: u8 = DISK_BASE + 2;
const DISK_COMMAND: u8 = DISK_BASE + 3; // out=command (1=read), in=status
const DISK_DATA: u8 = DISK_BASE + 4;

// A scratch byte in bank-0 RAM holding the next block number to read.
const CUR_BLOCK: u16 = 0x0200;

fn row_addr(row: u16) -> u16 {
    DISPLAY_BASE + row * COLS
}

/// A minimal 8080 assembler-in-code: push bytes with readable helpers.
#[derive(Default)]
struct Asm {
    code: Vec<u8>,
}
impl Asm {
    fn byte(&mut self, b: u8) {
        self.code.push(b);
    }
    fn bytes(&mut self, bs: &[u8]) {
        self.code.extend_from_slice(bs);
    }
    fn lxi_h(&mut self, addr: u16) {
        self.byte(0x21);
        self.bytes(&addr.to_le_bytes());
    }
    fn lxi_d(&mut self, addr: u16) {
        self.byte(0x11);
        self.bytes(&addr.to_le_bytes());
    }
    fn lxi_b(&mut self, v: u16) {
        self.byte(0x01);
        self.bytes(&v.to_le_bytes());
    }
    fn mvi_a(&mut self, v: u8) {
        self.bytes(&[0x3E, v]);
    }
    fn mvi_m(&mut self, v: u8) {
        self.bytes(&[0x36, v]);
    }
    fn mov_m_a(&mut self) {
        self.byte(0x77);
    }
    fn inx_h(&mut self) {
        self.byte(0x23);
    }
    fn inx_d(&mut self) {
        self.byte(0x13);
    }
    fn dcx_b(&mut self) {
        self.byte(0x0B);
    }
    fn mov_a_b(&mut self) {
        self.byte(0x78);
    }
    fn ora_c(&mut self) {
        self.byte(0xB1);
    }
    fn ldax_d(&mut self) {
        self.byte(0x1A);
    }
    fn jmp(&mut self, addr: u16) {
        self.byte(0xC3);
        self.bytes(&addr.to_le_bytes());
    }
    fn jnz(&mut self, addr: u16) {
        self.byte(0xC2);
        self.bytes(&addr.to_le_bytes());
    }
    fn ei(&mut self) {
        self.byte(0xFB);
    }
    fn ret(&mut self) {
        self.byte(0xC9);
    }
    // I/O and memory accessors used by the disk-read routine.
    fn out(&mut self, port: u8) {
        self.bytes(&[0xD3, port]);
    }
    fn in_a(&mut self, port: u8) {
        self.bytes(&[0xDB, port]);
    }
    fn sta(&mut self, addr: u16) {
        self.byte(0x32);
        self.bytes(&addr.to_le_bytes());
    }
    fn lda(&mut self, addr: u16) {
        self.byte(0x3A);
        self.bytes(&addr.to_le_bytes());
    }
    fn ani(&mut self, v: u8) {
        self.bytes(&[0xE6, v]);
    }
    fn inr_a(&mut self) {
        self.byte(0x3C);
    }
    fn len_at(&self, base: u16) -> u16 {
        base + self.code.len() as u16
    }
}

/// Write a string to display RAM starting at `addr`. Clobbers A, H, L.
fn write_string(a: &mut Asm, addr: u16, s: &str) {
    a.lxi_h(addr);
    for &byte in s.as_bytes() {
        a.mvi_m(byte); // MVI M, ch
        a.inx_h(); // advance to next cell
    }
}

/// Build the demo program, returning its bytes. Loaded at 0x0100.
pub fn display_demo() -> Vec<u8> {
    const BASE: u16 = 0x0100;

    // We assemble in two regions: the RST-1 retrace handler at 0x0008, and the main program at
    // 0x0100. The loader writes both into bank 0, so we return a single image spanning
    // 0x0000..end and the runner loads it at 0x0000. To keep the runner's `--demo` load simple
    // (it loads at 0x0100), we instead install the handler via code at startup: the main
    // program copies a small handler into 0x0008 itself. That avoids needing two load regions.

    let mut a = Asm::default();

    // --- main program (at 0x0100) ---

    // 1. Install the retrace handler bytes into 0x0008 (RST 1 vector).
    //    Handler scrolls the top-half up by one line, then EI; RET.
    //    We build the handler bytes separately and copy them with a byte-copy loop.
    let handler = build_handler();
    // Copy `handler` from an embedded table to 0x0008.
    // Emit: for each handler byte, MVI A,byte ; STA (0x0008+i).  Simple and position-independent.
    for (i, &hb) in handler.iter().enumerate() {
        a.mvi_a(hb);
        // STA addr
        a.byte(0x32);
        a.bytes(&(0x0008u16 + i as u16).to_le_bytes());
    }

    // 2. Title line.
    write_string(&mut a, row_addr(0), "CPT DISPLAY DEMO - retrace scroll test");

    // 3. Fill the top-half text region with numbered lines "LINE nn ...".
    //    For simplicity, write a short marker at the start of each top row.
    {
        let mut row = TOP_FIRST;
        let mut n = 0u8;
        while row <= TOP_LAST {
            // marker: 'A'+ (n % 26) repeated a few times, so scrolling is visible.
            let ch = b'A' + (n % 26);
            write_string_byte_run(&mut a, row_addr(row), ch, 8);
            row += 1;
            n += 1;
        }
    }

    // 4. Typing-line ruler: dashes across the row with a caret at the left margin.
    write_string_byte_run(&mut a, row_addr(TYPING_ROW), b'-', COLS as usize);
    // left-margin marker
    a.lxi_h(row_addr(TYPING_ROW));
    a.mvi_m(b'>');

    // 5. Status line.
    write_string(&mut a, row_addr(STATUS_ROW), "STATUS: running - mount a disk in drive A");

    // 6. Initialize the current-block counter to 0.
    a.mvi_a(0);
    a.sta(CUR_BLOCK);

    // 7. Enable interrupts, then enter the main loop: on each pass, try to read the current
    //    block from drive A and display it, then advance and pace with a delay. The retrace
    //    handler keeps scrolling the top half independently.
    a.ei();
    let main_loop = a.len_at(BASE);

    // --- select station A, set block number from CUR_BLOCK, issue READ ---
    a.mvi_a(0);
    a.out(DISK_STATION); // station A
    a.lda(CUR_BLOCK);
    a.out(DISK_BLOCK_LO); // block low = CUR_BLOCK (0..255 is plenty for the demo)
    a.mvi_a(0);
    a.out(DISK_BLOCK_HI); // block high = 0
    a.mvi_a(1);
    a.out(DISK_COMMAND); // CMD_READ

    // --- check status: if no-media (bit7) set, skip drawing this pass ---
    a.in_a(DISK_COMMAND); // A = status
    a.ani(0x80); // test no-media bit
    // if non-zero (no media), jump past the draw to the delay.
    let skip_target_placeholder = a.len_at(BASE); // we patch this by emitting jnz to a known addr
    let _ = skip_target_placeholder;
    // We need the address *after* the draw block. Emit the draw into a temp to learn its size,
    // then emit the real jnz. Simpler: compute the draw block length by building it separately.
    let draw = build_draw_block(BASE + (a.code.len() as u16) + 3); // +3 for the JNZ we emit next
    a.jnz(draw.skip_addr); // if no media -> skip the draw
    a.bytes(&draw.code);

    // --- advance CUR_BLOCK (wrap via byte overflow is fine for the demo) ---
    a.lda(CUR_BLOCK);
    a.inr_a();
    a.sta(CUR_BLOCK);

    // --- delay loop so blocks appear at a visible pace (BC counts down from ~0) ---
    // LXI B,0x0000 ; delay: DCX B ; MOV A,B ; ORA C ; JNZ delay
    a.lxi_b(0x0000);
    let delay = a.len_at(BASE);
    a.dcx_b();
    a.mov_a_b();
    a.ora_c();
    a.jnz(delay);

    a.jmp(main_loop);

    a.code
}

/// The "draw one block" code block, plus the address immediately after it (the skip target the
/// no-media branch jumps to). `start` is the absolute address where this block will be placed.
struct DrawBlock {
    code: Vec<u8>,
    skip_addr: u16,
}

/// Build code that streams 128 bytes from the DISK_DATA port into the bottom-half region
/// (starting at BOTTOM_FIRST). Clobbers A, B, H, L.
fn build_draw_block(start: u16) -> DrawBlock {
    let mut a = Asm::default();
    // HL = destination (bottom-half first row); B = 128 byte counter.
    a.lxi_h(row_addr(BOTTOM_FIRST));
    a.mvi_a(128);
    // move the counter into B via MOV B,A (0x47)
    a.byte(0x47); // MOV B,A
    // loop: IN DISK_DATA ; MOV M,A ; INX H ; DCR B ; JNZ loop
    let loop_addr = start + a.code.len() as u16;
    a.in_a(DISK_DATA);
    a.mov_m_a();
    a.inx_h();
    a.byte(0x05); // DCR B
    a.jnz(loop_addr);
    let skip_addr = start + a.code.len() as u16;
    DrawBlock { code: a.code, skip_addr }
}

/// Emit code that writes `ch` into `count` consecutive cells starting at `addr`
/// (unrolled, so no loop-address patching is needed). Clobbers A, H, L.
fn write_string_byte_run(a: &mut Asm, addr: u16, ch: u8, count: usize) {
    a.lxi_h(addr);
    for _ in 0..count {
        a.mvi_m(ch);
        a.inx_h();
    }
}

/// Build the retrace handler: scroll the top-half text region up by one row, then EI + RET.
///
/// Scrolling: for each top row r from TOP_FIRST..TOP_LAST, copy row r+1 into row r (80 bytes),
/// then clear the last top row. Implemented as an unrolled per-row copy using LDAX/STA-style
/// moves would be large; instead we copy the whole block (rows TOP_FIRST+1..=TOP_LAST) up by
/// one row with a single forward byte-copy loop, then blank the last row.
fn build_handler() -> Vec<u8> {
    let mut a = Asm::default();

    // Copy (TOP_LAST - TOP_FIRST) * COLS bytes from src=row_addr(TOP_FIRST+1) to
    // dst=row_addr(TOP_FIRST), forward. HL=dst, DE=src, BC=count.
    let count = (TOP_LAST - TOP_FIRST) * COLS;
    a.lxi_h(row_addr(TOP_FIRST)); // dst
    a.lxi_d(row_addr(TOP_FIRST + 1)); // src
    a.lxi_b(count);

    // The copy loop runs inside the handler, which lives at 0x0008. We need the loop's absolute
    // address to branch back. The handler is installed at 0x0008, and these first three LXI
    // instructions are 3 bytes each = 9 bytes, so the loop body starts at 0x0008 + 9 = 0x0011.
    let loop_addr = 0x0008u16 + 9;
    // loop: LDAX D ; MOV M,A ; INX H ; INX D ; DCX B ; MOV A,B ; ORA C ; JNZ loop
    a.ldax_d();
    a.mov_m_a();
    a.inx_h();
    a.inx_d();
    a.dcx_b();
    a.mov_a_b();
    a.ora_c();
    a.jnz(loop_addr);

    // Blank the last top row (row_addr(TOP_LAST), COLS spaces).
    // HL = row_addr(TOP_LAST); write ' ' COLS times, unrolled.
    a.lxi_h(row_addr(TOP_LAST));
    for _ in 0..COLS {
        a.mvi_m(b' ');
        a.inx_h();
    }

    a.ei();
    a.ret();
    a.code
}


