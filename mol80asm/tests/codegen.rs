//! End-to-end codegen tests: MOL80 source -> assembled bytes.

use mol80asm::assemble_source;

/// Assemble and return the byte image, asserting there were no diagnostics.
fn asm(src: &str) -> Vec<u8> {
    let out = assemble_source("test.mol", src);
    assert!(out.diags.is_empty(), "diagnostics:\n{}", out.diags.render());
    out.assembly.expect("assembly should succeed").bytes
}

/// Assemble expecting failure; return the rendered diagnostics.
fn asm_err(src: &str) -> String {
    let out = assemble_source("test.mol", src);
    assert!(out.assembly.is_none(), "expected assembly to fail");
    assert!(out.diags.has_errors());
    out.diags.render()
}

#[test]
fn simple_moves_and_immediate() {
    // 5:A (3E 05), A:B (47), C:(HL) (71)
    let bytes = asm("          5:A\n          A:B\n          C:(HL)\n");
    assert_eq!(bytes, vec![0x3E, 0x05, 0x47, 0x71]);
}

#[test]
fn alu_block() {
    // A+B:A (80), A+*C:A (A1), A-*D:A (B2), A**E:A (AB)
    let bytes = asm("          A+B:A\n          A+*C:A\n          A-*D:A\n          A**E:A\n");
    assert_eq!(bytes, vec![0x80, 0xA1, 0xB2, 0xAB]);
}

#[test]
fn inr_dcr() {
    // C+1:C (0C), A-1:A (3D)
    let bytes = asm("          C+1:C\n          A-1:A\n");
    assert_eq!(bytes, vec![0x0C, 0x3D]);
}

#[test]
fn unconditional_jump_backward() {
    // LOOP at 0x0000; /LOOP -> C3 00 00
    let bytes = asm("LOOP      A:B\n          /LOOP\n");
    // A:B = 0x47, then JMP 0x0000
    assert_eq!(bytes, vec![0x47, 0xC3, 0x00, 0x00]);
}

#[test]
fn forward_reference() {
    // /DONE before DONE is defined; DONE sits after the 3-byte jump, at 0x0003.
    let bytes = asm("          /DONE\nDONE      A:B\n");
    assert_eq!(bytes, vec![0xC3, 0x03, 0x00, 0x47]);
}

#[test]
fn conditional_jump_jz() {
    // FZ=1? /DONE -> JZ (CA) to DONE. DONE after the 3-byte jump = 0x0003.
    let bytes = asm("          FZ=1? /DONE\nDONE      A:B\n");
    assert_eq!(bytes, vec![0xCA, 0x03, 0x00, 0x47]);
}

#[test]
fn conditional_jump_flag_inversions() {
    // FZ=0? -> JNZ (C2); FC=1? -> JC (DA); FC=0? -> JNC (D2);
    // FS=1? -> JM (FA); FS=0? -> JP (F2); FP=1? -> JPE (EA); FP=0? -> JPO (E2)
    let cases = [
        ("FZ=0?", 0xC2u8),
        ("FC=1?", 0xDA),
        ("FC=0?", 0xD2),
        ("FS=1?", 0xFA),
        ("FS=0?", 0xF2),
        ("FP=1?", 0xEA),
        ("FP=0?", 0xE2),
    ];
    for (test, opcode) in cases {
        let src = format!("          {} /T\nT         A:B\n", test);
        let bytes = asm(&src);
        assert_eq!(bytes[0], opcode, "wrong opcode for {}", test);
        // target is 0x0003 (after the 3-byte jump)
        assert_eq!(&bytes[1..3], &[0x03, 0x00]);
    }
}

#[test]
fn conditional_block_compare_equal() {
    // A=B? (D:B  E:C)
    //   CMP B            B8
    //   JNZ @end         C2 <end>        (skip body when A != B)
    //   MOV B,D          42
    //   MOV C,E          4B
    // @end:
    // Layout: B8 (0) C2 lo hi (1..4) 42 (4) 4B (5) ; @end = 0x0006
    let bytes = asm("          A=B? (D:B  E:C)\n");
    assert_eq!(bytes, vec![0xB8, 0xC2, 0x06, 0x00, 0x42, 0x4B]);
}

#[test]
fn conditional_block_flag() {
    // FC=1? (A:B) lowers to: branch-when-false (JNC, D2) to @end; body A:B; @end.
    // D2 lo hi (0..3) 47 (3) ; @end = 0x0004
    let bytes = asm("          FC=1? (A:B)\n");
    assert_eq!(bytes, vec![0xD2, 0x04, 0x00, 0x47]);
}

#[test]
fn undefined_label_errors() {
    let msg = asm_err("          /NOWHERE\n");
    assert!(msg.contains("undefined label 'NOWHERE'"), "got: {}", msg);
}

#[test]
fn duplicate_label_errors() {
    let msg = asm_err("FOO       A:B\nFOO       A:C\n");
    assert!(msg.contains("duplicate label 'FOO'"), "got: {}", msg);
}

/// The full sample program assembles to the hand-verified byte image.
#[test]
fn sample_program_image() {
    let src = "\
START     5:A
          A:B
          C:(HL)
          A+B:A
          A+*C:A
          A-*D:A
          A**E:A
LOOP      A-1:A
          FZ=1? /DONE
          /LOOP
DONE      A=B? (D:B  E:C)
          /START
";
    let bytes = asm(src);
    // Hand-verified layout (no LXI line here, so it is a clean sequence):
    // 3E 05 | 47 | 71 | 80 | A1 | B2 | AB | 3D | CA <DONE> | C3 <LOOP> | B8 C2 <end> | 42 | 4B | C3 <START>
    // Addresses: START=0, LOOP=0x0008, DONE=0x000F
    let expected = vec![
        0x3E, 0x05, // 5:A        @0000
        0x47, //       A:B        @0002
        0x71, //       C:(HL)     @0003
        0x80, //       A+B:A      @0004
        0xA1, //       A+*C:A     @0005
        0xB2, //       A-*D:A     @0006
        0xAB, //       A**E:A     @0007
        0x3D, //       LOOP A-1:A @0008
        0xCA, 0x0F, 0x00, // FZ=1? /DONE  @0009  (DONE=0x000F)
        0xC3, 0x08, 0x00, // /LOOP         @000C  (LOOP=0x0008)
        0xB8, //       DONE CMP B @000F
        0xC2, 0x15, 0x00, // JNZ @end      @0010  (@end=0x0015)
        0x42, //       D:B        @0013
        0x4B, //       E:C        @0014
        0xC3, 0x00, 0x00, // /START        @0015  (START=0x0000)
    ];
    assert_eq!(bytes, expected);
}
