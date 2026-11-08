//! The real CPT floppy hardware model: 8257 DMA channel 0, FD1771 registers, and the
//! CPU-driven three-phase head stepper on port 23H.

use cpt_machine::cpt_floppy::{cpt_geometry, CptFloppy, SECTORS, SECTOR_SIZE};
use cpt_machine::{Device, DiskImage, DmaRequest, Machine, MachineConfig};
use i8080_cpu::Bus;

/// An image whose every byte is (track * 16 + sector) & 0xFF, so a sector is recognizable.
fn marked_image() -> DiskImage {
    let mut img = DiskImage::blank(cpt_geometry());
    for lin in 0..cpt_geometry().block_count {
        assert!(img.write_block(lin, &vec![(lin & 0xFF) as u8; SECTOR_SIZE]));
    }
    img
}

fn set_dma(f: &mut CptFloppy, addr: u16, count_minus_1: u16) {
    f.io_out(0x18, 0x00); // mode set also resets the low/high flip-flop
    f.io_out(0x10, addr as u8);
    f.io_out(0x10, (addr >> 8) as u8);
    f.io_out(0x11, count_minus_1 as u8);
    f.io_out(0x11, (0x4000 | count_minus_1 >> 8) as u8);
    f.io_out(0x18, 0x71);
}

/// Step the head in by `n` tracks through the three-phase sequence 1 -> 2 -> 4 -> 1.
fn step_in(f: &mut CptFloppy, phase: &mut u8, n: usize) {
    for _ in 0..n {
        *phase = if *phase == 4 { 1 } else { *phase << 1 };
        f.io_out(0x23, *phase | 0x08);
    }
}

#[test]
fn claims_the_real_ports() {
    let f = CptFloppy::new();
    for p in [0x10, 0x11, 0x18, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27] {
        assert!(f.ports().contains(&p), "port {:02X}", p);
    }
}

#[test]
fn the_stepper_moves_the_head_and_track_0_is_reported() {
    let mut f = CptFloppy::new();
    f.mount(0, marked_image());
    assert_eq!(f.io_in(0x23) & 0x04, 0x04, "head starts at track 0");
    let mut phase = 1;
    step_in(&mut f, &mut phase, 5);
    assert_eq!(f.head_track(), 5);
    assert_eq!(f.io_in(0x23) & 0x04, 0, "not at track 0 any more");
    // Step out: 4 -> 2 -> 1 -> 4 ...
    for _ in 0..5 {
        phase = if phase == 1 { 4 } else { phase >> 1 };
        f.io_out(0x23, phase | 0x08);
    }
    assert_eq!(f.head_track(), 0);
    // Repeating the same phase does not move the head.
    f.io_out(0x23, phase | 0x08);
    assert_eq!(f.head_track(), 0);
}

#[test]
fn read_sector_dmas_the_sector_and_raises_the_request_bit() {
    let mut f = CptFloppy::new();
    f.mount(0, marked_image());
    let mut phase = 1;
    step_in(&mut f, &mut phase, 3);
    set_dma(&mut f, 0x4000, 0xFF);
    f.io_out(0x25, 3); // track register
    f.io_out(0x26, 7); // sector register
    assert_eq!(f.io_in(0x23) & 0x02, 0);
    f.io_out(0x24, 0x88); // read sector
    assert_eq!(f.io_in(0x23) & 0x02, 0x02, "interrupt request: done");
    match f.take_dma() {
        Some(DmaRequest::WriteMemory { addr, data }) => {
            assert_eq!(addr, 0x4000);
            assert_eq!(data.len(), 256);
            assert!(data.iter().all(|&b| b == (3 * SECTORS + 7) as u8));
        }
        other => panic!("expected a DMA write, got {:?}", other),
    }
    assert_eq!(f.io_in(0x24), 0, "status OK");
    assert_eq!(f.io_in(0x23) & 0x02, 0, "reading status clears the request");
}

#[test]
fn reading_with_the_head_on_the_wrong_track_is_record_not_found() {
    let mut f = CptFloppy::new();
    f.mount(0, marked_image());
    set_dma(&mut f, 0x4000, 0xFF);
    f.io_out(0x25, 9); // the controller looks for track 9, but the head is on track 0
    f.io_out(0x26, 0);
    f.io_out(0x24, 0x88);
    assert_eq!(f.io_in(0x24) & 0x10, 0x10);
    assert!(f.take_dma().is_none());
}

