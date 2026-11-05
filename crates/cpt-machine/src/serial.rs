//! Bidirectional serial port (USART), e.g. the CPT's RS-232 port.
//!
//! The CPU talks to a USART-style chip (think 8251) through two I/O ports: a **data register**
//! (read a received byte / write a byte to transmit) and a **status/control register** (flags
//! for "receive byte ready", "transmitter ready", etc.). Higher-level protocols — including
//! bisync — are **CPT software** layered on top; this device models only the byte-level
//! hardware primitive.
//!
//! The device is pure and synchronous (safe for the CPU step loop and for wasm): it owns an
//! **RX queue** (bytes arriving from the peer) and a **TX queue** (bytes the CPU has written,
//! waiting to go out). It never blocks. Moving bytes between those queues and an actual
//! transport (a TCP socket, a loopback, etc.) is the job of a host-side pump that calls
//! [`SerialDevice::take_tx`] and [`SerialDevice::push_rx`] — see the `Transport` trait.
//!
//! IMPORTANT [VERIFY]: the port addresses, the exact status-bit layout, the USART chip, and the
//! interrupt vector come from the CPT system source. The structure here is a plausible
//! placeholder, isolated so swapping in the real details is a localized change (mostly config).

use crate::device::Device;
use std::any::Any;
use std::collections::VecDeque;

/// STATUS register bits (placeholder layout — [VERIFY] against the real USART/firmware).
/// Bit 0: a received byte is available to read from the data port.
pub const ST_RX_READY: u8 = 0x01;
/// Bit 1: the transmitter can accept a byte (always true in this model — the TX queue is
/// unbounded in practice; the host pump drains it).
pub const ST_TX_READY: u8 = 0x02;

pub struct SerialDevice {
    base: u8,
    ports: [u8; 2],
    /// Bytes received from the peer, waiting for the CPU to read.
    rx: VecDeque<u8>,
    /// Bytes written by the CPU, waiting for the host pump to send to the peer.
    tx: VecDeque<u8>,
    /// If set, raise this RST vector while a received byte is available.
    irq_vector: Option<u8>,
    irq_pending: bool,
}

impl SerialDevice {
    /// Create a serial device claiming two consecutive ports from `base` (DATA at +0, STATUS at
    /// +1), with an optional receive interrupt vector.
    pub fn new(base: u8, irq_vector: Option<u8>) -> Self {
        SerialDevice {
            base,
            ports: [base, base + 1],
            rx: VecDeque::new(),
            tx: VecDeque::new(),
            irq_vector,
            irq_pending: false,
        }
    }

    // --- host-pump interface (called by the transport side, not the CPU) ---

    /// Push a byte that arrived from the peer into the RX queue (arms the RX interrupt).
    pub fn push_rx(&mut self, byte: u8) {
        self.rx.push_back(byte);
        if self.irq_vector.is_some() {
            self.irq_pending = true;
        }
    }

    /// Drain all bytes the CPU has queued for transmission (to be sent to the peer).
    pub fn take_tx(&mut self) -> Vec<u8> {
        self.tx.drain(..).collect()
    }

    /// Bytes waiting to be transmitted (for tests/inspection).
    pub fn tx_pending(&self) -> usize {
        self.tx.len()
    }
    /// Bytes waiting to be read by the CPU (for tests/inspection).
    pub fn rx_pending(&self) -> usize {
        self.rx.len()
    }

    fn status(&self) -> u8 {
        let mut s = ST_TX_READY; // always ready to accept a TX byte in this model
        if !self.rx.is_empty() {
            s |= ST_RX_READY;
        }
        s
    }
}

impl Device for SerialDevice {
    fn ports(&self) -> &[u8] {
        &self.ports
    }

    fn io_in(&mut self, port: u8) -> u8 {
        let off = port.wrapping_sub(self.base);
        match off {
            0 => self.rx.pop_front().unwrap_or(0), // DATA: next received byte
            1 => self.status(),                    // STATUS
            _ => 0,
        }
    }

    fn io_out(&mut self, port: u8, val: u8) {
        let off = port.wrapping_sub(self.base);
        match off {
            0 => self.tx.push_back(val), // DATA: queue a byte to transmit
            1 => {}                      // control writes: no-op in the placeholder model
            _ => {}
        }
    }

    fn take_irq(&mut self) -> Option<u8> {
        match self.irq_vector {
            Some(v) if self.irq_pending && !self.rx.is_empty() => {
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

/// A byte transport that moves data between the [`SerialDevice`] queues and some endpoint
/// (loopback, TCP, …). The host pumps the device: it calls `send` with the device's TX bytes
/// and `recv` to get bytes to push into the device's RX. Implementations must be non-blocking.
pub trait Transport {
    /// Send bytes toward the peer (the device's TX output).
    fn send(&mut self, bytes: &[u8]);
    /// Return any bytes received from the peer since the last call (non-blocking; may be empty).
    fn recv(&mut self) -> Vec<u8>;
}

/// A loopback transport: everything "sent" comes straight back as "received". Useful for tests
/// and for running comm software against itself without a network.
#[derive(Default)]
pub struct LoopbackTransport {
    buf: VecDeque<u8>,
}

impl LoopbackTransport {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Transport for LoopbackTransport {
    fn send(&mut self, bytes: &[u8]) {
        self.buf.extend(bytes.iter().copied());
    }
    fn recv(&mut self) -> Vec<u8> {
        self.buf.drain(..).collect()
    }
}

/// Pump one round of bytes between a [`SerialDevice`] and a [`Transport`]: send the device's
/// queued TX bytes out, and push any received bytes into the device's RX. The host calls this
/// periodically (e.g. each frame / every N cycles).
pub fn pump(device: &mut SerialDevice, transport: &mut dyn Transport) {
    let out = device.take_tx();
    if !out.is_empty() {
        transport.send(&out);
    }
    for b in transport.recv() {
        device.push_rx(b);
    }
}
