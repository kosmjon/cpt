//! The authoritative MOL80 instruction table (data-driven, Req 8).
//!
//! Each [`IsaEntry`] maps a MOL80 operand *pattern* to its opcode byte, standard Intel 8080
//! mnemonic (for transpilation + cross-checking), and instruction length. Opcode values are
//! decimal in the MOL80 alphabetical scan and align with the standard 8080 opcode map, which
//! lets tests cross-check them.
//!
//! Rows carry a `verified` flag: `true` where the MOL80 spelling is confirmed from the scan,
//! `false` where the spelling is still tentative (the logical operators and anything awaiting
//! the official manual). The [VERIFY] backlog therefore lives in code, not just docs.

use crate::ast::AluOp;
use crate::token::Reg;

/// How a MOL80 instruction's operands are shaped. Codegen matches an AST item against these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    /// Register-to-register / memory move: `src : dst` (MOV family). No extra bytes.
    Move { src: Reg, dst: Reg },
    /// 8-bit immediate into a register/memory: `n : dst` (MVI family). +1 immediate byte.
    MviImm { dst: Reg },
    /// 16-bit immediate into a register pair: `nn : rp` (LXI family). +2 immediate bytes.
    LxiImm { rp: Reg },
    /// Increment: `r + 1 : r` (INR family). No extra bytes.
    Inr { reg: Reg },
    /// Decrement: `r - 1 : r` (DCR family). No extra bytes.
    Dcr { reg: Reg },
    /// ALU op targeting A with a register/memory operand: `A <op> r : A` (ADD/SUB/ANA/ORA/XRA).
    Alu { op: AluOp, rhs: Reg },
    /// 16-bit increment of a register pair: `rp + 1 : rp` (INX family).
    Inx { rp: Reg },
    /// 16-bit decrement of a register pair: `rp - 1 : rp` (DCX family).
    Dcx { rp: Reg },
    /// 16-bit add into HL: `HL + rp : HL` (DAD family). `HL*2:HL` is DAD H.
    Dad { rp: Reg },
}

/// One row of the instruction table.
#[derive(Debug, Clone, Copy)]
pub struct IsaEntry {
    pub opcode: u8,
    pub pattern: Pattern,
    /// Standard Intel 8080 mnemonic template (used for transpile output + cross-checks).
    pub i8080: &'static str,
    /// Total instruction length in bytes (1..=3).
    pub len: u8,
    /// True when the MOL80 spelling is confirmed from the scan; false for tentative rows.
    pub verified: bool,
}

/// The eight MOV/MVI/INR/DCR register slots in 8080 encoding order (B,C,D,E,H,L,M,A).
/// The index is the 3-bit register field used throughout the 8080 opcode map.
const REG_ORDER: [Reg; 8] = [
    Reg::B,
    Reg::C,
    Reg::D,
    Reg::E,
    Reg::H,
    Reg::L,
    Reg::M,
    Reg::A,
];

const REG_NAME: [&str; 8] = ["B", "C", "D", "E", "H", "L", "M", "A"];

