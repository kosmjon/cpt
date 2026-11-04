//! Tests for the CPT banked memory and bank-select port (Req 2).

use cpt_machine::{Machine, MachineConfig};
use i8080_cpu::{Bus, Cpu};

fn machine() -> Machine {
    Machine::new(MachineConfig::default())
}

#[test]
fn bank0_is_resident_and_independent_of_selection() {
    let mut m = machine();
    // Write into bank 0.
    m.write(0x0000, 0x11);
    m.write(0x7FFF, 0x22);
    // Switching bank-1 blocks must not affect bank 0.
    m.mem.select_block(3);
    assert_eq!(m.read(0x0000), 0x11);
    assert_eq!(m.read(0x7FFF), 0x22);
}

#[test]
fn bank1_writes_target_selected_block() {
    let mut m = machine();
    m.mem.select_block(0);
    m.write(0x8000, 0xAA); // first byte of bank 1
    m.write(0xFFFF, 0xBB); // last byte of bank 1
    assert_eq!(m.read(0x8000), 0xAA);
    assert_eq!(m.read(0xFFFF), 0xBB);
    // The bytes landed in block 0, at offsets 0 and 0x7FFF.
    assert_eq!(m.mem.peek_block(0, 0x0000), 0xAA);
    assert_eq!(m.mem.peek_block(0, 0x7FFF), 0xBB);
}

#[test]
fn deselected_blocks_persist() {
    // The long-call mechanism depends on this: write block 1, swap to block 2, swap back,
    // and block 1's contents must be intact.
    let mut m = machine();

    m.mem.select_block(1);
    m.write(0x8000, 0x42);
    m.write(0x9000, 0x99);

    m.mem.select_block(2);
    // Block 2 is a different, zeroed block.
    assert_eq!(m.read(0x8000), 0x00);
    m.write(0x8000, 0x7E); // scribble on block 2

    m.mem.select_block(1);
    // Block 1 retained its original contents across the detour through block 2.
    assert_eq!(m.read(0x8000), 0x42);
    assert_eq!(m.read(0x9000), 0x99);

    // And block 2 kept its own write too.
    m.mem.select_block(2);
    assert_eq!(m.read(0x8000), 0x7E);
}

#[test]
fn bank_select_via_io_port() {
    let mut m = machine();
    let port = m.config().bank_select_port;

    // Put a distinct marker at offset 0 of several blocks.
    for b in 0..m.mem.block_count() {
        m.mem.load_block(b, 0x0000, &[0xB0 + b as u8]);
    }

    // Writing the port selects the block whose index is in the low 3 bits.
    m.io_out(port, 5);
    assert_eq!(m.selected_block(), 5);
    assert_eq!(m.read(0x8000), 0xB0 + 5);

    m.io_out(port, 2);
    assert_eq!(m.selected_block(), 2);
    assert_eq!(m.read(0x8000), 0xB0 + 2);
}

#[test]
fn bank_select_encoding_masks_high_bits() {
    let mut m = machine();
    let port = m.config().bank_select_port;
    // Default encoding is low 3 bits; 0xF5 -> low 3 bits = 0b101 = 5.
    m.io_out(port, 0xF5);
    assert_eq!(m.selected_block(), 5);
}

#[test]
fn unmapped_io_is_recorded_not_crashed() {
    let mut m = machine();
    m.io_out(0x99, 0xAB); // some port no device owns yet
    assert_eq!(m.unmapped_io, vec![(0x99, 0xAB)]);
    assert_eq!(m.io_in(0x99), 0); // unmapped reads are benign
}

/// End-to-end: the CPU executes an OUT to the bank-select port and the window remaps, so code
/// running from bank 0 can swap bank 1 — exactly how the long-call handler operates.
#[test]
fn cpu_can_switch_banks_with_out() {
    let mut m = machine();
    let port = m.config().bank_select_port;

    // Mark block 0 and block 1 distinctly at 0x8000.
    m.mem.load_block(0, 0x0000, &[0xC0]);
    m.mem.load_block(1, 0x0000, &[0xC1]);

    // Program in bank 0: MVI A,1 ; OUT <port> ; LDA 8000 ; HLT
    //   3E 01        MVI A,1
    //   D3 <port>    OUT port
    //   3A 00 80     LDA 0x8000
    //   76           HLT
    m.mem.load_bank0(0x0000, &[0x3E, 0x01, 0xD3, port, 0x3A, 0x00, 0x80, 0x76]);

    let mut cpu = Cpu::new();
    // Run until HLT.
    for _ in 0..10 {
        if cpu.halted {
            break;
        }
        cpu.step(&mut m);
    }
    assert_eq!(cpu.a, 0xC1, "after selecting block 1, LDA 0x8000 should read block 1's byte");
    assert_eq!(m.selected_block(), 1);
}

#[test]
fn initial_block_selection() {
    let mut cfg = MachineConfig::default();
    cfg.initial_block = 3;
    let m = Machine::new(cfg);
    assert_eq!(m.selected_block(), 3);
}
