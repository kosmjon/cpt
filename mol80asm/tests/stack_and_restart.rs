//! Confirmed from the scan: `A:(BC)` = STAX B (2), `A:(DE)` = STAX D (18),
//! `STK:HL/HL:STK` = XTHL (227), `CALL(0)`..`CALL(7)` = RST 0..7 (199..255 step 8).

use mol80asm::{assemble_source, transpile_source};

fn asm(src: &str) -> Vec<u8> {
    let out = assemble_source("t.mol", src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.unwrap().bytes
}

#[test]
fn store_a_through_bc_and_de() {
    assert_eq!(asm("          A:(BC)\n          A:(DE)\n"), vec![2, 18]);
}

#[test]
fn plain_pairs_are_unaffected() {
    // BC/DE without parentheses are still register pairs (push, LXI).
    assert_eq!(asm("          BC:STK\n          5:DE\n"), vec![0xC5, 0x11, 0x05, 0x00]);
}

#[test]
fn exchange_hl_with_top_of_stack() {
    assert_eq!(asm("          STK:HL/HL:STK\n"), vec![227]);
    assert_eq!(asm("          HL:STK/STK:HL\n"), vec![227]);
}

#[test]
fn restarts() {
    let src: String = (0..8).map(|n| format!("          CALL({})\n", n)).collect();
    assert_eq!(asm(&src), vec![199, 207, 215, 223, 231, 239, 247, 255]);
}

#[test]
fn call_to_eight_and_above_is_a_real_call() {
    assert_eq!(asm("          CALL(8)\n"), vec![0xCD, 0x08, 0x00]);
}

#[test]
fn equate_below_eight_is_a_restart() {
    assert_eq!(asm("BREAK     EQU 7\n          CALL(BREAK)\n"), vec![255]);
}

#[test]
fn transpiles() {
    let t = transpile_source("t.mol", "          A:(BC)\n          A:(DE)\n          STK:HL/HL:STK\n          CALL(1)\n")
        .asm
        .unwrap();
    for m in ["STAX B", "STAX D", "XTHL", "RST 1"] {
        assert!(t.contains(m), "missing {} in {}", m, t);
    }
}