/// Build the full instruction table.
///
/// The regular families (MOV, MVI, INR, DCR, ALU) are generated from the 8080 encoding rules;
/// irregular instructions (jumps, calls, returns, LXI, misc) are listed explicitly. Generation
/// keeps the table correct-by-construction for the large regular blocks while the explicit
/// rows capture the rest.
pub fn build_table() -> Vec<IsaEntry> {
    let mut t = Vec::new();

    // --- MOV dst,src : opcode 0x40 + (dst<<3) + src, excluding 0x76 (HALT) ---
    // MOL80 writes this as `src : dst`.
    for (di, &dst) in REG_ORDER.iter().enumerate() {
        for (si, &src) in REG_ORDER.iter().enumerate() {
            let opcode = 0x40 + ((di as u8) << 3) + (si as u8);
            if opcode == 0x76 {
                continue; // 0x76 is HALT, not MOV M,M
            }
            t.push(IsaEntry {
                opcode,
                pattern: Pattern::Move { src, dst },
                i8080: mov_name(di, si),
                len: 1,
                verified: true,
            });
        }
    }

    // --- MVI dst,n : opcode 0x06 + (dst<<3), +1 immediate ---
    for (di, &dst) in REG_ORDER.iter().enumerate() {
        let opcode = 0x06 + ((di as u8) << 3);
        t.push(IsaEntry {
            opcode,
            pattern: Pattern::MviImm { dst },
            i8080: mvi_name(di),
            len: 2,
            verified: true,
        });
    }

    // --- INR r : opcode 0x04 + (r<<3) ---
    for (ri, &reg) in REG_ORDER.iter().enumerate() {
        let opcode = 0x04 + ((ri as u8) << 3);
        t.push(IsaEntry {
            opcode,
            pattern: Pattern::Inr { reg },
            i8080: inr_name(ri),
            len: 1,
            verified: true,
        });
    }

    // --- DCR r : opcode 0x05 + (r<<3) ---
    for (ri, &reg) in REG_ORDER.iter().enumerate() {
        let opcode = 0x05 + ((ri as u8) << 3);
        t.push(IsaEntry {
            opcode,
            pattern: Pattern::Dcr { reg },
            i8080: dcr_name(ri),
            len: 1,
            verified: true,
        });
    }

    // --- ALU A,r families. Base opcodes per op; +r (0..7) for the register field. ---
    // ADD=0x80, SUB=0x90, ANA=0xA0, XRA=0xA8, ORA=0xB0.
    // The MOL80 logical spellings (+*, -*, **) are TENTATIVE, so those rows are unverified.
    for (ri, &rhs) in REG_ORDER.iter().enumerate() {
        push_alu(&mut t, AluOp::Add, 0x80, ri, rhs, "ADD", true);
        push_alu(&mut t, AluOp::Sub, 0x90, ri, rhs, "SUB", true);
        push_alu(&mut t, AluOp::And, 0xA0, ri, rhs, "ANA", false);
        push_alu(&mut t, AluOp::Xor, 0xA8, ri, rhs, "XRA", false);
        push_alu(&mut t, AluOp::Or, 0xB0, ri, rhs, "ORA", false);
    }

    // --- 16-bit INX / DCX / DAD families (register pairs B, D, H, SP). ---
    // INX rp: 0x03 + (rp<<4); DCX rp: 0x0B + (rp<<4); DAD rp: 0x09 + (rp<<4).
    for (code, rp) in [(0u8, Reg::BC), (1, Reg::DE), (2, Reg::HL), (3, Reg::SP)] {
        let hi = code << 4;
        t.push(IsaEntry { opcode: 0x03 + hi, pattern: Pattern::Inx { rp }, i8080: inx_name(rp), len: 1, verified: true });
        t.push(IsaEntry { opcode: 0x0B + hi, pattern: Pattern::Dcx { rp }, i8080: dcx_name(rp), len: 1, verified: true });
        t.push(IsaEntry { opcode: 0x09 + hi, pattern: Pattern::Dad { rp }, i8080: dad_name(rp), len: 1, verified: true });
    }

    // --- LXI rp,nn : 16-bit immediate into a register pair. ---
    // Only HL is confirmed from the scan (`33 nn:HL`); BC/DE/SP by the regular encoding.
    t.push(IsaEntry { opcode: 0x01, pattern: Pattern::LxiImm { rp: Reg::BC }, i8080: "LXI B,{nn}", len: 3, verified: false });
    t.push(IsaEntry { opcode: 0x11, pattern: Pattern::LxiImm { rp: Reg::DE }, i8080: "LXI D,{nn}", len: 3, verified: false });
    t.push(IsaEntry { opcode: 0x21, pattern: Pattern::LxiImm { rp: Reg::HL }, i8080: "LXI H,{nn}", len: 3, verified: true });
    t.push(IsaEntry { opcode: 0x31, pattern: Pattern::LxiImm { rp: Reg::SP }, i8080: "LXI SP,{nn}", len: 3, verified: false });

    t
}

