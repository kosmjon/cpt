//! Integration test for the trace runner's core behavior: a binary loaded into the machine
//! runs to a terminal HLT with the expected final state. (The CLI/arg parsing is thin glue in
//! main.rs; this exercises the load + execute path the runner relies on.)

use cpt_machine::{Executive, Machine, MachineConfig};
use i8080_cpu::Cpu;

/// Run a bank-0 program from 0x0000 until terminal halt or the step cap; return the executive.
fn run_bin(program: &[u8], max_steps: u64) -> Executive {
    let mut machine = Machine::new(MachineConfig::default());
    machine.mem.load_bank0(0x0000, program);
    let mut cpu = Cpu::new();
    cpu.sp = 0x7F00;
    let mut exec = Executive::new(cpu, machine);
    let mut steps = 0;
    while steps < max_steps && !exec.is_terminally_halted() {
        exec.step();
        steps += 1;
    }
    exec
}

#[test]
fn runs_to_halt_with_expected_state() {
    // MVI A,05 ; MVI B,03 ; ADD B ; HLT  -> A = 8, then terminal HLT.
    let prog = [0x3E, 0x05, 0x06, 0x03, 0x80, 0x76];
    let exec = run_bin(&prog, 100);
    assert!(exec.is_terminally_halted());
    assert_eq!(exec.cpu.a, 0x08);
}

#[test]
fn call_ret_through_the_machine() {
    // CALL 0006 ; HLT ; (0006) MVI A,0x42 ; RET
    let mut prog = vec![0xCD, 0x06, 0x00, 0x76, 0x00, 0x00];
    prog.extend_from_slice(&[0x3E, 0x42, 0xC9]); // at 0x0006
    let exec = run_bin(&prog, 100);
    assert!(exec.is_terminally_halted());
    assert_eq!(exec.cpu.a, 0x42);
    assert_eq!(exec.cpu.pc, 0x0004); // returned to the HLT after the CALL
}

#[test]
fn step_cap_stops_an_infinite_loop() {
    // loop: JMP loop  (never halts)
    let prog = [0xC3, 0x00, 0x00];
    let exec = run_bin(&prog, 50);
    assert!(!exec.is_terminally_halted(), "infinite loop should hit the step cap");
}
