//! The task-switch timer: a fixed-rate periodic interrupt.
//!
//! The CPT monitor was **multitasking**, and a periodic timer interrupt (recalled as about
//! 100 Hz) triggered its task switches. Unlike the display's vertical retrace (a separate,
//! independent interrupt: see [`crate::display`]), this tick is load-bearing for the system
//! software: the scheduler depends on it.
//!
//! The hardware is deliberately minimal: **not programmable**. It has no I/O ports, no
//! registers, and no enable bit: it just raises its interrupt every period for as long as the
//! machine runs. The CPU masks it with `DI` like any other interrupt. Like a real INT line it
//! is a *request*, not a queue: a tick that arrives while interrupts are disabled stays
//! pending and is delivered when they are re-enabled, and ticks that pile up while masked
//! coalesce into one. (The executive only polls devices while the CPU can accept an interrupt.)
//!
//! Frequency and RST vector are configuration [VERIFY]; see
//! [`MachineConfig`](crate::MachineConfig).

use crate::device::Device;
use std::any::Any;

pub struct TimerDevice {
    period: u32,
    vector: u8,
    cycle_accum: u32,
    pending: bool,
    /// Number of timer interrupts delivered (consumed via `take_irq`, which the executive
    /// only calls when the CPU accepts the interrupt).
    pub tick_count: u64,
}

impl TimerDevice {
    /// A timer that raises RST `vector` every `period_cycles` CPU cycles.
    pub fn new(period_cycles: u32, vector: u8) -> Self {
        assert!(period_cycles > 0, "timer period must be non-zero");
        TimerDevice { period: period_cycles, vector, cycle_accum: 0, pending: false, tick_count: 0 }
    }

    /// The period between interrupts, in CPU cycles.
    pub fn period_cycles(&self) -> u32 {
        self.period
    }
}

impl Device for TimerDevice {
    /// No ports: the timer is not programmable.
    fn ports(&self) -> &[u8] {
        &[]
    }

    fn io_in(&mut self, _port: u8) -> u8 {
        0
    }

    fn io_out(&mut self, _port: u8, _val: u8) {}

    /// Accumulate cycles; request an interrupt each time a period elapses. Ticks that elapse
    /// before the previous one is taken coalesce into one (an interrupt line, not a queue).
    fn tick(&mut self, cycles: u32) {
        self.cycle_accum += cycles;
        while self.cycle_accum >= self.period {
            self.cycle_accum -= self.period;
            self.pending = true;
        }
    }

    fn take_irq(&mut self) -> Option<u8> {
        if self.pending {
            self.pending = false;
            self.tick_count += 1;
            Some(self.vector)
        } else {
            None
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