fn push_alu(
    t: &mut Vec<IsaEntry>,
    op: AluOp,
    base: u8,
    ri: usize,
    rhs: Reg,
    mnem: &'static str,
    verified: bool,
) {
    t.push(IsaEntry {
        opcode: base + ri as u8,
        pattern: Pattern::Alu { op, rhs },
        i8080: alu_name(mnem, ri),
        len: 1,
        verified,
    });
}

// The `i8080` field is &'static str, so we return fixed names via lookup tables rather than
// formatting at runtime. These cover the regular register-indexed families.

fn mov_name(di: usize, si: usize) -> &'static str {
    MOV_NAMES[di][si]
}
fn mvi_name(di: usize) -> &'static str {
    MVI_NAMES[di]
}
fn inr_name(ri: usize) -> &'static str {
    INR_NAMES[ri]
}
fn dcr_name(ri: usize) -> &'static str {
    DCR_NAMES[ri]
}
fn alu_name(mnem: &str, ri: usize) -> &'static str {
    match mnem {
        "ADD" => ADD_NAMES[ri],
        "SUB" => SUB_NAMES[ri],
        "ANA" => ANA_NAMES[ri],
        "XRA" => XRA_NAMES[ri],
        "ORA" => ORA_NAMES[ri],
        _ => "???",
    }
}

// --- static name tables (generated once, keep &'static str out of the hot path) ---

macro_rules! reg_indexed {
    ($name:ident, $prefix:literal) => {
        const $name: [&'static str; 8] = [
            concat!($prefix, "B"),
            concat!($prefix, "C"),
            concat!($prefix, "D"),
            concat!($prefix, "E"),
            concat!($prefix, "H"),
            concat!($prefix, "L"),
            concat!($prefix, "M"),
            concat!($prefix, "A"),
        ];
    };
}

// MVI needs a ",{n}" suffix, so build it explicitly rather than via the reg_indexed! macro.
const MVI_NAMES: [&str; 8] = [
    "MVI B,{n}", "MVI C,{n}", "MVI D,{n}", "MVI E,{n}",
    "MVI H,{n}", "MVI L,{n}", "MVI M,{n}", "MVI A,{n}",
];
reg_indexed!(INR_NAMES, "INR ");
reg_indexed!(DCR_NAMES, "DCR ");
reg_indexed!(ADD_NAMES, "ADD ");
reg_indexed!(SUB_NAMES, "SUB ");
reg_indexed!(ANA_NAMES, "ANA ");
reg_indexed!(XRA_NAMES, "XRA ");
reg_indexed!(ORA_NAMES, "ORA ");

