//! Verify the demo's disk-read routine: with a disk mounted in A, block data should stream
//! into the bottom-half display region, and the drive's activity LED should light.

use cpt_machine::disk::{DiskImage, Geometry, Station};
use cpt_machine::{DiskController, DisplayDevice, Executive, Machine, MachineConfig};
use i8080_cpu::{Bus, Cpu};

#[test]
fn demo_reads_disk_block_into_bottom_region() {
    let cfg = MachineConfig::default();
    let geom = Geometry::default();
    let mut machine = Machine::new(cfg.clone());
    machine.attach_display(DisplayDevice::new(cfg.retrace_period_cycles, cfg.retrace_vector));

    // Build a disk whose block 0 starts with a recognizable marker.
    let mut img = DiskImage::blank(geom);
    let mut block0 = vec![0u8; geom.block_size];
    for (i, b) in b"DISKDATA".iter().enumerate() {
        block0[i] = *b;
    }
    img.write_block(0, &block0);

    let mut ctrl = DiskController::new(cfg.disk_base_port, geom);
    ctrl.mount(Station::A, img);
    machine.attach_device(Box::new(ctrl));

    machine.mem.load_bank0(0x0100, &cpt_machine::demo::display_demo());

    let mut cpu = Cpu::new();
    cpu.pc = 0x0100;
    cpu.sp = 0x7F00;
    let mut exec = Executive::new(cpu, machine);

    // Run just enough to complete setup and exactly one disk read (before the pacing delay
    // and a second read can occur).
    exec.run_cycles(60_000);

    // The bottom-half region starts at row 46. Block 0's first bytes should appear there.
    let base = cfg.display_base;
    let bottom = base + 46 * 80;
    let got: String = (0..8)
        .map(|i| exec.machine.read(bottom + i) as char)
        .collect();
    assert_eq!(got, "DISKDATA", "block 0 data should be read into the bottom-half region");

    // The read lit drive A's activity LED.
    let active = exec
        .machine
        .device_mut(1)
        .unwrap()
        .as_any_mut()
        .downcast_mut::<DiskController>()
        .unwrap()
        .is_active(Station::A);
    assert!(active, "reading a block should light drive A's activity LED");
}

#[test]
fn demo_with_no_disk_does_not_hang_or_draw() {
    // No disk mounted: the read returns no-media and the routine skips drawing; the machine
    // should keep running (retrace scroll still happens) without hanging.
    let cfg = MachineConfig::default();
    let mut machine = Machine::new(cfg.clone());
    machine.attach_display(DisplayDevice::new(cfg.retrace_period_cycles, cfg.retrace_vector));
    machine.attach_device(Box::new(DiskController::new(cfg.disk_base_port, Geometry::default())));
    machine.mem.load_bank0(0x0100, &cpt_machine::demo::display_demo());

    let mut cpu = Cpu::new();
    cpu.pc = 0x0100;
    cpu.sp = 0x7F00;
    let mut exec = Executive::new(cpu, machine);
    exec.run_cycles(2_000_000);

    // It ran (cycles advanced) and did not terminally halt.
    assert!(!exec.is_terminally_halted());
    // The bottom region stays blank (no media to draw).
    let base = cfg.display_base;
    let bottom = base + 46 * 80;
    assert_eq!(exec.machine.read(bottom), 0, "nothing drawn when no disk is mounted");
}
