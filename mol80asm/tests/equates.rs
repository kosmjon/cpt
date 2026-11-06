//! Tests for `NAME EQU value` constants (our-convention placeholder syntax).

use mol80asm::assemble_source;

fn asm(src: &str) -> mol80asm::codegen::Assembly {
    let out = assemble_source("t.mol", src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.unwrap()
}

fn asm_err(src: &str) -> String {
    let out = assemble_source("t.mol", src);
    assert!(out.assembly.is_none(), "expected failure");
    out.diags.render()
}

#[test]
fn equate_as_out_port() {
    let a = asm("SERDATA   EQU 112\n          A:PORT(SERDATA)\n");
    assert_eq!(a.bytes, vec![0xD3, 0x70]);
}

#[test]
fn equate_expression_in_in_port() {
    let a = asm("SERDATA   EQU 112\nSERSTAT   EQU SERDATA+1\n          PORT(SERSTAT):A\n");
    assert_eq!(a.bytes, vec![0xDB, 0x71]);
}

#[test]
fn port_expression_directly() {
    let a = asm("SERDATA   EQU 70H\n          PORT(SERDATA+1):A\n");
    assert_eq!(a.bytes, vec![0xDB, 0x71]);
}

#[test]
fn forward_use_works() {
    let a = asm("          A:PORT(SERDATA)\nSERDATA   EQU 112\n");
    assert_eq!(a.bytes, vec![0xD3, 0x70]);
}

#[test]
fn equate_as_immediate() {
    let a = asm("TXRDY     EQU 2\n          A+*TXRDY:A\n          TXRDY:B\n");
    assert_eq!(a.bytes, vec![0xE6, 0x02, 0x06, 0x02]);
}

#[test]
fn equate_as_absolute_address_has_no_relocation() {
    let a = asm("BIOS      EQU 0F600H\n          CALL(BIOS)\n          /BIOS\n");
    assert_eq!(a.bytes, vec![0xCD, 0x00, 0xF6, 0xC3, 0x00, 0xF6]);
    assert!(a.relocations.is_empty());
}

#[test]
fn equate_in_db_and_dw() {
    let a = asm("CR        EQU 13\nBIOS      EQU 0F600H\n          DB CR,10\n          DW BIOS\n");
    assert_eq!(a.bytes, vec![13, 10, 0x00, 0xF6]);
}

#[test]
fn hex_binary_and_arithmetic() {
    let a = asm(
        "BASE      EQU 10H\nLIM       EQU BASE+101B-1\n          DB BASE,LIM\n",
    );
    assert_eq!(a.bytes, vec![0x10, 0x14]);
}

#[test]
fn equate_emits_nothing_and_keeps_location() {
    let a = asm("          ORG 100H\nK         EQU 5\nHERE      DB 1\n          /HERE\n");
    assert_eq!(a.bytes, vec![1, 0xC3, 0x00, 0x01]);
}

#[test]
fn duplicate_equate_is_error() {
    let m = asm_err("K         EQU 1\nK         EQU 2\n");
    assert!(m.contains("duplicate"), "got: {}", m);
}

#[test]
fn undefined_equate_in_equ_is_error() {
    let m = asm_err("X         EQU Y\n");
    assert!(m.contains("undefined equate"), "got: {}", m);
}

#[test]
fn reserved_names_are_errors() {
    assert!(asm_err("A         EQU 1\n").contains("reserved"));
    assert!(asm_err("DB        EQU 1\n").contains("reserved"));
}

#[test]
fn equ_without_name_is_error() {
    let m = asm_err("          EQU 1\n");
    assert!(m.contains("name"), "got: {}", m);
}

#[test]
fn label_conflicting_with_equate_is_error() {
    let m = asm_err("K         EQU 1\nK         DB 1\n");
    assert!(m.contains("conflicts"), "got: {}", m);
}

#[test]
fn undefined_equate_in_port_is_error() {
    let m = asm_err("          A:PORT(NOPE)\n");
    assert!(m.contains("undefined equate"), "got: {}", m);
}

#[test]
fn bad_expression_is_error() {
    assert!(asm_err("K         EQU 1+\n").contains("missing operand"));
    assert!(asm_err("K         EQU 1+*2\n").contains("invalid"));
}

#[test]
fn port_out_of_range_still_errors() {
    let m = asm_err("BIG       EQU 300\n          A:PORT(BIG)\n");
    assert!(m.contains("out of range"), "got: {}", m);
}
