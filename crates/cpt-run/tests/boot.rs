//! End-to-end test of the boot path (CP/M-over-serial increment 3).
//!
//! Assembles the real MOL80 sources `cpm/boot.mol` (boot ROM) and `cpm/bootsector.mol`
//! with `mol80asm`, puts the boot sector in block 0 of a disk in drive 1, and runs the ROM on
//! the emulated machine. The ROM must read the block, jump to it, and the boot sector must
//! print its banner on the serial port. Exercises: assembler -> CPU -> disk controller ->
//! serial device.

use cpt_machine::disk::{DiskImage, Geometry, Station};
use cpt_machine::{DiskController, Executive, Machine, MachineConfig, SerialDevice};
use i8080_cpu::Cpu;

/// Assemble a repo-relative `.mol` file, failing the test with diagnostics on any error.
fn assemble_file(rel: &str) -> mol80asm::codegen::Assembly {
    let path = format!("{}/../../{}", env!("CARGO_MANIFEST_DIR"), rel);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path, e));
    let out = mol80asm::assemble_source(&path, &src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.expect("assembly should succeed")
}

/// A disk with the boot sector in block 0.
fn bootable_disk(geom: Geometry) -> DiskImage {
    let sector = assemble_file("cpm/bootsector.mol");
    assert_eq!(sector.origin, 0x0100, "boot sector is loaded at 0100H");
    assert!(
        sector.bytes.len() <= geom.block_size,
        "boot sector is {} bytes, must fit in one {}-byte block",
        sector.bytes.len(),
        geom.block_size
    );
    let mut block = vec![0u8; geom.block_size];
    block[..sector.bytes.len()].copy_from_slice(&sector.bytes);
    let mut img = DiskImage::blank(geom);
    assert!(img.write_block(0, &block));
    img
}

struct Rig {
    exec: Executive,
    disk_dev: usize,
    serial_dev: usize,
}

/// Machine with the boot ROM loaded at reset, a disk controller, and a serial port.
fn rig(disk_in_drive1: bool) -> Rig {
    let cfg = MachineConfig::default();
    let geom = Geometry::default();

    let rom = assemble_file("cpm/boot.mol");
    assert_eq!(rom.origin, 0);
    assert!(rom.bytes.len() < 0x100, "boot ROM must stay below the 0100H load address");

    let mut machine = Machine::new(cfg.clone());
    let mut ctrl = DiskController::new(cfg.disk_base_port, geom);
    if disk_in_drive1 {
        ctrl.mount(Station::A, bootable_disk(geom));
    }
    let disk_dev = machine.device_count();
    machine.attach_device(Box::new(ctrl));
    let serial_dev = machine.device_count();
    machine.attach_device(Box::new(SerialDevice::new(
        cfg.serial_base_port,
        Some(cfg.serial_vector),
    )));
    machine.mem.load_bank0(rom.origin, &rom.bytes);

    let mut cpu = Cpu::new();
    cpu.pc = rom.origin;
    Rig { exec: Executive::new(cpu, machine), disk_dev, serial_dev }
}

impl Rig {
    fn serial(&mut self) -> &mut SerialDevice {
        let i = self.serial_dev;
        self.exec
            .machine
            .device_mut(i)
            .unwrap()
            .as_any_mut()
            .downcast_mut::<SerialDevice>()
            .unwrap()
    }

    fn disk(&mut self) -> &mut DiskController {
        let i = self.disk_dev;
        self.exec
            .machine
            .device_mut(i)
            .unwrap()
            .as_any_mut()
            .downcast_mut::<DiskController>()
            .unwrap()
    }

    /// Everything the guest has transmitted on the serial port since the last call.
    fn take_tx(&mut self) -> String {
        let bytes = self.serial().take_tx();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

#[test]
fn rom_boots_the_disk_and_boot_sector_prints_banner() {
    let mut r = rig(true);
    r.exec.run_cycles(200_000);
    let tx = r.take_tx();
    assert!(
        tx.contains("BOOT SECTOR RUNNING (MOL80)"),
        "boot sector banner missing; serial output was {:?}",
        tx
    );
    assert!(tx.ends_with("\r\n"), "banner should end with CR LF, got {:?}", tx);
}

#[test]
fn boot_sector_echoes_typed_characters() {
    let mut r = rig(true);
    r.exec.run_cycles(200_000);
    let _banner = r.take_tx();

    r.serial().push_rx(b'z');
    r.exec.run_cycles(50_000);
    assert_eq!(r.take_tx(), "z", "the booted program should echo what it receives");
}

#[test]
fn rom_keeps_retrying_with_no_disk() {
    let mut r = rig(false);
    r.exec.run_cycles(200_000);
    assert_eq!(r.take_tx(), "", "nothing should boot without a disk");
    assert!(!r.exec.is_terminally_halted(), "the ROM retries rather than halting");
    assert!(r.exec.cpu.pc < 0x0100, "still looping inside the ROM, pc = {:#06X}", r.exec.cpu.pc);
}

#[test]
fn disk_inserted_later_is_booted() {
    // The real hardware retried 2-3 times a second, so a disk inserted after power-on gets
    // picked up. Run with no disk, then insert one and keep running.
    let mut r = rig(false);
    r.exec.run_cycles(100_000);
    assert_eq!(r.take_tx(), "");

    let geom = Geometry::default();
    r.disk().mount(Station::A, bootable_disk(geom));
    r.exec.run_cycles(200_000);
    assert!(r.take_tx().contains("BOOT SECTOR RUNNING (MOL80)"));
}
