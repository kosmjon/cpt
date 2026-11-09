//! Interrupt enable timing: on the 8080, EI takes effect only after the instruction that
//! follows it completes, so `EI; RET` at the end of an interrupt handler can't be interrupted
//! between the two. (Without that, a device holding its interrupt line asserted re-enters the
//! handler on every EI and the stack grows without bound.)

use i8080_cpu::{Cpu, FlatMemory};

fn cpu_with(program: &[u8]) -> (Cpu, FlatMemory) {
    let mut mem = FlatMemory::new();
    mem.load(0, program);
    let mut cpu = Cpu::new();
    cpu.sp = 0xF000;
    (cpu, mem)
}

#[test]
fn ei_enables_interrupts_after_the_next_instruction() {
    // EI; NOP; NOP
    let (mut cpu, mut mem) = cpu_with(&[0xFB, 0x00, 0x00]);
    cpu.step(&mut mem);
    assert!(!cpu.int_enabled, "not yet: the instruction after EI hasn't run");
    assert!(!cpu.interrupt(7, &mut mem), "an interrupt is refused in that gap");
    cpu.step(&mut mem);
    assert!(cpu.int_enabled, "enabled once the next instruction completed");
    assert!(cpu.interrupt(7, &mut mem), "and now accepted");
    assert_eq!(cpu.pc, 0x38, "RST 7 vectors to 0038H");
    assert!(!cpu.int_enabled, "accepting an interrupt disables them");
}

#[test]
fn ei_then_ret_runs_the_ret_before_any_interrupt() {
    // 0000: CALL 0010   0010: EI; RET
    let (mut cpu, mut mem) = cpu_with(&[0xCD, 0x10, 0x00]);
    mem.load(0x0010, &[0xFB, 0xC9]);
    cpu.step(&mut mem); // CALL
    cpu.step(&mut mem); // EI
    assert!(!cpu.interrupt(4, &mut mem), "between EI and RET: refused");
    cpu.step(&mut mem); // RET
    assert_eq!(cpu.pc, 0x0003, "the RET ran");
    assert_eq!(cpu.sp, 0xF000, "the stack is back where it started");
    assert!(cpu.int_enabled);
}

#[test]
fn di_right_after_ei_leaves_interrupts_off() {
    // EI; DI; NOP
    let (mut cpu, mut mem) = cpu_with(&[0xFB, 0xF3, 0x00]);
    cpu.step(&mut mem);
    cpu.step(&mut mem);
    assert!(!cpu.int_enabled);
    cpu.step(&mut mem);
    assert!(!cpu.int_enabled, "and it stays off");
}

#[test]
fn ei_then_hlt_waits_for_an_interrupt() {
    // EI; HLT
    let (mut cpu, mut mem) = cpu_with(&[0xFB, 0x76]);
    cpu.step(&mut mem);
    cpu.step(&mut mem);
    assert!(cpu.halted);
    assert!(cpu.int_enabled, "HLT was the instruction after EI, so interrupts are on");
    assert!(cpu.interrupt(1, &mut mem), "an interrupt wakes it");
    assert!(!cpu.halted);
    assert_eq!(cpu.pc, 0x08);
}

#[test]
fn a_second_ei_changes_nothing() {
    // EI; EI; NOP
    let (mut cpu, mut mem) = cpu_with(&[0xFB, 0xFB, 0x00]);
    cpu.step(&mut mem);
    cpu.step(&mut mem);
    assert!(cpu.int_enabled, "the first EI took effect after the second instruction");
    cpu.step(&mut mem);
    assert!(cpu.int_enabled);
}
