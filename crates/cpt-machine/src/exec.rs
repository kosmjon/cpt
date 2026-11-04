//! The executive: drives the CPU against the machine, ticking devices and delivering
//! interrupts.
//!
//! One `step` executes a single instruction, advances device time by the cycles it took, and
//! — if a device is requesting an interrupt and the CPU has interrupts enabled — services it.
//! This is the heartbeat the whole system runs on.

use crate::Machine;
use i8080_cpu::Cpu;

pub struct Executive {
    pub cpu: Cpu,
    pub machine: Machine,
}

impl Executive {
    pub fn new(cpu: Cpu, machine: Machine) -> Self {
        Executive { cpu, machine }
    }

    /// Execute one instruction, tick devices, and deliver a pending interrupt if enabled.
    /// Returns the cycles consumed by the instruction.
    pub fn step(&mut self) -> u32 {
        let cycles = self.cpu.step(&mut self.machine);
        self.machine.tick_devices(cycles);

        // Deliver at most one interrupt per step. `Cpu::interrupt` itself checks int_enabled
        // and returns false if disabled; it also wakes the CPU from HLT.
        if let Some(vector) = self.machine.poll_irq() {
            self.cpu.interrupt(vector, &mut self.machine);
        }
        cycles
    }

    /// True when the CPU is in a halt that nothing can wake: it executed HLT with interrupts
    /// disabled. (A HLT with interrupts enabled can still be resumed by a device IRQ, so it is
    /// not terminal.) This is the conventional "program is done" signal.
    pub fn is_terminally_halted(&self) -> bool {
        self.cpu.halted && !self.cpu.int_enabled
    }

    /// Run until the CPU is terminally halted, or until `max_cycles` is exceeded. Returns the
    /// total cycles run. The halt check does not consume device interrupts: a HLT with
    /// interrupts enabled keeps stepping, and `step()` itself delivers the IRQ that wakes it.
    pub fn run_until_halt(&mut self, max_cycles: u64) -> u64 {
        let start = self.cpu.cycles;
        while !self.is_terminally_halted() {
            self.step();
            if self.cpu.cycles.wrapping_sub(start) >= max_cycles {
                break;
            }
        }
        self.cpu.cycles.wrapping_sub(start)
    }

    /// Run approximately `budget` cycles' worth of instructions (e.g. one video frame), then
    /// return. Useful for cooperative scheduling with a UI render loop. Returns cycles run.
    pub fn run_cycles(&mut self, budget: u64) -> u64 {
        let start = self.cpu.cycles;
        while self.cpu.cycles.wrapping_sub(start) < budget && !self.is_terminally_halted() {
            self.step();
        }
        self.cpu.cycles.wrapping_sub(start)
    }
}
