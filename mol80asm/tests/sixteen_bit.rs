//! Tests for 16-bit pair operations, the dual-move exchange, and register-indirect jump.
//! These exercise the "one MOL80 mnemonic may map to multiple 8080 opcodes" case.

use mol80asm::{assemble_source, transpile_source};

fn asm(src: &str) -> Vec<u8> {
    let out = assemble_source("t.mol", src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.unwrap().bytes
}

fn tr(src: &str) -> String {
    let out = transpile_source("t.mol", src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.asm.unwrap()
}

#[test]
fn xchg_single_opcode() {
    // DE:HL/HL:DE is the dedicated exchange -> XCHG (0xEB), a single byte.
    assert_eq!(asm("          DE:HL/HL:DE\n"), vec![0xEB]);
    assert!(tr("          DE:HL/HL:DE\n").contains("XCHG"));
}

#[test]
fn exchange_expands_to_multiple_opcodes() {
    // BC<->HL has no dedicated opcode: PUSH B ; MOV B,H ; MOV C,L ; POP H.
    assert_eq!(asm("          BC:HL/HL:BC\n"), vec![0xC5, 0x44, 0x4D, 0xE1]);
    let a = tr("          BC:HL/HL:BC\n");
    assert!(a.contains("PUSH B"));
    assert!(a.contains("MOV B,H"));
    assert!(a.contains("MOV C,L"));
    assert!(a.contains("POP H"));
}

#[test]
fn inx_family() {
    assert_eq!(asm("          BC+1:BC\n"), vec![0x03]);
    assert_eq!(asm("          DE+1:DE\n"), vec![0x13]);
    assert_eq!(asm("          HL+1:HL\n"), vec![0x23]);
    assert_eq!(asm("          SP+1:SP\n"), vec![0x33]);
}

#[test]
fn dcx_family() {
    assert_eq!(asm("          BC-1:BC\n"), vec![0x0B]);
    assert_eq!(asm("          DE-1:DE\n"), vec![0x1B]);
    assert_eq!(asm("          HL-1:HL\n"), vec![0x2B]);
    assert_eq!(asm("          SP-1:SP\n"), vec![0x3B]);
}

#[test]
fn dad_family() {
    assert_eq!(asm("          HL+BC:HL\n"), vec![0x09]);
    assert_eq!(asm("          HL+DE:HL\n"), vec![0x19]);
    assert_eq!(asm("          HL*2:HL\n"), vec![0x29]); // DAD H
    assert_eq!(asm("          HL+SP:HL\n"), vec![0x39]);
}

#[test]
fn sphl_and_pchl() {
    assert_eq!(asm("          HL:SP\n"), vec![0xF9]); // SPHL
    assert_eq!(asm("          /HL\n"), vec![0xE9]); // PCHL
    assert!(tr("          HL:SP\n").contains("SPHL"));
    assert!(tr("          /HL\n").contains("PCHL"));
}

#[test]
fn non_mirrored_dual_move_errors() {
    // `DE:HL/BC:DE` is not a mirrored swap and should error.
    let out = assemble_source("t.mol", "          DE:HL/BC:DE\n");
    assert!(out.diags.has_errors());
}
