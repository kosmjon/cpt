//! Focused unit tests for the 8080 core. The exhaustive coverage comes from the CP/M
//! diagnostics (see `conformance.rs`); these pin down specific behaviors and flag edge cases.

use i8080_cpu::{Cpu, FlatMemory};

/// Assemble a tiny program at 0x0000, run `n` instructions, return the CPU.
fn run(program: &[u8], n: usize) -> (Cpu, FlatMemory) {
    let mut cpu = Cpu::new();
    let mut mem = FlatMemory::new();
    mem.load(0x0000, program);
    for _ in 0..n {
        cpu.step(&mut mem);
    }
    (cpu, mem)
}

#[test]
fn mvi_and_mov() {
    // MVI A,0x42 ; MOV B,A
    let (cpu, _) = run(&[0x3E, 0x42, 0x47], 2);
    assert_eq!(cpu.a, 0x42);
    assert_eq!(cpu.b, 0x42);
}

#[test]
fn lxi_and_memory_via_m() {
    // LXI H,0x1234 ; MVI M,0x99
    let (cpu, mem) = run(&[0x21, 0x34, 0x12, 0x36, 0x99], 2);
    assert_eq!(cpu.hl(), 0x1234);
    assert_eq!(mem.mem[0x1234], 0x99);
}

#[test]
fn add_sets_carry_and_zero() {
    // MVI A,0xFF ; MVI B,0x01 ; ADD B  -> A=0x00, carry set, zero set
    let (cpu, _) = run(&[0x3E, 0xFF, 0x06, 0x01, 0x80], 3);
    assert_eq!(cpu.a, 0x00);
    assert!(cpu.flags.carry);
    assert!(cpu.flags.zero);
    assert!(cpu.flags.aux_carry); // 0xF + 0x1 overflows the low nibble
}

#[test]
fn sub_sets_borrow() {
    // MVI A,0x05 ; MVI B,0x0A ; SUB B -> A=0xFB, carry (borrow) set, sign set
    let (cpu, _) = run(&[0x3E, 0x05, 0x06, 0x0A, 0x90], 3);
    assert_eq!(cpu.a, 0xFB);
    assert!(cpu.flags.carry);
    assert!(cpu.flags.sign);
}

#[test]
fn inr_dcr_dont_touch_carry() {
    // STC (carry=1) ; MVI A,0xFF ; INR A -> A=0, carry must remain set
    let (cpu, _) = run(&[0x37, 0x3E, 0xFF, 0x3C], 3);
    assert_eq!(cpu.a, 0x00);
    assert!(cpu.flags.zero);
    assert!(cpu.flags.carry, "INR must not affect carry");
}

#[test]
fn parity_flag() {
    // MVI A,0x03 ; ORA A  -> 0x03 has two set bits -> even parity (true)
    let (cpu, _) = run(&[0x3E, 0x03, 0xB7], 2);
    assert!(cpu.flags.parity);
    // MVI A,0x07 ; ORA A  -> three set bits -> odd parity (false)
    let (cpu2, _) = run(&[0x3E, 0x07, 0xB7], 2);
    assert!(!cpu2.flags.parity);
}

#[test]
fn rotates() {
    // MVI A,0x80 ; RLC -> A=0x01, carry=1
    let (cpu, _) = run(&[0x3E, 0x80, 0x07], 2);
    assert_eq!(cpu.a, 0x01);
    assert!(cpu.flags.carry);
    // MVI A,0x01 ; RRC -> A=0x80, carry=1
    let (cpu2, _) = run(&[0x3E, 0x01, 0x0F], 2);
    assert_eq!(cpu2.a, 0x80);
    assert!(cpu2.flags.carry);
}

#[test]
fn dad_sets_carry() {
    // LXI H,0x8000 ; LXI B,0x8000 ; DAD B -> HL=0x0000 carry set
    let (cpu, _) = run(&[0x21, 0x00, 0x80, 0x01, 0x00, 0x80, 0x09], 3);
    assert_eq!(cpu.hl(), 0x0000);
    assert!(cpu.flags.carry);
}

