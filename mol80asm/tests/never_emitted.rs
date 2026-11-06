//! Opcodes MOL80 never uses. Most are the 8080's undocumented slots (duplicate NOPs 8, 16, 40,
//! 48, 56; 32 = 8085 RIM; 203/217/237/253 duplicate JMP/RET/CALL); 143 (ADC A), 159 (SBB A)
//! and 167 (ANA A) are real instructions with no MOL80 spelling. [CONFIRMED]
//!
//! This assembles one program containing every MOL80 form we support and checks that none of
//! those opcodes appears in the output.

use mol80asm::assemble_source;

const NEVER: [u8; 13] = [8, 16, 32, 40, 48, 56, 143, 159, 167, 203, 217, 237, 253];

#[test]
fn no_supported_form_emits_an_unused_opcode() {
    let regs = ["B", "C", "D", "E", "H", "L", "(HL)", "A"];
    let mut src = String::new();
    let mut line = |s: &str| {
        src.push_str("          ");
        src.push_str(s);
        src.push('\n');
    };
    for s in &regs {
        for d in &regs {
            if !(s == &"(HL)" && d == &"(HL)") {
                line(&format!("{}:{}", s, d));
            }
        }
        line(&format!("5:{}", s));
        line(&format!("{}+1:{}", s, s));
        line(&format!("{}-1:{}", s, s));
        line(&format!("A+{}:A", s));
        line(&format!("A-{}:A", s));
        if s != &"A" {
            line(&format!("A-*{}:A", s));
            line(&format!("A**{}:A", s));
            line(&format!("A+*{}:A", s));
            line(&format!("A+FC+{}:A", s));
            line(&format!("A-FC-{}:A", s));
        }
    }
    for f in [
        "1:FI", "0:FI", "0:FC<F>", "1:FC", "NOT(FC):FC", "NOT(A):A", "0:A", "PAUSE", "WAIT",
        "A:(BC)", "A:(DE)", "(BC):A", "(DE):A", "STK:HL/HL:STK", "DE:HL/HL:DE", "A<*1:A",
        "A>*1:A", "A,FC<*1:FC,A", "FC,A>*1:A,FC", "DEC(A):A", "A+FC+3:A", "A-FC-3:A",
        "A+3:A", "A-3:A", "A+*3:A", "A-*3:A", "A**3:A", "HL:SP", "/HL", "RTN", "FZ=1? RTN",
        "A=3? RTN", "A=B? RTN", "BC:STK", "STK:BC", "A:STK", "STK:A", "HL+BC:HL", "HL*2:HL",
        "SP+1:SP", "SP-1:SP", "1000:SP", "1000:BC", "A:PORT(5)", "PORT(5):A", "CALL(3)",
        "CALL(1000)", "FZ=0? CALL(1000)", "FS=1? CALL(1000)", "/1000", "FC=1? /1000",
    ] {
        line(f);
    }
    let out = assemble_source("t.mol", &src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    let bytes = out.assembly.unwrap().bytes;
    // Only check opcode positions would be ideal; operand bytes here (5, 3, 0xE8, 0x03) are
    // chosen so they never collide with the forbidden values.
    for op in NEVER {
        assert!(!bytes.contains(&op), "opcode {} was emitted", op);
    }
}
