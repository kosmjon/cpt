//! Conditional calls, confirmed from the scan: `Fx=v? CALL(s)`.
//! CNZ 196, CZ 204, CNC 212, CC 220, CPO 228, CPE 236, CP 244, CM 252.

use mol80asm::{assemble_source, transpile_source};

fn asm(src: &str) -> mol80asm::codegen::Assembly {
    let out = assemble_source("t.mol", src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.unwrap()
}

#[test]
fn all_eight_conditional_calls() {
    let tests = [
        ("FZ=0", 196), ("FZ=1", 204), ("FC=0", 212), ("FC=1", 220),
        ("FP=0", 228), ("FP=1", 236), ("FS=0", 244), ("FS=1", 252),
    ];
    for (cond, op) in tests {
        let a = asm(&format!("          {}? CALL(SUB)\nSUB       RTN\n", cond));
        assert_eq!(a.bytes, vec![op, 0x03, 0x00, 0xC9], "{}", cond);
        assert_eq!(a.relocations.len(), 1, "label target should be relocatable");
    }
}

#[test]
fn absolute_target() {
    assert_eq!(asm("          FZ=1? CALL(0F600H)\n").bytes, vec![204, 0x00, 0xF6]);
}

#[test]
fn restart_number_means_address_8n() {
    // CALL(1) is RST 1 (address 8); the conditional form calls address 8.
    assert_eq!(asm("          FC=1? CALL(1)\n").bytes, vec![220, 0x08, 0x00]);
}

#[test]
fn transpiles() {
    let t = transpile_source("t.mol", "          FZ=0? CALL(SUB)\n          FS=1? CALL(SUB)\nSUB       RTN\n")
        .asm
        .unwrap();
    assert!(t.contains("CNZ SUB") && t.contains("CM SUB"), "{}", t);
}
