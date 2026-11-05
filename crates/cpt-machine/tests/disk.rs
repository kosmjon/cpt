//! Tests for the file-backed data disk: backing store, the controller protocol, persistence,
//! runtime mount/eject, and an A->B copy (the re-pagination scenario).

use cpt_machine::disk::{DiskImage, Geometry, Station};
use cpt_machine::{Device, DiskController, Machine, MachineConfig};

fn geom() -> Geometry {
    Geometry { block_size: 128, block_count: 16 }
}

// --- backing store ---

#[test]
fn block_read_write_roundtrip() {
    let mut img = DiskImage::blank(geom());
    let data: Vec<u8> = (0..128).map(|i| i as u8).collect();
    assert!(img.write_block(3, &data));
    let mut out = vec![0u8; 128];
    assert!(img.read_block(3, &mut out));
    assert_eq!(out, data);
}

#[test]
fn out_of_range_block_is_rejected() {
    let mut img = DiskImage::blank(geom());
    let data = vec![0u8; 128];
    assert!(!img.write_block(16, &data)); // only blocks 0..=15 exist
    let mut out = vec![0u8; 128];
    assert!(!img.read_block(16, &mut out));
}

#[test]
fn wrong_buffer_size_is_rejected() {
    let mut img = DiskImage::blank(geom());
    assert!(!img.write_block(0, &vec![0u8; 64])); // not a full block
}

#[test]
fn persistence_to_host_file() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("cpt_disk_test_{}.img", std::process::id()));
    let path = path.to_str().unwrap();

    // Write a block and flush, then drop the image.
    {
        let mut img = DiskImage::open_file(path, geom()).unwrap();
        let data: Vec<u8> = (0..128).map(|i| (255 - i) as u8).collect();
        img.write_block(5, &data);
        img.flush().unwrap();
    }
    // Reopen: the block should still be there.
    {
        let img = DiskImage::open_file(path, geom()).unwrap();
        let mut out = vec![0u8; 128];
        img.read_block(5, &mut out);
        assert_eq!(out[0], 255);
        assert_eq!(out[1], 254);
    }
    std::fs::remove_file(path).ok();
}

// --- controller protocol (driven directly) ---

/// Drive the placeholder protocol to write `block` then read it back on `station`.
fn ctrl_write_read(ctrl: &mut DiskController, base: u8, station: Station, block: u16, fill: u8) -> Vec<u8> {
    // select station
    ctrl.io_out(base, if station == Station::A { 0 } else { 1 });
    // block number
    ctrl.io_out(base + 1, (block & 0xFF) as u8);
    ctrl.io_out(base + 2, (block >> 8) as u8);
    // write command, then stream a full block of `fill`
    ctrl.io_out(base + 3, 2); // CMD_WRITE
    for _ in 0..128 {
        ctrl.io_out(base + 4, fill);
    }
    // now read it back
    ctrl.io_out(base + 3, 1); // CMD_READ
    (0..128).map(|_| ctrl.io_in(base + 4)).collect()
}

#[test]
fn controller_write_then_read() {
    let base = 0x50;
    let mut ctrl = DiskController::new(base, geom());
    ctrl.mount(Station::A, DiskImage::blank(geom()));

    let got = ctrl_write_read(&mut ctrl, base, Station::A, 2, 0xAB);
    assert!(got.iter().all(|&b| b == 0xAB));
    // STATUS reports OK (bit 0).
    assert_eq!(ctrl.io_in(base + 3) & 0x01, 0x01);
}

#[test]
fn controller_reports_no_media() {
    let base = 0x50;
    let mut ctrl = DiskController::new(base, geom());
    // Station B is empty; a read should report no-media (bit 7).
    ctrl.io_out(base, 1); // select B
    ctrl.io_out(base + 3, 1); // CMD_READ
    assert_eq!(ctrl.io_in(base + 3) & 0x80, 0x80);
}

