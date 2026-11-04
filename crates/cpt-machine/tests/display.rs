//! Tests for the memory-mapped display device and the vertical-retrace interrupt.

use cpt_machine::{DisplayDevice, Executive, Machine, MachineConfig, COLS, ROWS};
use i8080_cpu::{Bus, Cpu};

fn machine_with_display() -> Machine {
    let cfg = MachineConfig::default();
    let mut m = Machine::new(cfg.clone());
    m.attach_display(DisplayDevice::new(cfg.retrace_period_cycles, cfg.retrace_vector));
    m
}

#[test]
fn cpu_writes_land_in_display_ram() {
    let mut m = machine_with_display();
    let base = m.config().display_base;

    // Write a couple of character codes through the CPU memory interface.
    m.write(base, b'H');
    m.write(base + 1, b'I');

    // They read back through the bus...
    assert_eq!(m.read(base), b'H');
    assert_eq!(m.read(base + 1), b'I');

    // ...and appear in the display snapshot at row 0, cols 0 and 1.
    let snap = m.display_mut().unwrap().snapshot();
    assert_eq!(snap.cols, COLS);
    assert_eq!(snap.rows, ROWS);
    assert_eq!(snap.cell(0, 0), b'H');
    assert_eq!(snap.cell(0, 1), b'I');
}

#[test]
fn display_window_does_not_leak_into_banked_ram() {
    // A write just below the display base goes to banked RAM, not the display.
    let mut m = machine_with_display();
    let base = m.config().display_base;
    m.write(base - 1, 0xAB);
    assert_eq!(m.mem.read(base - 1), 0xAB);
    // And the display cell at offset 0 is untouched.
    assert_eq!(m.display_mut().unwrap().read_cell(0), 0x00);
}

#[test]
fn layout_regions_are_ordered_and_cover_the_frame() {
    let m = {
        let mut m = machine_with_display();
        let l = m.display_mut().unwrap().layout();
        // title < top < typing < bottom < status, all within ROWS.
        assert_eq!(l.title_row, 0);
        assert!(l.top_text.0 <= l.top_text.1);
        assert!(l.top_text.1 < l.typing_row);
        assert!(l.typing_row < l.bottom_text.0);
        assert!(l.bottom_text.0 <= l.bottom_text.1);
        assert!(l.bottom_text.1 < l.status.0);
        assert!(l.status.1 < ROWS);
        // Bottom region is the small one (~8 lines).
        let bottom_lines = l.bottom_text.1 - l.bottom_text.0 + 1;
        assert!(bottom_lines <= 10, "bottom region should be small, got {}", bottom_lines);
        m
    };
    drop(m);
}

#[test]
fn retrace_interrupt_fires_periodically() {
    let cfg = MachineConfig::default();
    let mut d = DisplayDevice::new(1000, cfg.retrace_vector);
    use cpt_machine::Device;
    // Not yet a full period.
    d.tick(600);
    assert_eq!(d.take_irq(), None);
    // Cross the period -> one retrace.
    d.tick(600); // 1200 total
    assert_eq!(d.take_irq(), Some(cfg.retrace_vector));
    assert_eq!(d.take_irq(), None);
    assert_eq!(d.retrace_count, 1);
}

/// End-to-end: a "retrace handler" in bank 0 scrolls the display by writing a marker, and it
/// only runs because the retrace IRQ fires. This mirrors how CPT drains queued scroll actions
/// during vertical retrace — the emulator provides the IRQ, the software does the scrolling.
#[test]
fn retrace_handler_runs_and_updates_display() {
    let cfg = MachineConfig::default();
    let mut m = Machine::new(cfg.clone());
    // Short period so the test runs quickly.
    m.attach_display(DisplayDevice::new(50, cfg.retrace_vector));
    let base = cfg.display_base;

    // Retrace vector is RST `retrace_vector` -> address vector*8.
    let handler_addr = (cfg.retrace_vector as u16) * 8;

    // Main program at 0x0100: EI ; loop: JMP loop  (idle, waiting for retrace)
    //   0100: FB         EI
    //   0101: C3 01 01   JMP 0101
    m.mem.load_bank0(0x0100, &[0xFB, 0xC3, 0x01, 0x01]);

    // Retrace handler at vector: write 'X' to display cell 0, then EI + RET so further
    // retraces keep coming (it pushed the return addr; RET resumes the idle loop).
    //   MVI A,'X'      3E 58
    //   STA <base>     32 lo hi
    //   EI             FB
    //   RET            C9
    let b = base.to_le_bytes();
    m.mem.load_bank0(handler_addr, &[0x3E, b'X', 0x32, b[0], b[1], 0xFB, 0xC9]);

    let mut cpu = Cpu::new();
    cpu.pc = 0x0100;
    cpu.sp = 0x7000;
    let mut exec = Executive::new(cpu, m);

    // Run enough cycles for a couple of retraces to occur.
    exec.run_cycles(500);

    // The handler ran (driven only by the retrace IRQ) and wrote 'X' to display cell 0.
    assert_eq!(exec.machine.read(base), b'X');
    assert!(
        exec.machine.display_mut().unwrap().retrace_count >= 1,
        "at least one retrace interrupt should have fired"
    );
}
