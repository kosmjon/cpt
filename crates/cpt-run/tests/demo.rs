//! Test the built-in display demo: it draws the regions and the retrace handler scrolls.

use cpt_machine::{DisplayDevice, Executive, Machine, MachineConfig};
use i8080_cpu::{Bus, Cpu};

// Re-declare the demo builder path by including the module. The binary crate's `demo` module
// isn't importable from an integration test, so we reproduce the run here using the same
// config and the demo bytes via a small copy of the entry points we need.
#[path = "../src/demo.rs"]
mod demo;

fn run_demo(steps: u64) -> Executive {
    let cfg = MachineConfig::default();
    let mut machine = Machine::new(cfg.clone());
    machine.attach_display(DisplayDevice::new(cfg.retrace_period_cycles, cfg.retrace_vector));
    machine.mem.load_bank0(0x0100, &demo::display_demo());
    let mut cpu = Cpu::new();
    cpu.pc = 0x0100;
    cpu.sp = 0x7F00;
    let mut exec = Executive::new(cpu, machine);
    let mut n = 0;
    while n < steps && !exec.is_terminally_halted() {
        exec.step();
        n += 1;
    }
    exec
}

fn display_base() -> u16 {
    MachineConfig::default().display_base
}

#[test]
fn demo_draws_regions() {
    // Short run: just the initial draw, before any retrace scroll.
    let mut exec = run_demo(2500);
    let base = display_base();

    // Title row begins with 'C' ("CPT ...").
    assert_eq!(exec.machine.read(base), b'C');
    // Typing-line ruler row (row 45) starts with the '>' left-margin marker.
    let typing = base + 45 * 80;
    assert_eq!(exec.machine.read(typing), b'>');
    assert_eq!(exec.machine.read(typing + 1), b'-');
    // Status row (54) begins with 'S' ("STATUS: running").
    let status = base + 54 * 80;
    assert_eq!(exec.machine.read(status), b'S');
    // First top-half row (row 1) starts with 'A' before any scrolling.
    assert_eq!(exec.machine.read(base + 80), b'A');
}

#[test]
fn retrace_handler_scrolls_top_half() {
    // Long enough for several retrace interrupts.
    let mut exec = run_demo(40_000);
    let base = display_base();

    let raised = exec.machine.display_mut().unwrap().retrace_count;
    assert!(raised >= 1, "expected at least one retrace interrupt, got {}", raised);

    // The handler scrolls the top-half up by one row per *serviced* retrace. The big block
    // copy takes longer than one retrace period, so some raised IRQs are missed while the
    // handler runs with interrupts disabled — i.e. serviced scrolls <= raised. We therefore
    // assert the scroll is correct in *kind*, not an exact count:
    //
    //   * row 1 is no longer 'A' (it scrolled),
    //   * the top rows still form the consecutive A..Z sequence shifted up by some k >= 1,
    //   * regions outside the scroll area are untouched.
    let row1 = exec.machine.read(base + 80);
    assert_ne!(row1, b'A', "top row should have scrolled away from its initial 'A'");
    let k = (row1 - b'A') as u64; // number of effective scrolls
    assert!(k >= 1 && k <= raised, "scrolls {} should be >=1 and <= raised {}", k, raised);

    // Row 2 should be the next letter after row 1 (sequence preserved by the block copy).
    let row2 = exec.machine.read(base + 2 * 80);
    assert_eq!(row2, b'A' + ((k + 1) % 26) as u8, "letter sequence should be intact after scroll");
    let _ = k;

    // The title, typing line, and status are outside the scrolled region and unchanged.
    assert_eq!(exec.machine.read(base), b'C'); // title
    assert_eq!(exec.machine.read(base + 45 * 80), b'>'); // typing ruler
    assert_eq!(exec.machine.read(base + 54 * 80), b'S'); // status
}