#[test]
fn stations_are_independent() {
    let base = 0x50;
    let mut ctrl = DiskController::new(base, geom());
    ctrl.mount(Station::A, DiskImage::blank(geom()));
    ctrl.mount(Station::B, DiskImage::blank(geom()));

    ctrl_write_read(&mut ctrl, base, Station::A, 0, 0x11);
    ctrl_write_read(&mut ctrl, base, Station::B, 0, 0x22);

    // Re-read A's block 0: still 0x11 (B's write didn't touch it).
    ctrl.io_out(base, 0);
    ctrl.io_out(base + 1, 0);
    ctrl.io_out(base + 2, 0);
    ctrl.io_out(base + 3, 1);
    let a0: Vec<u8> = (0..128).map(|_| ctrl.io_in(base + 4)).collect();
    assert!(a0.iter().all(|&b| b == 0x11));
}

#[test]
fn activity_led_lights_on_access_and_decays() {
    use cpt_machine::disk::Station;
    let base = 0x50;
    let mut ctrl = DiskController::new(base, geom());
    ctrl.mount(Station::A, DiskImage::blank(geom()));

    assert!(!ctrl.is_active(Station::A), "idle at start");

    // A read command marks the station active.
    ctrl.io_out(base, 0); // select A
    ctrl.io_out(base + 3, 1); // CMD_READ
    assert!(ctrl.is_active(Station::A), "active right after a read");
    assert!(!ctrl.is_active(Station::B), "B untouched");

    // Ticking enough cycles lets the activity decay (LED turns off).
    ctrl.tick(200_000); // > ACTIVITY_HOLD_CYCLES
    assert!(!ctrl.is_active(Station::A), "LED off after the hold window elapses");
}

#[test]
fn runtime_mount_and_eject() {
    let base = 0x50;
    let mut ctrl = DiskController::new(base, geom());
    assert!(!ctrl.is_mounted(Station::A));
    ctrl.mount(Station::A, DiskImage::blank(geom()));
    assert!(ctrl.is_mounted(Station::A));
    let img = ctrl.eject(Station::A);
    assert!(img.is_some());
    assert!(!ctrl.is_mounted(Station::A));
}

// --- end to end: the controller attached to the Machine, driven by the CPU ---

#[test]
fn cpu_drives_disk_via_ports() {
    let cfg = MachineConfig::default();
    let base = cfg.disk_base_port;
    let mut m = Machine::new(cfg.clone());

    let mut ctrl = DiskController::new(base, geom());
    ctrl.mount(Station::A, DiskImage::blank(geom()));
    m.attach_device(Box::new(ctrl));

    // Program: select A, block 1, write a block of 0x5A, then flush. (Written as OUTs.)
    //   For brevity we write the block via a loop in 8080:
    //   MVI A,0 ; OUT base          (station A)
    //   MVI A,1 ; OUT base+1        (block lo = 1)
    //   MVI A,0 ; OUT base+2        (block hi = 0)
    //   MVI A,2 ; OUT base+3        (CMD_WRITE)
    //   MVI A,5A ; MVI B,80 ; loop: OUT base+4 ; DCR B ; JNZ loop
    //   HLT
    let b4 = base + 4;
    let prog: Vec<u8> = vec![
        0x3E, 0x00, 0xD3, base,
        0x3E, 0x01, 0xD3, base + 1,
        0x3E, 0x00, 0xD3, base + 2,
        0x3E, 0x02, 0xD3, base + 3,
        0x3E, 0x5A,            // MVI A,0x5A
        0x06, 0x80,            // MVI B,128
        0xD3, b4,              // loop (0x14): OUT base+4
        0x05,                  // DCR B
        0xC2, 0x14, 0x00,      // JNZ 0x0014
        0x76,                  // HLT
    ];
    m.mem.load_bank0(0x0000, &prog);

    let mut cpu = i8080_cpu::Cpu::new();
    cpu.sp = 0x7F00;
    let mut exec = cpt_machine::Executive::new(cpu, m);
    exec.run_until_halt(100_000);
    assert!(exec.is_terminally_halted());

    // Verify the block landed in station A's image by reading it back through the controller.
    let ctrl = exec
        .machine
        .device_mut(0)
        .unwrap()
        .as_any_mut()
        .downcast_mut::<DiskController>()
        .unwrap();
    ctrl.io_out(base, 0);
    ctrl.io_out(base + 1, 1);
    ctrl.io_out(base + 2, 0);
    ctrl.io_out(base + 3, 1); // read
    let got: Vec<u8> = (0..128).map(|_| ctrl.io_in(base + 4)).collect();
    assert!(got.iter().all(|&x| x == 0x5A), "CPU-written block should read back as 0x5A");
}