#[test]
fn push_pop_psw_roundtrip() {
    // Set up a known A and flags, PUSH PSW, change A, POP PSW -> flags/A restored.
    // MVI A,0xAA ; ORA A (sets S, clears Z/C) ; PUSH PSW ; MVI A,0x00 ; POP PSW
    let prog = [0x3E, 0xAA, 0xB7, 0xF5, 0x3E, 0x00, 0xF1];
    let mut cpu = Cpu::new();
    let mut mem = FlatMemory::new();
    cpu.sp = 0x8000;
    mem.load(0x0000, &prog);
    for _ in 0..5 {
        cpu.step(&mut mem);
    }
    assert_eq!(cpu.a, 0xAA);
    assert!(cpu.flags.sign);
    assert!(!cpu.flags.zero);
}

#[test]
fn call_and_ret() {
    // CALL 0x0006 ; (pad) ; at 0x0006: MVI A,0x7 ; RET
    // 0000: CD 06 00   CALL 0006
    // 0003: 76         HLT  (should be reached after RET)
    // 0006: 3E 07      MVI A,07
    // 0008: C9         RET
    let mut cpu = Cpu::new();
    let mut mem = FlatMemory::new();
    cpu.sp = 0x8000;
    mem.load(0x0000, &[0xCD, 0x06, 0x00, 0x76]);
    mem.load(0x0006, &[0x3E, 0x07, 0xC9]);
    cpu.step(&mut mem); // CALL
    assert_eq!(cpu.pc, 0x0006);
    cpu.step(&mut mem); // MVI A,7
    cpu.step(&mut mem); // RET
    assert_eq!(cpu.pc, 0x0003);
    assert_eq!(cpu.a, 0x07);
}

#[test]
fn conditional_jump_taken_and_not() {
    // MVI A,0 ; ORA A (Z=1) ; JZ 0x0008 ; HLT ; at 8: MVI A,0x55
    let mut cpu = Cpu::new();
    let mut mem = FlatMemory::new();
    mem.load(0x0000, &[0x3E, 0x00, 0xB7, 0xCA, 0x08, 0x00, 0x76]);
    mem.load(0x0008, &[0x3E, 0x55]);
    for _ in 0..3 {
        cpu.step(&mut mem);
    }
    assert_eq!(cpu.pc, 0x0008, "JZ should be taken when zero flag set");
}

#[test]
fn xchg_swaps_de_hl() {
    // LXI H,0x1111 ; LXI D,0x2222 ; XCHG
    let (cpu, _) = run(&[0x21, 0x11, 0x11, 0x11, 0x22, 0x22, 0xEB], 3);
    // note: second LXI opcode is 0x11 (LXI D)
    assert_eq!(cpu.hl(), 0x2222);
    assert_eq!(cpu.de(), 0x1111);
}

#[test]
fn daa_basic() {
    // MVI A,0x9B ; DAA -> 0x01 with carry (0x9B + 0x66 = 0x101)
    let (cpu, _) = run(&[0x3E, 0x9B, 0x27], 2);
    assert_eq!(cpu.a, 0x01);
    assert!(cpu.flags.carry);
}

#[test]
fn interrupt_rst_vector() {
    let mut cpu = Cpu::new();
    let mut mem = FlatMemory::new();
    cpu.sp = 0x8000;
    cpu.pc = 0x1234;
    cpu.int_enabled = true;
    let accepted = cpu.interrupt(7, &mut mem); // RST 7 -> 0x0038
    assert!(accepted);
    assert_eq!(cpu.pc, 0x0038);
    assert!(!cpu.int_enabled, "interrupt accept clears IE");
    // return address pushed
    assert_eq!(mem.mem[0x7FFE], 0x34);
    assert_eq!(mem.mem[0x7FFF], 0x12);
}

#[test]
fn interrupt_ignored_when_disabled() {
    let mut cpu = Cpu::new();
    let mut mem = FlatMemory::new();
    cpu.int_enabled = false;
    assert!(!cpu.interrupt(7, &mut mem));
}
