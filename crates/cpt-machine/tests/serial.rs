//! Tests for the bidirectional serial (USART) device, loopback transport, and the pump.

use cpt_machine::serial::{ST_RX_READY, ST_TX_READY};
use cpt_machine::{
    serial_pump, Device, Executive, LoopbackTransport, Machine, MachineConfig, SerialDevice,
};
use i8080_cpu::{Bus, Cpu};

#[test]
fn tx_queues_and_status_tx_ready() {
    let base = 0x70;
    let mut s = SerialDevice::new(base, None);
    // TX always ready; RX not ready when empty.
    assert_eq!(s.io_in(base + 1) & ST_TX_READY, ST_TX_READY);
    assert_eq!(s.io_in(base + 1) & ST_RX_READY, 0);
    // Writing the data port queues bytes for transmission.
    s.io_out(base, b'H');
    s.io_out(base, b'i');
    assert_eq!(s.tx_pending(), 2);
    assert_eq!(s.take_tx(), vec![b'H', b'i']);
    assert_eq!(s.tx_pending(), 0);
}

#[test]
fn rx_sets_status_and_reads_fifo() {
    let base = 0x70;
    let mut s = SerialDevice::new(base, None);
    s.push_rx(b'X');
    s.push_rx(b'Y');
    assert_eq!(s.io_in(base + 1) & ST_RX_READY, ST_RX_READY);
    assert_eq!(s.io_in(base), b'X'); // DATA read pops FIFO
    assert_eq!(s.io_in(base), b'Y');
    assert_eq!(s.io_in(base + 1) & ST_RX_READY, 0); // drained
    assert_eq!(s.io_in(base), 0); // empty reads 0
}

#[test]
fn loopback_round_trip_via_pump() {
    let base = 0x70;
    let mut s = SerialDevice::new(base, None);
    let mut t = LoopbackTransport::new();

    // CPU writes two bytes to transmit.
    s.io_out(base, b'A');
    s.io_out(base, b'B');
    // Pump: TX goes to loopback, which echoes it back as RX on the next pump.
    serial_pump(&mut s, &mut t); // send A,B -> loopback buffer
    serial_pump(&mut s, &mut t); // recv A,B -> device RX
    assert_eq!(s.rx_pending(), 2);
    assert_eq!(s.io_in(base), b'A');
    assert_eq!(s.io_in(base), b'B');
}

#[test]
fn receive_interrupt() {
    let base = 0x70;
    let mut s = SerialDevice::new(base, Some(3));
    assert_eq!(s.take_irq(), None, "idle: no interrupt");
    s.push_rx(b'Z');
    assert_eq!(s.take_irq(), Some(3), "received byte raises the vector");
    assert_eq!(s.take_irq(), None, "not re-raised until a new byte");
}

/// End to end: a received byte interrupts the CPU; the RST-3 handler reads it from the data
/// port and stores it.
#[test]
fn cpu_receives_byte_via_interrupt() {
    let cfg = MachineConfig::default();
    let base = cfg.serial_base_port;
    let vector = cfg.serial_vector; // RST 3 -> 0x0018
    let mut m = Machine::new(cfg.clone());

    let dev = m.device_count();
    m.attach_device(Box::new(SerialDevice::new(base, Some(vector))));

    // Main: EI ; loop: JMP loop
    m.mem.load_bank0(0x0000, &[0xFB, 0xC3, 0x01, 0x00]);
    // RST 3 handler at 0x0018: IN (base) ; STA 0x0200 ; EI ; RET
    let handler = (vector as u16) * 8;
    m.mem.load_bank0(handler, &[0xDB, base, 0x32, 0x00, 0x02, 0xFB, 0xC9]);

    // A byte arrives from the peer.
    m.device_mut(dev)
        .unwrap()
        .as_any_mut()
        .downcast_mut::<SerialDevice>()
        .unwrap()
        .push_rx(b'!');

    let mut cpu = Cpu::new();
    cpu.sp = 0x7000;
    let mut exec = Executive::new(cpu, m);
    exec.run_cycles(500);

    assert_eq!(exec.machine.read(0x0200), b'!', "handler should have read the serial byte");
}
