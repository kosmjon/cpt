//! Memory watchpoints (`Machine::add_watch`): the debugging hook behind `cpt-run --watch`.

use cpt_machine::{Machine, MachineConfig, Watch};
use i8080_cpu::Bus;

fn machine() -> Machine {
    Machine::new(MachineConfig::cpt_real())
}

fn watch(lo: u16, hi: u16, reads: bool, writes: bool, block: Option<usize>) -> Watch {
    Watch { lo, hi, reads, writes, block }
}

#[test]
fn nothing_is_logged_without_a_watch() {
    let mut m = machine();
    m.write(0x3200, 0x55);
    let _ = m.read(0x3200);
    assert!(m.watch_hits.is_empty());
}

#[test]
fn reads_and_writes_in_the_range_are_logged_in_order() {
    let mut m = machine();
    m.add_watch(watch(0x3200, 0x32FF, true, true, None));
    m.write(0x3201, 0xAB);
    assert_eq!(m.read(0x3201), 0xAB);
    m.write(0x3300, 0x01); // outside the range
    let _ = m.read(0x31FF); // outside the range
    let hits: Vec<_> = m.watch_hits.iter().map(|h| (h.addr, h.write, h.value, h.dma)).collect();
    assert_eq!(hits, vec![(0x3201, true, 0xAB, false), (0x3201, false, 0xAB, false)]);
}

#[test]
fn the_read_and_write_filters_work() {
    let mut m = machine();
    m.add_watch(watch(0x1000, 0x1000, false, true, None)); // writes only
    m.add_watch(watch(0x2000, 0x2000, true, false, None)); // reads only
    m.write(0x1000, 1);
    let _ = m.read(0x1000);
    m.write(0x2000, 2);
    let _ = m.read(0x2000);
    let hits: Vec<_> = m.watch_hits.iter().map(|h| (h.addr, h.write)).collect();
    assert_eq!(hits, vec![(0x1000, true), (0x2000, false)]);
}

#[test]
fn a_block_limits_a_watch_at_8000h_and_up() {
    let mut m = machine();
    m.add_watch(watch(0x8000, 0x80FF, false, true, Some(1)));
    m.mem.select_block(0);
    m.write(0x8010, 0xAA); // block 0: not watched
    m.mem.select_block(1);
    m.write(0x8010, 0xBB); // block 1: watched
    assert_eq!(m.watch_hits.len(), 1);
    assert_eq!((m.watch_hits[0].addr, m.watch_hits[0].value, m.watch_hits[0].block), (0x8010, 0xBB, 1));
    // Bank 0 (below 8000H) ignores the block.
    m.add_watch(watch(0x0100, 0x0100, false, true, Some(2)));
    m.write(0x0100, 7);
    assert_eq!(m.watch_hits.len(), 2);
}

#[test]
fn the_host_can_take_the_log() {
    let mut m = machine();
    m.add_watch(watch(0x4000, 0x4000, true, true, None));
    m.write(0x4000, 9);
    let taken = std::mem::take(&mut m.watch_hits);
    assert_eq!(taken.len(), 1);
    assert!(m.watch_hits.is_empty());
    m.write(0x4000, 10);
    assert_eq!(m.watch_hits.len(), 1);
}
