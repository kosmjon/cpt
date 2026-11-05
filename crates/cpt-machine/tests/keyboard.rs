//! Tests for the keyboard device: queue/status/data ports, N-key rollover make/break, and
//! interrupt-driven delivery to the CPU.

use cpt_machine::keyboard::BREAK_FLAG;
use cpt_machine::{Device, Executive, KeyboardDevice, Machine, MachineConfig};
use i8080_cpu::{Bus, Cpu};

#[test]
fn status_and_data_ports() {
    let base = 0x60;
    let mut kbd = KeyboardDevice::polled(base);
    // Empty: status bit clear, data reads 0.
    assert_eq!(kbd.io_in(base) & 1, 0);
    assert_eq!(kbd.io_in(base + 1), 0);

    kbd.push_key(b'A');
    assert_eq!(kbd.io_in(base) & 1, 1, "status shows a key available");
    assert_eq!(kbd.io_in(base + 1), b'A', "data returns the queued code");
    assert_eq!(kbd.io_in(base) & 1, 0, "queue empty again after the read");
}

#[test]
fn fifo_order_and_nkey_make_break() {
    let base = 0x60;
    let mut kbd = KeyboardDevice::polled(base);
    // Press A, press B, release A, release B — N-key rollover reports each independently.
    kbd.push_key(b'A');
    kbd.push_key(b'B');
    kbd.push_key_up(b'A');
    kbd.push_key_up(b'B');
    assert_eq!(kbd.io_in(base + 1), b'A'); // make A
    assert_eq!(kbd.io_in(base + 1), b'B'); // make B
    assert_eq!(kbd.io_in(base + 1), b'A' | BREAK_FLAG); // break A
    assert_eq!(kbd.io_in(base + 1), b'B' | BREAK_FLAG); // break B
}

#[test]
fn interrupt_raised_on_key() {
    let base = 0x60;
    let mut kbd = KeyboardDevice::new(base, 2); // interrupt-driven, RST 2
    assert_eq!(kbd.take_irq(), None, "no interrupt while idle");
    kbd.push_key(b'X');
    assert_eq!(kbd.take_irq(), Some(2), "key press raises the configured vector");
    // Not re-raised until a new key arrives (even though the key is still queued).
    assert_eq!(kbd.take_irq(), None);
    kbd.push_key(b'Y');
    assert_eq!(kbd.take_irq(), Some(2));
}

/// End to end: a key event interrupts the CPU, whose RST-2 handler reads the key from the data
/// port and stores it. Proves the interrupt-driven keyboard path through the executive.
#[test]
fn cpu_reads_key_via_interrupt() {
    let cfg = MachineConfig::default();
    let base = cfg.keyboard_base_port;
    let vector = cfg.keyboard_vector; // RST 2 -> 0x0010
    let mut m = Machine::new(cfg.clone());

    let kbd_dev = m.device_count();
    m.attach_device(Box::new(KeyboardDevice::new(base, vector)));

    // Main program at 0x0000: EI ; loop: JMP loop
    m.mem.load_bank0(0x0000, &[0xFB, 0xC3, 0x01, 0x00]);
    // RST 2 handler at 0x0010: IN (base+1) ; STA 0x0200 ; EI ; RET
    let handler = (vector as u16) * 8;
    m.mem.load_bank0(handler, &[0xDB, base + 1, 0x32, 0x00, 0x02, 0xFB, 0xC9]);

    // Queue a key before running.
    m.device_mut(kbd_dev)
        .unwrap()
        .as_any_mut()
        .downcast_mut::<KeyboardDevice>()
        .unwrap()
        .push_key(b'Q');

    let mut cpu = Cpu::new();
    cpu.sp = 0x7000;
    let mut exec = Executive::new(cpu, m);
    // Run a bit; the keyboard IRQ should fire, the handler should read 'Q' into 0x0200.
    exec.run_cycles(500);

    assert_eq!(exec.machine.read(0x0200), b'Q', "handler should have read the key code");
}
