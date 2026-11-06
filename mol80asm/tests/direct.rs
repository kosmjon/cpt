//! Direct memory access through `VAR` symbols: LDA 58 / STA 50 / LHLD 42 / SHLD 34 (decimal).
//! Spelling is our-convention [VERIFY]; the opcodes are the standard 8080 ones.

use mol80asm::{assemble_source, transpile_source};

fn asm(src: &str) -> Vec<u8> {
    let out = assemble_source("t.mol", src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.unwrap().bytes
}

fn asm_err(src: &str) -> String {
    let out = assemble_source("t.mol", src);
    assert!(out.assembly.is_none(), "expected failure");
    out.diags.render()
}

const VARS: &str = "TRK       VAR 0F000H\nSEC       VAR TRK+2\n";

#[test]
fn four_direct_forms() {
    let src = format!("{}          TRK:A\n          A:TRK\n          TRK:HL\n          HL:TRK\n", VARS);
    assert_eq!(
        asm(&src),
        vec![0x3A, 0x00, 0xF0, 0x32, 0x00, 0xF0, 0x2A, 0x00, 0xF0, 0x22, 0x00, 0xF0]
    );
}

#[test]
fn var_defined_from_another_var() {
    let src = format!("{}          A:SEC\n", VARS);
    assert_eq!(asm(&src), vec![0x32, 0x02, 0xF0]);
}

#[test]
fn equ_stays_immediate() {
    // Same shapes with EQU are MVI / LXI, not direct memory.
    assert_eq!(asm("K         EQU 5\n          K:A\n          K:HL\n"), vec![0x3E, 0x05, 0x21, 0x05, 0x00]);
}

#[test]
fn var_with_other_register_is_error() {
    let m = asm_err(&format!("{}          TRK:B\n", VARS));
    assert!(m.contains("VAR"), "got: {}", m);
}

#[test]
fn var_reserved_and_duplicate() {
    assert!(asm_err("VAR       VAR 1\n").contains("reserved"));
    assert!(asm_err("X         VAR 1\nX         EQU 2\n").contains("duplicate"));
}

#[test]
fn transpiles_direct() {
    let t = transpile_source("t.mol", &format!("{}          TRK:A\n          HL:TRK\n", VARS)).asm.unwrap();
    assert!(t.contains("LDA F000H") && t.contains("SHLD F000H"), "{}", t);
}
