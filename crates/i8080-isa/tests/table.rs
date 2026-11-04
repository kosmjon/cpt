//! Tests for the shared 8080 ISA metadata table.

use i8080_isa::{disassemble, disassemble_all, lookup};

#[test]
fn every_opcode_has_sane_metadata() {
    for op in 0u8..=255 {
        let info = lookup(op);
        assert!((1..=3).contains(&info.len), "opcode {:#04X} len {}", op, info.len);
        assert!(!info.mnemonic.is_empty(), "opcode {:#04X} empty mnemonic", op);
    }
}

#[test]
fn known_opcodes() {
    assert_eq!(lookup(0x00).mnemonic, "NOP");
    assert_eq!(lookup(0x76).mnemonic, "HLT");
    assert_eq!(lookup(0x71).mnemonic, "MOV M,C"); // the C:(HL) example
    assert_eq!(lookup(0x3E).mnemonic, "MVI A,d8");
    assert_eq!(lookup(0xC3).mnemonic, "JMP a16");
    assert_eq!(lookup(0xC9).mnemonic, "RET");
    assert_eq!(lookup(0xEB).mnemonic, "XCHG");
    assert_eq!(lookup(0xE9).mnemonic, "PCHL");
}

#[test]
fn two_and_three_byte_lengths() {
    assert_eq!(lookup(0x3E).len, 2); // MVI A,d8
    assert_eq!(lookup(0x21).len, 3); // LXI H,d16
    assert_eq!(lookup(0xC3).len, 3); // JMP a16
    assert_eq!(lookup(0x47).len, 1); // MOV B,A
}

#[test]
fn disassemble_operands() {
    assert_eq!(disassemble(&[0x3E, 0x2A]), ("MVI A,2AH".to_string(), 2));
    assert_eq!(disassemble(&[0x21, 0x00, 0x80]), ("LXI H,8000H".to_string(), 3));
    assert_eq!(disassemble(&[0xCD, 0x34, 0x12]), ("CALL 1234H".to_string(), 3));
    assert_eq!(disassemble(&[0x00]), ("NOP".to_string(), 1));
}

#[test]
fn disassemble_all_advances_correctly() {
    // LXI H,8000 ; MOV M,A ; RET
    let bytes = [0x21, 0x00, 0x80, 0x77, 0xC9];
    let lines = disassemble_all(&bytes, 0);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], (0x0000, "LXI H,8000H".to_string()));
    assert_eq!(lines[1], (0x0003, "MOV M,A".to_string()));
    assert_eq!(lines[2], (0x0004, "RET".to_string()));
}

/// The MOV block covers 0x40..=0x7F except 0x76 (HLT), all one byte.
#[test]
fn mov_block_shape() {
    for op in 0x40u8..=0x7F {
        let info = lookup(op);
        assert_eq!(info.len, 1);
        if op == 0x76 {
            assert_eq!(info.mnemonic, "HLT");
        } else {
            assert!(info.mnemonic.starts_with("MOV "), "{:#04X} = {}", op, info.mnemonic);
        }
    }
}
