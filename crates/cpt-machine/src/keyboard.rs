//! The keyboard device: queues keystrokes from the host and presents them to the CPU.
//!
//! The host (UI / wasm) pushes key codes into a FIFO with [`KeyboardDevice::push_key`]. The CPU
//! reads them through two I/O ports:
//!   * STATUS (in): bit 0 = a key is available.
//!   * DATA   (in): returns the next queued key code and removes it (0 if the queue is empty).
//!
//! The real CPT keyboard was **N-key rollover and interrupt-driven**: it tracked any number of
//! simultaneously-held keys without dropping events, and raised an interrupt when a key event
//! occurred (rather than being polled). We model that here: the device is interrupt-driven by
//! default, and `push_key` / `push_key_up` queue independent make (press) and break (release)
//! events so software can follow N-key rollover. A key-available STATUS bit and a DATA port are
//! also provided so polled access still works.
//!
//! IMPORTANT [VERIFY]: the port addresses, the STATUS bit layout, the scan/key codes each
//! physical key sends (including the make/break encoding), and the exact interrupt vector all
//! come from the CPT system source. The structure here is a plausible placeholder, isolated so
//! swapping in the real details is a localized change.

use crate::device::Device;
use std::any::Any;
use std::collections::VecDeque;

pub struct KeyboardDevice {
    base: u8,
    ports: [u8; 2],
    /// FIFO of key codes waiting to be read by the CPU.
    queue: VecDeque<u8>,
    /// If set, raise this RST vector while a key is available (interrupt-driven mode).
    irq_vector: Option<u8>,
    /// Latches that an interrupt is pending until taken (so we raise once per new key batch).
    irq_pending: bool,
}

/// High bit convention for a break (key-release) event in this placeholder encoding: a make
/// event sends `code`; the matching break event sends `code | BREAK_FLAG`. [VERIFY] — the real
/// make/break encoding comes from the system source.
pub const BREAK_FLAG: u8 = 0x80;

impl KeyboardDevice {
    /// Create an interrupt-driven keyboard claiming two consecutive ports from `base`, raising
    /// `irq_vector` on key events (the real CPT keyboard was interrupt-driven).
    pub fn new(base: u8, irq_vector: u8) -> Self {
        KeyboardDevice {
            base,
            ports: [base, base + 1],
            queue: VecDeque::new(),
            irq_vector: Some(irq_vector),
            irq_pending: false,
        }
    }

    /// Create a polled-only keyboard (no interrupt) — mainly for tests.
    pub fn polled(base: u8) -> Self {
        KeyboardDevice {
            base,
            ports: [base, base + 1],
            queue: VecDeque::new(),
            irq_vector: None,
            irq_pending: false,
        }
    }

    /// Queue a key **press** (make) event.
    pub fn push_key(&mut self, code: u8) {
        self.queue.push_back(code);
        if self.irq_vector.is_some() {
            self.irq_pending = true;
        }
    }

    /// Queue a key **release** (break) event. With N-key rollover the host reports releases
    /// independently; here the break is `code | BREAK_FLAG` ([VERIFY] encoding).
    pub fn push_key_up(&mut self, code: u8) {
        self.push_key(code | BREAK_FLAG);
    }

    /// Number of keys waiting (for tests / host inspection).
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// STATUS byte: bit 0 set when a key is available.
    fn status(&self) -> u8 {
        if self.queue.is_empty() {
            0
        } else {
            0x01
        }
    }

    /// DATA read: pop and return the next key code (0 if empty).
    fn read_data(&mut self) -> u8 {
        self.queue.pop_front().unwrap_or(0)
    }
}

impl Device for KeyboardDevice {
    fn ports(&self) -> &[u8] {
        &self.ports
    }

    fn io_in(&mut self, port: u8) -> u8 {
        let off = port.wrapping_sub(self.base);
        match off {
            0 => self.status(),
            1 => self.read_data(),
            _ => 0,
        }
    }

    fn io_out(&mut self, _port: u8, _val: u8) {
        // No output ports in this placeholder model (e.g. keyboard LEDs/bell would go here).
    }

    fn take_irq(&mut self) -> Option<u8> {
        match self.irq_vector {
            Some(v) if self.irq_pending && !self.queue.is_empty() => {
                self.irq_pending = false;
                Some(v)
            }
            _ => None,
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
