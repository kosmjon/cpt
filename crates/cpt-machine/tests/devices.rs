//! Tests for the Device trait, port routing, the executive loop, and interrupt delivery.

use cpt_machine::{Device, Executive, Machine, MachineConfig};
use i8080_cpu::{Bus, Cpu};
use std::any::Any;

/// A trivial latch device: `OUT` stores a byte, `IN` returns it. Claims two ports.
struct Latch {
    ports: [u8; 2],
    last: u8,
}
impl Latch {
    fn new(p0: u8, p1: u8) -> Self {
        Latch { ports: [p0, p1], last: 0 }
    }
}
impl Device for Latch {
    fn ports(&self) -> &[u8] {
        &self.ports
    }
    fn io_in(&mut self, _port: u8) -> u8 {
        self.last
    }
    fn io_out(&mut self, _port: u8, val: u8) {
        self.last = val;
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A timer that raises RST `vector` once after `period` cycles have elapsed.
struct Timer {
    period: u32,
    elapsed: u32,
    vector: u8,
    pending: bool,
    fired: bool,
}
impl Timer {
    fn new(period: u32, vector: u8) -> Self {
        Timer { period, elapsed: 0, vector, pending: false, fired: false }
    }
}
impl Device for Timer {
    fn ports(&self) -> &[u8] {
        &[]
    }
    fn io_in(&mut self, _: u8) -> u8 {
        0
    }
    fn io_out(&mut self, _: u8, _: u8) {}
    fn tick(&mut self, cycles: u32) {
        if self.fired {
            return;
        }
        self.elapsed += cycles;
        if self.elapsed >= self.period {
            self.pending = true;
        }
    }
    fn take_irq(&mut self) -> Option<u8> {
        if self.pending {
            self.pending = false;
            self.fired = true;
            Some(self.vector)
        } else {
            None
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn machine() -> Machine {
    Machine::new(MachineConfig::default())
}

#[test]
fn device_port_routing() {
    let mut m = machine();
    m.attach_device(Box::new(Latch::new(0x10, 0x11)));
    m.io_out(0x10, 0x5A);
    assert_eq!(m.io_in(0x10), 0x5A);
    assert_eq!(m.io_in(0x11), 0x5A); // same latch, both ports
    // A port no device owns is still recorded as unmapped.
    m.io_out(0x20, 0x01);
    assert_eq!(m.unmapped_io, vec![(0x20, 0x01)]);
}

#[test]
#[should_panic(expected = "already claimed")]
fn overlapping_device_ports_panic() {
    let mut m = machine();
    m.attach_device(Box::new(Latch::new(0x10, 0x11)));
    m.attach_device(Box::new(Latch::new(0x11, 0x12))); // 0x11 collides
}

#[test]
#[should_panic(expected = "bank-select port")]
fn device_port_colliding_with_bank_select_panics() {
    let mut m = machine();
    let bs = m.config().bank_select_port;
    m.attach_device(Box::new(Latch::new(bs, 0x12)));
}

#[test]
fn tick_accumulates_cycles_and_raises_irq() {
    let mut m = machine();
    m.attach_device(Box::new(Timer::new(100, 7)));
    // Before enough cycles, no IRQ.
    m.tick_devices(40);
    assert_eq!(m.poll_irq(), None);
    m.tick_devices(40);
    assert_eq!(m.poll_irq(), None);
    // Crossing the period raises the IRQ exactly once.
    m.tick_devices(40); // total 120 >= 100
    assert_eq!(m.poll_irq(), Some(7));
    assert_eq!(m.poll_irq(), None, "IRQ consumed by take_irq");
}

#[test]
fn executive_runs_to_terminal_halt() {
    // Bank-0 program: NOP ; NOP ; HLT (with interrupts disabled => terminal).
    let mut m = machine();
    m.mem.load_bank0(0x0000, &[0x00, 0x00, 0x76]);
    let mut exec = Executive::new(Cpu::new(), m);
    let ran = exec.run_until_halt(1000);
    assert!(exec.is_terminally_halted());
    assert!(ran >= 4 + 4 + 7); // two NOPs + HLT cycles at minimum
}

#[test]
fn interrupt_wakes_halted_cpu() {
    // Program: EI ; HLT ; (woken into) at 0x0038 (RST 7): MVI A,0xAB ; HLT
    // The timer fires an RST 7 shortly after the HLT, which should vector the CPU to 0x0038.
    let mut m = machine();
    m.mem.load_bank0(0x0000, &[0xFB, 0x76]); // EI ; HLT
    m.mem.load_bank0(0x0038, &[0x3E, 0xAB, 0x76]); // RST 7 handler: MVI A,0xAB ; HLT
    m.attach_device(Box::new(Timer::new(8, 7))); // fires after a few cycles

    let mut cpu = Cpu::new();
    cpu.sp = 0x7000; // somewhere in bank 0 RAM for the interrupt to push the return addr
    let mut exec = Executive::new(cpu, m);

    exec.run_until_halt(10_000);

    // After the interrupt handler ran, A holds 0xAB and the CPU is terminally halted (the
    // handler's HLT runs with interrupts still disabled after accept).
    assert_eq!(exec.cpu.a, 0xAB, "RST 7 handler should have executed");
    assert!(exec.is_terminally_halted());
}

#[test]
fn device_mut_downcast() {
    let mut m = machine();
    m.attach_device(Box::new(Latch::new(0x10, 0x11)));
    let dev = m.device_mut(0).unwrap();
    let latch = dev.as_any_mut().downcast_mut::<Latch>().unwrap();
    latch.last = 0x99;
    assert_eq!(m.io_in(0x10), 0x99);
}
