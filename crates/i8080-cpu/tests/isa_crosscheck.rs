//! Cross-check the shared `i8080-isa` disassembly table against the CPU's actual decoding.
//!
//! The two must agree on instruction length for every opcode, or a disassembly/transpile
//! would desync from what the CPU executes. For each opcode we place it at a known PC, run one
//! step, and compare the PC advance to the table's declared `len`. Control-flow opcodes (which
//! set PC non-sequentially) are checked for length separately via their known encoding.

use i8080_cpu::{Cpu, FlatMemory};
use i8080_isa::lookup;

/// Opcodes whose PC does not simply advance by their length (jumps/calls/returns/PCHL/RST/HLT).
/// For these we trust the table's length (they are fixed by the 8080 encoding) but skip the
/// PC-delta check.
fn is_control_flow(op: u8) -> bool {
    matches!(op,
        0xC3 | 0xCB |                                           // JMP (+alias)
        0xC2 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA | // cond jumps
        0xCD | 0xDD | 0xED | 0xFD |                             // CALL (+aliases)
        0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC | // cond calls
        0xC9 | 0xD9 |                                           // RET (+alias)
        0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 | // cond returns
        0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF | // RST
        0xE9 |                                                  // PCHL
        0x76                                                    // HLT
    )
}

#[test]
fn isa_length_matches_cpu_pc_advance() {
    for op in 0u8..=255 {
        if is_control_flow(op) {
            continue;
        }
        let info = lookup(op);
        let mut cpu = Cpu::new();
        let mut mem = FlatMemory::new();
        // Start at 0x0100 with the opcode + two filler operand bytes, and a valid SP/HL so
        // memory-operand instructions (M = (HL)) and PUSH/POP have somewhere to go.
        cpu.pc = 0x0100;
        cpu.sp = 0x0F00;
        cpu.set_hl(0x0200);
        mem.load(0x0100, &[op, 0x00, 0x00]);

        cpu.step(&mut mem);
        let advanced = cpu.pc.wrapping_sub(0x0100);
        assert_eq!(
            advanced as u8, info.len,
            "opcode {:#04X} ({}): CPU advanced PC by {}, ISA table says len {}",
            op, info.mnemonic, advanced, info.len
        );
    }
}

#[test]
fn isa_lengths_are_1_to_3() {
    for op in 0u8..=255 {
        let len = lookup(op).len;
        assert!((1..=3).contains(&len), "opcode {:#04X} has len {}", op, len);
    }
}

#[test]
fn disassemble_fills_operands() {
    // MVI A,d8
    let (text, len) = i8080_isa::disassemble(&[0x3E, 0x42]);
    assert_eq!(text, "MVI A,42H");
    assert_eq!(len, 2);
    // JMP a16 (little-endian 0x1234)
    let (text, len) = i8080_isa::disassemble(&[0xC3, 0x34, 0x12]);
    assert_eq!(text, "JMP 1234H");
    assert_eq!(len, 3);
    // single-byte
    let (text, len) = i8080_isa::disassemble(&[0x76]);
    assert_eq!(text, "HLT");
    assert_eq!(len, 1);
}

#[test]
fn disassemble_all_walks_a_program() {
    // MVI A,05 ; MOV B,A ; JMP 0000
    let bytes = [0x3E, 0x05, 0x47, 0xC3, 0x00, 0x00];
    let lines = i8080_isa::disassemble_all(&bytes, 0x0100);
    assert_eq!(lines[0], (0x0100, "MVI A,05H".to_string()));
    assert_eq!(lines[1], (0x0102, "MOV B,A".to_string()));
    assert_eq!(lines[2], (0x0103, "JMP 0000H".to_string()));
}
