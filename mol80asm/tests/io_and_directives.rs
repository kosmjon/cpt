//! Tests for PORT(n) I/O (confirmed MOL80 syntax) and the ORG/DB/DW/DS directives
//! (our-convention placeholders).

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

// --- PORT(n) ---

#[test]
fn out_to_port() {
    // A:PORT(112) -> OUT 0x70
    assert_eq!(asm("          A:PORT(112)\n").bytes, vec![0xD3, 0x70]);
}

#[test]
fn in_from_port() {
    // PORT(113):A -> IN 0x71
    assert_eq!(asm("          PORT(113):A\n").bytes, vec![0xDB, 0x71]);
}

#[test]
fn port_hex_number() {
    // PORT(70H):A -> IN 0x70
    assert_eq!(asm("          PORT(70H):A\n").bytes, vec![0xDB, 0x70]);
}

#[test]
fn port_with_non_a_is_error() {
    // Only A pairs with a port.
    let msg = asm_err("          B:PORT(10)\n");
    assert!(msg.to_lowercase().contains("port"), "got: {}", msg);
}

// --- directives ---

#[test]
fn org_sets_origin_and_labels() {
    let a = asm("          ORG 256\nHERE      A:B\n          /HERE\n");
    assert_eq!(a.origin, 0x0100);
    // A:B (0x47) then JMP HERE. HERE is at 0x0100.
    assert_eq!(a.bytes[0], 0x47);
    // The JMP target bytes should resolve to 0x0100.
    // bytes: 47 C3 00 01
    assert_eq!(&a.bytes[1..4], &[0xC3, 0x00, 0x01]);
}

#[test]
fn db_bytes_and_strings() {
    let a = asm("          DB \"Hi\", 13, 10, 0\n");
    assert_eq!(a.bytes, vec![b'H', b'i', 13, 10, 0]);
}

#[test]
fn dw_little_endian() {
    let a = asm("          DW 258, 1\n"); // 0x0102, 0x0001
    assert_eq!(a.bytes, vec![0x02, 0x01, 0x01, 0x00]);
}

#[test]
fn ds_reserves_zeros() {
    let a = asm("          A:B\n          DS 3\n          A:C\n");
    // MOV B,A (47), 3 zero bytes, MOV C,A (4F)
    assert_eq!(a.bytes, vec![0x47, 0x00, 0x00, 0x00, 0x4F]);
}

#[test]
fn org_forward_fill() {
    // Code at 0, then ORG 5 forward-fills to offset 5.
    let a = asm("          A:B\n          ORG 5\n          A:C\n");
    // 47 at 0, zero-fill 1..5, 4F at 5
    assert_eq!(a.bytes, vec![0x47, 0x00, 0x00, 0x00, 0x00, 0x4F]);
}

#[test]
fn org_backward_is_error() {
    let msg = asm_err("          ORG 10\n          A:B\n          ORG 2\n          A:C\n");
    assert!(msg.contains("backward"), "got: {}", msg);
}
