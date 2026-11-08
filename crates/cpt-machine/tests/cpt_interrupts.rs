//! The real CPT 8253 interval timer (1CH-1FH) and interrupt acknowledge port (00H).

use cpt_machine::{Device, InterruptAck, Pit8253};

/// Program counter `n` with `count`, mode 2, 16-bit binary, as the CPT system does
/// (control word 74H for counter 1, B4H for counter 2).
fn program(p: &mut Pit8253, n: u8, count: u16) {
    p.io_out(0x1F, n << 6 | 0x30 | 0x04);
    p.io_out(0x1C + n, count as u8);
    p.io_out(0x1C + n, (count >> 8) as u8);
}

#[test]
fn the_systems_own_programming_gives_125_and_100_hz() {
    let mut p = Pit8253::cpt();
    p.io_out(0x1F, 0x74);
    p.io_out(0x1D, 0x80);
    p.io_out(0x1D, 0x3E);
    p.io_out(0x1F, 0xB4);
    p.io_out(0x1E, 0x20);
    p.io_out(0x1E, 0x4E);
    assert_eq!(p.reload(1), 16000);
    assert_eq!(p.reload(2), 20000);
    // One second at 2 MHz, in instruction-sized steps.
    for _ in 0..(2_000_000 / 10) {
        p.tick(10);
    }
    assert_eq!(p.pulses, [0, 125, 100]);
}

#[test]
fn counter_outputs_raise_their_rst_vectors() {
    let mut p = Pit8253::cpt(); // counter 1 -> RST 5, counter 2 -> RST 7
    program(&mut p, 1, 100);
    program(&mut p, 2, 100);
    p.tick(99);
    assert_eq!(p.take_irq(), None);
    p.tick(1);
    assert_eq!(p.take_irq(), Some(7), "the higher RST goes first");
    assert_eq!(p.take_irq(), Some(5));
    assert_eq!(p.take_irq(), None);
}

#[test]
fn an_unprogrammed_or_unwired_counter_is_silent() {
    let mut p = Pit8253::cpt();
    program(&mut p, 0, 10); // counter 0 has no interrupt wired
    p.tick(1000);
    assert_eq!(p.take_irq(), None);
    assert!(p.pulses[0] > 0, "it still counts");
}

#[test]
fn a_new_control_word_stops_the_counter_until_it_is_loaded() {
    let mut p = Pit8253::cpt();
    program(&mut p, 2, 50);
    p.io_out(0x1F, 0xB4); // reprogram: stopped until the count arrives
    p.tick(1000);
    assert_eq!(p.take_irq(), None);
    p.io_out(0x1E, 50);
    p.io_out(0x1E, 0);
    p.tick(50);
    assert_eq!(p.take_irq(), Some(7));
}

#[test]
fn acknowledges_are_recorded() {
    let mut a = InterruptAck::new(0x00);
    assert_eq!(a.ports(), &[0x00]);
    a.io_out(0x00, 0x80);
    a.io_out(0x00, 0x80);
    a.io_out(0x00, 0x20);
    assert_eq!(a.acks, vec![(0x80, 2), (0x20, 1)]);
}
