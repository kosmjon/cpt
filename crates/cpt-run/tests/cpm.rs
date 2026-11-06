//! End-to-end CP/M 2.2 test (increment 4): boot ROM -> MOL80 loader -> stock CCP/BDOS ->
//! MOL80 BIOS, driven entirely over the serial port.
//!
//! The CCP and BDOS are built from the vendored Digital Research source by
//! `cpm/tools/asm80.py` (needs Python), so this test needs `cpm/build/ccp.bin` and
//! `cpm/build/bdos.bin`; run `cpm/build-cpm.ps1` first. If they are missing the tests print a
//! note and pass without running, so a plain `cargo test` works on a machine without Python.
use cpt_machine::disk::{DiskImage, Geometry, Station};
use cpt_machine::{DiskController, Executive, Machine, MachineConfig, SerialDevice};
use i8080_cpu::Cpu;

const BLOCK: usize = 128;

fn repo(rel: &str) -> String {
    format!("{}/../../{}", env!("CARGO_MANIFEST_DIR"), rel)
}

fn assemble_file(rel: &str) -> mol80asm::codegen::Assembly {
    let path = repo(rel);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {}", path, e));
    let out = mol80asm::assemble_source(&path, &src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.expect("assembly should succeed")
}

/// Build the system disk in memory, mirroring cpm/mkcpm.mjs. None if CCP/BDOS aren't built.
fn system_disk() -> Option<DiskImage> {
    let ccp = std::fs::read(repo("cpm/build/ccp.bin")).ok()?;
    let bdos = std::fs::read(repo("cpm/build/bdos.bin")).ok()?;
    let loader = assemble_file("cpm/loader.mol");
    let bios = assemble_file("cpm/bios.mol");
    assert_eq!(bios.origin, 0xFA00);
    let geom = Geometry::default();
    let mut raw = vec![0xE5u8; geom.block_size * geom.block_count];
    raw[..52 * BLOCK].fill(0);
    raw[..loader.bytes.len()].copy_from_slice(&loader.bytes);
    raw[BLOCK..BLOCK + ccp.len()].copy_from_slice(&ccp);
    raw[BLOCK + 0x800..BLOCK + 0x800 + bdos.len()].copy_from_slice(&bdos);
    let bios_at = BLOCK + 0x1600;
    raw[bios_at..bios_at + bios.bytes.len()].copy_from_slice(&bios.bytes);
    let mut img = DiskImage::blank(geom);
    for (i, chunk) in raw.chunks(BLOCK).enumerate() {
        assert!(img.write_block(i, chunk));
    }
    Some(img)
}

struct Rig {
    exec: Executive,
    serial_dev: usize,
}

/// A formatted, empty CP/M data disk: every byte 0E5H (an empty directory).
fn blank_data_disk() -> DiskImage {
    let geom = Geometry::default();
    let mut img = DiskImage::blank(geom);
    let block = vec![0xE5u8; geom.block_size];
    for i in 0..geom.block_count {
        assert!(img.write_block(i, &block));
    }
    img
}

fn rig(disk: DiskImage) -> Rig {
    rig_inner(disk, None)
}

fn rig_with_b(disk: DiskImage, b: DiskImage) -> Rig {
    rig_inner(disk, Some(b))
}

fn rig_inner(disk: DiskImage, disk_b: Option<DiskImage>) -> Rig {
    let cfg = MachineConfig::default();
    let rom = assemble_file("cpm/boot.mol");
    let mut machine = Machine::new(cfg.clone());
    let mut ctrl = DiskController::new(cfg.disk_base_port, Geometry::default());
    ctrl.mount(Station::A, disk);
    if let Some(b) = disk_b {
        ctrl.mount(Station::B, b);
    }
    machine.attach_device(Box::new(ctrl));
    let serial_dev = machine.device_count();
    machine.attach_device(Box::new(SerialDevice::new(cfg.serial_base_port, Some(cfg.serial_vector))));
    machine.mem.load_bank0(rom.origin, &rom.bytes);
    let mut cpu = Cpu::new();
    cpu.pc = rom.origin;
    Rig { exec: Executive::new(cpu, machine), serial_dev }
}

impl Rig {
    fn serial(&mut self) -> &mut SerialDevice {
        let i = self.serial_dev;
        self.exec.machine.device_mut(i).unwrap().as_any_mut().downcast_mut::<SerialDevice>().unwrap()
    }
    fn tx(&mut self) -> String {
        let b = self.serial().take_tx();
        String::from_utf8_lossy(&b).into_owned()
    }
    /// Type a line (one character at a time, as a terminal would) and return the response.
    fn type_line(&mut self, line: &str) -> String {
        for b in line.bytes().chain(std::iter::once(b'\r')) {
            self.serial().push_rx(b);
            self.exec.run_cycles(200_000);
        }
        self.exec.run_cycles(3_000_000);
        self.tx()
    }
}

#[test]
fn cpm_boots_to_the_a_prompt() {
    let Some(disk) = system_disk() else {
        eprintln!("skipping: run cpm/build-cpm.ps1 to build the CCP/BDOS first");
        return;
    };
    let mut r = rig(disk);
    r.exec.run_cycles(20_000_000);
    let tx = r.tx();
    assert!(tx.contains("CP/M 2.2 on CPT (MOL80 BIOS)"), "no sign-on; got {:?}", tx);
    assert!(tx.ends_with("A>"), "no A> prompt; got {:?}", tx);
}

#[test]
fn dir_on_an_empty_disk_says_no_file() {
    let Some(disk) = system_disk() else { return };
    let mut r = rig(disk);
    r.exec.run_cycles(20_000_000);
    let _ = r.tx();
    let out = r.type_line("DIR");
    assert!(out.contains("NO FILE"), "got {:?}", out);
}

#[test]
fn save_creates_a_file_that_dir_lists() {
    let Some(disk) = system_disk() else { return };
    let mut r = rig(disk);
    r.exec.run_cycles(20_000_000);
    let _ = r.tx();
    let _ = r.type_line("SAVE 1 T.COM");
    let out = r.type_line("DIR");
    assert!(out.contains("T        COM"), "T.COM missing from DIR; got {:?}", out);
}

#[test]
fn telnet_enter_as_cr_lf_gives_one_prompt() {
    // A Telnet client sends CR LF for Enter. The stray LF must not run a second command line.
    let Some(disk) = system_disk() else { return };
    let mut r = rig(disk);
    r.exec.run_cycles(20_000_000);
    let _ = r.tx();
    for b in *b"\r\n" {
        r.serial().push_rx(b);
        r.exec.run_cycles(200_000);
    }
    r.exec.run_cycles(3_000_000);
    let out = r.tx();
    assert_eq!(out.matches("A>").count(), 1, "expected exactly one prompt, got {:?}", out);
}

#[test]
fn typed_command_works_after_cr_nul() {
    let Some(disk) = system_disk() else { return };
    let mut r = rig(disk);
    r.exec.run_cycles(20_000_000);
    let _ = r.tx();
    for b in *b"DIR\r\0" {
        r.serial().push_rx(b);
        r.exec.run_cycles(200_000);
    }
    r.exec.run_cycles(3_000_000);
    assert!(r.tx().contains("NO FILE"));
}

/// Load the full disk built by `cpm/build-cpm.ps1` (with the utilities on it), if present.
fn built_image() -> Option<DiskImage> {
    let raw = std::fs::read(repo("cpm/build/cpm.img")).ok()?;
    let geom = Geometry::default();
    let mut img = DiskImage::blank(geom);
    for (i, chunk) in raw.chunks(BLOCK).enumerate() {
        assert!(img.write_block(i, chunk));
    }
    Some(img)
}

#[test]
fn utilities_on_the_disk_run() {
    let Some(disk) = built_image() else { return };
    let mut r = rig(disk);
    r.exec.run_cycles(20_000_000);
    let _ = r.tx();
    let dir = r.type_line("DIR");
    assert!(dir.contains("STAT") && dir.contains("PIP"), "utilities missing from DIR: {:?}", dir);
    let stat = r.type_line("STAT");
    assert!(stat.contains("A: R/W, Space:"), "STAT printed {:?}", stat);
}


#[test]
fn stat_reports_free_space() {
    let Some(disk) = built_image() else { return };
    let mut r = rig(disk);
    r.exec.run_cycles(20_000_000);
    let _ = r.tx();
    let out = r.type_line("STAT A:*.*");
    assert!(out.contains("R/W A:PIP.COM"), "got {:?}", out);
    assert!(out.contains("Bytes Remaining On A:"), "got {:?}", out);
}

#[test]
fn pip_copies_to_a_blank_data_disk_in_drive_2() {
    let Some(disk) = built_image() else { return };
    let mut r = rig_with_b(disk, blank_data_disk());
    r.exec.run_cycles(20_000_000);
    let _ = r.tx();
    assert!(r.type_line("DIR B:").contains("NO FILE"), "drive 2 should start empty");
    let _ = r.type_line("PIP B:=A:STAT.COM");
    let dir = r.type_line("DIR B:");
    assert!(dir.contains("B: STAT     COM"), "got {:?}", dir);
    // The copy must be byte-identical: run it from B:.
    let _ = r.type_line("B:");
    let stat = r.type_line("STAT");
    assert!(stat.contains("B: R/W, Space: 235k"), "got {:?}", stat);
}