#[test]
fn no_disk_is_not_ready() {
    let mut f = CptFloppy::new();
    f.io_out(0x24, 0x88);
    assert_eq!(f.io_in(0x24) & 0x80, 0x80);
}

#[test]
fn a_read_can_take_time() {
    let mut f = CptFloppy::new();
    f.mount(0, marked_image());
    f.op_delay_cycles = 1000;
    set_dma(&mut f, 0x4000, 0xFF);
    f.io_out(0x24, 0x88);
    assert_eq!(f.io_in(0x24) & 0x01, 0x01, "busy");
    assert!(f.take_dma().is_none(), "no data yet");
    f.tick(999);
    assert_eq!(f.io_in(0x23) & 0x02, 0);
    f.tick(1);
    assert_eq!(f.io_in(0x23) & 0x02, 0x02, "done after the delay");
    assert!(f.take_dma().is_some());
}

#[test]
fn write_sector_takes_its_data_from_memory() {
    let mut f = CptFloppy::new();
    f.mount(0, marked_image());
    set_dma(&mut f, 0x5000, 0xFF);
    f.io_out(0x25, 0);
    f.io_out(0x26, 2);
    f.io_out(0x24, 0xA8);
    match f.take_dma() {
        Some(DmaRequest::ReadMemory { addr, len }) => {
            assert_eq!((addr, len), (0x5000, 256));
            f.dma_read_done(&vec![0x5A; 256]);
        }
        other => panic!("expected a DMA read, got {:?}", other),
    }
    let mut buf = vec![0; 256];
    f.image(0).unwrap().read_block(2, &mut buf);
    assert!(buf.iter().all(|&b| b == 0x5A));
}

/// End to end through the machine: the DMA lands in memory before the CPU sees "done".
#[test]
fn the_machine_carries_out_the_dma() {
    let mut m = Machine::new(MachineConfig::cpt_real());
    let mut f = CptFloppy::new();
    f.mount(0, marked_image());
    m.attach_device(Box::new(f));
    // Program the DMA and read track 0 sector 5 into 3000H, as the CPU would.
    for (p, v) in [(0x18, 0x00), (0x10, 0x00), (0x10, 0x30), (0x11, 0xFF), (0x11, 0x40), (0x18, 0x71),
                   (0x25, 0), (0x26, 5), (0x24, 0x88)] {
        m.io_out(p, v);
    }
    assert_eq!(m.io_in(0x23) & 0x02, 0x02);
    assert_eq!(m.read(0x3000), 5);
    assert_eq!(m.read(0x30FF), 5);
}

#[test]
fn the_real_profile_selects_banks_on_port_03() {
    let mut m = Machine::new(MachineConfig::cpt_real());
    m.io_out(0x03, 2);
    assert_eq!(m.selected_block(), 2);
}

/// The main system reads interrupt-driven: INTRQ raises RST 3 once per completed command. A
/// polled reader (the boot loader) that reads status first clears INTRQ, and with it the
/// request, so it never sees a stray interrupt later.
#[test]
fn intrq_raises_rst3_once_per_command_unless_status_is_polled_first() {
    let mut f = CptFloppy::new();
    f.mount(0, marked_image());
    set_dma(&mut f, 0x4000, 0xFF);
    f.io_out(0x26, 1);
    f.io_out(0x24, 0x88);
    while f.take_dma().is_some() {}
    assert_eq!(f.take_irq(), Some(3));
    assert_eq!(f.take_irq(), None, "one request per command");

    // Polled: the status read clears INTRQ before interrupts are taken.
    set_dma(&mut f, 0x4000, 0xFF);
    f.io_out(0x24, 0x88);
    while f.take_dma().is_some() {}
    assert_eq!(f.io_in(0x23) & 0x02, 0x02, "INTRQ visible on 23H bit 1");
    let _ = f.io_in(0x24);
    assert_eq!(f.take_irq(), None);
}

/// The system finds its drives by selecting 0-3 on 22H and reading status bit 7 (Not Ready,
/// routine 0C74H). Only drives with a disk are ready, and selects 2 and 3 are drives of their
/// own, not aliases of 0 and 1.
#[test]
fn not_ready_follows_the_selected_drive() {
    let mut f = CptFloppy::new();
    f.mount(0, marked_image());
    for (drive, ready) in [(0u8, true), (1, false), (2, false), (3, false)] {
        f.io_out(0x22, drive);
        let not_ready = f.io_in(0x24) & 0x80 != 0;
        assert_eq!(not_ready, !ready, "drive {}", drive);
    }
}