// MOV names are a 2-D table: MOV <dst>,<src>.
const MOV_NAMES: [[&'static str; 8]; 8] = build_mov_names();

const fn build_mov_names() -> [[&'static str; 8]; 8] {
    // const context can't loop over strings easily, so lay it out explicitly.
    [
        ["MOV B,B","MOV B,C","MOV B,D","MOV B,E","MOV B,H","MOV B,L","MOV B,M","MOV B,A"],
        ["MOV C,B","MOV C,C","MOV C,D","MOV C,E","MOV C,H","MOV C,L","MOV C,M","MOV C,A"],
        ["MOV D,B","MOV D,C","MOV D,D","MOV D,E","MOV D,H","MOV D,L","MOV D,M","MOV D,A"],
        ["MOV E,B","MOV E,C","MOV E,D","MOV E,E","MOV E,H","MOV E,L","MOV E,M","MOV E,A"],
        ["MOV H,B","MOV H,C","MOV H,D","MOV H,E","MOV H,H","MOV H,L","MOV H,M","MOV H,A"],
        ["MOV L,B","MOV L,C","MOV L,D","MOV L,E","MOV L,H","MOV L,L","MOV L,M","MOV L,A"],
        ["MOV M,B","MOV M,C","MOV M,D","MOV M,E","MOV M,H","MOV M,L","MOV M,M","MOV M,A"],
        ["MOV A,B","MOV A,C","MOV A,D","MOV A,E","MOV A,H","MOV A,L","MOV A,M","MOV A,A"],
    ]
}

// --- Control-flow and stack opcodes (used directly by codegen, not operand-pattern matched) ---

use crate::token::FlagKind;

/// Opcode for unconditional `JMP addr` (0xC3). MOL80 `/s`.
pub const JMP: u8 = 0xC3;
/// Opcode for unconditional `CALL addr` (0xCD). MOL80 `CALL(s)`.
pub const CALL: u8 = 0xCD;
/// Opcode for `RET` (0xC9). MOL80 `RTN` (or the rare synonym `/(STK)`).
pub const RET: u8 = 0xC9;
/// CPI n (compare A with immediate) = opcode 254. [CONFIRMED from scan]
pub const CPI: u8 = 0xFE;
/// Opcode for `PCHL` (0xE9) — jump to the address in HL. MOL80 `/HL`.
pub const PCHL: u8 = 0xE9;
/// Opcode for `XCHG` (0xEB) — exchange DE and HL. MOL80 `DE:HL/HL:DE`.
pub const XCHG: u8 = 0xEB;
/// Opcode for `SPHL` (0xF9) — load SP from HL. MOL80 `HL:SP`.
pub const SPHL: u8 = 0xF9;

/// A conditional jump lowered from a flag test.
pub struct CondJumpInfo {
    pub opcode: u8,
    /// Standard 8080 mnemonic (for transpilation).
    pub i8080: &'static str,
}

/// Map a PSW flag test `(flag, value)` to its 8080 conditional-JMP opcode.
///
/// 8080 conditional jumps: JNZ=0xC2, JZ=0xCA, JNC=0xD2, JC=0xDA, JPO=0xE2, JPE=0xEA,
/// JP=0xF2, JM=0xFA. Confirmed by the scan's right column (opcodes 242..=250 region).
/// The opcode is the single source of truth; the mnemonic comes from the shared `i8080-isa`.
pub fn cond_jump(flag: FlagKind, value: u8) -> CondJumpInfo {
    let opcode = match (flag, value) {
        (FlagKind::Z, 1) => 0xCA,
        (FlagKind::Z, 0) => 0xC2,
        (FlagKind::C, 1) => 0xDA,
        (FlagKind::C, 0) => 0xD2,
        (FlagKind::P, 1) => 0xEA, // JPE (parity even)
        (FlagKind::P, 0) => 0xE2, // JPO (parity odd)
        (FlagKind::S, 1) => 0xFA, // JM (sign set => minus)
        (FlagKind::S, 0) => 0xF2, // JP (sign clear => plus)
        _ => 0xC3,                // parser validates value to 0/1; keep total
    };
    // `i8080-isa` renders e.g. "JZ a16"; strip the operand to get the bare mnemonic, then the
    // transpiler appends the target itself.
    let i8080 = bare_mnemonic(opcode);
    CondJumpInfo { opcode, i8080 }
}

/// The bare mnemonic (no operand) for an opcode, from the shared `i8080-isa` table.
/// E.g. opcode 0xCA ("JZ a16") -> "JZ"; 0xC8 ("RZ") -> "RZ".
fn bare_mnemonic(opcode: u8) -> &'static str {
    let full = i8080_isa::lookup(opcode).mnemonic;
    // Mnemonics are "OP" or "OP operand"; take the part before the first space.
    match full.split_once(' ') {
        Some((op, _)) => op,
        None => full,
    }
}

/// Map a flag test to its conditional-CALL opcode. Each Ccc is its Jcc + 2:
/// CNZ=196, CZ=204, CNC=212, CC=220, CPO=228, CPE=236, CP=244, CM=252. [CONFIRMED from scan]
pub fn cond_call(flag: FlagKind, value: u8) -> CondJumpInfo {
    let opcode = cond_jump(flag, value).opcode + 2;
    CondJumpInfo { opcode, i8080: bare_mnemonic(opcode) }
}

/// PUSH rp opcode: B=0xC5, D=0xD5, H=0xE5, PSW=0xF5. Returns None for unsupported operands.
pub fn push_opcode(rp: Reg) -> Option<u8> {
    match rp {
        Reg::BC => Some(0xC5),
        Reg::DE => Some(0xD5),
        Reg::HL => Some(0xE5),
        Reg::Psw => Some(0xF5),
        _ => None,
    }
}

/// POP rp opcode: B=0xC1, D=0xD1, H=0xE1, PSW=0xF1. Returns None for unsupported operands.
pub fn pop_opcode(rp: Reg) -> Option<u8> {
    match rp {
        Reg::BC => Some(0xC1),
        Reg::DE => Some(0xD1),
        Reg::HL => Some(0xE1),
        Reg::Psw => Some(0xF1),
        _ => None,
    }
}

fn inx_name(rp: Reg) -> &'static str {
    match rp { Reg::BC => "INX B", Reg::DE => "INX D", Reg::HL => "INX H", Reg::SP => "INX SP", _ => "INX ?" }
}
fn dcx_name(rp: Reg) -> &'static str {
    match rp { Reg::BC => "DCX B", Reg::DE => "DCX D", Reg::HL => "DCX H", Reg::SP => "DCX SP", _ => "DCX ?" }
}
fn dad_name(rp: Reg) -> &'static str {
    match rp { Reg::BC => "DAD B", Reg::DE => "DAD D", Reg::HL => "DAD H", Reg::SP => "DAD SP", _ => "DAD ?" }
}

