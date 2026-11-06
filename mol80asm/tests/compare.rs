//! `A=n?` / `A=r?` comparisons: CPI n (opcode 254) and CMP r, in block, jump and return forms.

use mol80asm::{assemble_source, transpile_source};

fn asm(src: &str) -> Vec<u8> {
    let out = assemble_source("t.mol", src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.unwrap().bytes
}

#[test]
fn immediate_block_is_cpi_then_jnz() {
    // CPI 13; JNZ end; (body: INR A = 3C) ; end:
    assert_eq!(
        asm("          A=13? (\n          A+1:A\n          )\n"),
        vec![0xFE, 0x0D, 0xC2, 0x06, 0x00, 0x3C]
    );
}

#[test]
fn immediate_jump_is_cpi_then_jz() {
    assert_eq!(
        asm("          A=13? /DONE\nDONE      RTN\n"),
        vec![0xFE, 0x0D, 0xCA, 0x05, 0x00, 0xC9]
    );
}

#[test]
fn immediate_return_is_cpi_then_rz() {
    assert_eq!(asm("          A=10? RTN\n"), vec![0xFE, 0x0A, 0xC8]);
}

#[test]
fn register_compare_still_works() {
    assert_eq!(asm("          A=B? RTN\n"), vec![0xB8, 0xC8]);
}

#[test]
fn equate_as_compare_value() {
    assert_eq!(asm("CR        EQU 13\n          A=CR? RTN\n"), vec![0xFE, 0x0D, 0xC8]);
}

#[test]
fn out_of_range_is_error() {
    let out = assemble_source("t.mol", "          A=300? RTN\n");
    assert!(out.assembly.is_none());
    assert!(out.diags.render().contains("out of range"));
}

#[test]
fn transpiles_to_cpi() {
    let t = transpile_source("t.mol", "          A=13? RTN\n").asm.unwrap();
    assert!(t.contains("CPI 0DH") && t.contains("RZ"), "{}", t);
}