/// Standard 8080 name for a register pair (for transpile of PUSH/POP).
pub fn pair_name(rp: Reg) -> &'static str {
    match rp {
        Reg::BC => "B",
        Reg::DE => "D",
        Reg::HL => "H",
        Reg::SP => "SP",
        Reg::Psw => "PSW",
        _ => "?",
    }
}

/// A conditional return lowered from a flag test.
pub struct CondRetInfo {
    pub opcode: u8,
    pub i8080: &'static str,
}

/// Map a PSW flag test `(flag, value)` to its 8080 conditional-RET opcode.
///
/// RNZ=0xC0, RZ=0xC8, RNC=0xD0, RC=0xD8, RPO=0xE0, RPE=0xE8, RP=0xF0, RM=0xF8.
/// The mnemonic comes from the shared `i8080-isa` table.
pub fn cond_return(flag: FlagKind, value: u8) -> CondRetInfo {
    let opcode = match (flag, value) {
        (FlagKind::Z, 1) => 0xC8,
        (FlagKind::Z, 0) => 0xC0,
        (FlagKind::C, 1) => 0xD8,
        (FlagKind::C, 0) => 0xD0,
        (FlagKind::P, 1) => 0xE8,
        (FlagKind::P, 0) => 0xE0,
        (FlagKind::S, 1) => 0xF8,
        (FlagKind::S, 0) => 0xF0,
        _ => 0xC9,
    };
    CondRetInfo {
        opcode,
        i8080: bare_mnemonic(opcode),
    }
}

/// Compare-register opcode CMP r (0xB8 + r), used to lower `A=B? (...)` blocks.
/// Returns None for operands that can't be a CMP source.
pub fn cmp_reg(r: Reg) -> Option<u8> {
    reg_index(r).map(|i| 0xB8 + i as u8)
}

/// The 3-bit register-field index for B,C,D,E,H,L,M,A, or None for pairs/other.
pub fn reg_index(r: Reg) -> Option<usize> {
    REG_ORDER.iter().position(|&x| x == r)
}

use std::sync::OnceLock;

/// The process-wide instruction table, built once.
pub fn isa() -> &'static [IsaEntry] {
    static TABLE: OnceLock<Vec<IsaEntry>> = OnceLock::new();
    TABLE.get_or_init(build_table)
}

/// Convenience alias so call sites read `ISA()`.
#[allow(non_snake_case)]
pub fn ISA() -> &'static [IsaEntry] {
    isa()
}

// `REG_NAME` is kept for future transpile/diagnostic use; reference it to avoid a dead-code
// warning while the codegen/transpile stages are still being built.
#[allow(dead_code)]
const _REG_NAME: [&str; 8] = REG_NAME;
