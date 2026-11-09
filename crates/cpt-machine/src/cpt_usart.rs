//! The CPT's serial port: an Intel 8251 USART at 4AH (control/status) and 4BH (data), with a
//! companion register at 4CH.
//!
//! What the TTY program disk (cpt2489) shows [CONFIRMED by disassembly and by running it]:
//! * "TTY COMMUNICATIONS ENABLED" (CODE + I, then the control document `CTTY`) resets the chip
//!   with `00 00 00 40` (three zeros to reach the command register, then internal reset), then
//!   writes mode `4EH` (asynchronous, x16 clock, 8 data bits, no parity, 1 stop bit) and
//!   command `37H` (TxEN, DTR, RxE, error reset, RTS). It writes `00`, then `09`, to 4CH.
//! * The receiver interrupts on **RST 1** (handler 1DD8H: acknowledges 02H on port 00H, checks
//!   RxRDY, reads 4BH, checks the overrun and framing bits 30H).
//! * The transmitter interrupts on **RST 4** (handler 1EB1H: checks TxRDY, writes the next byte
//!   to 4BH and then writes 10H to port 00H; with nothing to send it writes no 10H, and may
//!   turn TxEN off with command 16H). The routine that queues a byte (DF4BH) writes 10H too.
//!
//! The model:
//! * The control port follows the 8251's mode / sync-character / command sequence; internal
//!   reset (command bit 6) goes back to expecting a mode byte.
//! * Status: bit 0 TxRDY (holding register empty), 1 RxRDY, 2 TxEMPTY, 3 PE, 4 OE, 5 FE,
//!   6 SYNDET/BRKDET (always 0), 7 DSR (set by the host: a terminal is attached).
//! * Characters take their real time on the line: (start + data + parity + stop bits) at the
//!   baud rate, 1200 by default (the TTY disk's control page `CTTY` says `RATE: 1200` and
//!   `EVEN PARITY`; the parity bit is made and checked by the software, the chip runs 8 data
//!   bits without parity). 4CH's value is recorded; it is probably the baud-rate select (09
//!   with `RATE: 1200`) [HYPOTHESIS], so it doesn't change the rate.
//! * RxRDY is level-sensitive: it requests RST 1 while a received character waits (with RxE
//!   on). TxRDY (TxEN on, CTS asserted, holding register empty) requests RST 4 **once**; the
//!   request is then held off until 10H is written to port 00H [HYPOTHESIS: the interrupt
//!   controller holds the level in service until that write]. As a plain level, TxRDY with
//!   nothing to send re-interrupted after every `EI; RET`, starving the timer handler (116FH,
//!   which enables interrupts at once), so ticks nested until the stack ran from 2E00H down
//!   through the comm control block at 2D00H. [`Usart8251::new`] keeps the plain level; the
//!   CPT's [`Usart8251::cpt`] uses the acknowledge port.
//! * Received characters are paced: the next one starts only after the CPU has read the last,
//!   so a fast paste from the terminal doesn't overrun (as if the terminal were flow-controlled).
//!   Characters that arrive while the receiver is off are lost, as on a real line.
//!
//! The host side (a terminal) uses [`Usart8251::push_rx`], [`Usart8251::take_tx`] or
//! [`Usart8251::pump`] with a [`Transport`]. Nothing here uses sockets or threads, so the
//! browser build carries the device too.

use crate::device::Device;
use crate::serial::Transport;
use std::any::Any;
use std::collections::VecDeque;

/// Status register bits.
pub const ST_TXRDY: u8 = 0x01;
pub const ST_RXRDY: u8 = 0x02;
pub const ST_TXEMPTY: u8 = 0x04;
pub const ST_PE: u8 = 0x08;
pub const ST_OE: u8 = 0x10;
pub const ST_FE: u8 = 0x20;
pub const ST_DSR: u8 = 0x80;

/// Command register bits.
pub const CMD_TXEN: u8 = 0x01;
pub const CMD_DTR: u8 = 0x02;
pub const CMD_RXE: u8 = 0x04;
pub const CMD_SBRK: u8 = 0x08;
pub const CMD_ER: u8 = 0x10;
pub const CMD_RTS: u8 = 0x20;
pub const CMD_IR: u8 = 0x40;

/// The CPU clock the line timing is counted in.
pub const CPU_HZ: u32 = 2_000_000;

/// The baud rate used until the host sets another (the TTY disk's control page says 1200).
pub const DEFAULT_BAUD: u32 = 1200;

/// At most this many transmitted bytes are kept for the host; older ones are dropped (so a
/// system with no terminal attached doesn't grow the queue without bound).
const TX_QUEUE_LIMIT: usize = 65_536;

/// What the next write to the control port is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    Mode,
    Sync1,
    Sync2,
    Command,
}

pub struct Usart8251 {
    ports: [u8; 3],
    base: u8,
    expect: Expect,
    mode: u8,
    command: u8,
    rate_select: Option<u8>,
    baud: u32,
    /// Interrupt vectors: receiver (RST 1) and transmitter (RST 4) on the CPT.
    pub rx_vector: Option<u8>,
    pub tx_vector: Option<u8>,
    /// The interrupt acknowledge port. With one, the transmit request is taken once and then
    /// held off until a write there with the transmit vector's bit (10H for RST 4) re-arms it.
    /// Without one, TxRDY requests an interrupt for as long as it is set (a plain level).
    ack_port: Option<u8>,
    tx_armed: bool,
    /// Transmit interrupts taken (for reports).
    pub tx_interrupts: u64,
    /// Modem inputs: DSR (status bit 7) and CTS (gates the transmitter).
    dsr: bool,
    cts: bool,
    // Transmitter: the holding register and the character on the line (cycles left).
    tx_hold: Option<u8>,
    tx_shift: Option<(u8, u32)>,
    tx_out: VecDeque<u8>,
    // Receiver: characters from the host, the one on the line, and the receive buffer.
    rx_in: VecDeque<u8>,
    rx_shift: Option<(u8, u32)>,
    rx_buf: Option<u8>,
    rx_last: u8,
    errors: u8,
    /// Every control-port and 4CH write, in order (port, value), for reports; the first 256.
    pub control_log: Vec<(u8, u8)>,
    /// Characters sent to and received from the line, and breaks started (SBRK set).
    pub sent: u64,
    pub received: u64,
    pub breaks: u64,
}

impl Usart8251 {
    /// An 8251 with control/status at `base`, data at `base + 1` and the companion register at
    /// `base + 2`, interrupting on `rx_vector` / `tx_vector`.
    pub fn new(base: u8, rx_vector: Option<u8>, tx_vector: Option<u8>) -> Self {
        Usart8251 {
            ports: [base, base + 1, base + 2],
            base,
            expect: Expect::Mode,
            mode: 0,
            command: 0,
            rate_select: None,
            baud: DEFAULT_BAUD,
            rx_vector,
            tx_vector,
            ack_port: None,
            tx_armed: true,
            tx_interrupts: 0,
            dsr: false,
            cts: true,
            tx_hold: None,
            tx_shift: None,
            tx_out: VecDeque::new(),
            rx_in: VecDeque::new(),
            rx_shift: None,
            rx_buf: None,
            rx_last: 0,
            errors: 0,
            control_log: Vec::new(),
            sent: 0,
            received: 0,
            breaks: 0,
        }
    }

    /// The CPT's serial port: 4AH-4CH, receive on RST 1, transmit on RST 4.
    /// The transmit request is re-armed through the acknowledge port 00H.
    pub fn cpt() -> Self {
        Usart8251::new(0x4A, Some(1), Some(4)).with_ack_port(0x00)
    }

    /// Hold the transmit request off after each interrupt until `port` gets the vector's bit.
    pub fn with_ack_port(mut self, port: u8) -> Self {
        self.ack_port = Some(port);
        self
    }

    // --- host (terminal) side ---

    /// Characters from the terminal, to arrive on the receive line in order.
    pub fn push_rx(&mut self, bytes: &[u8]) {
        self.rx_in.extend(bytes.iter().copied());
    }

    /// Characters the CPT has finished sending since the last call.
    pub fn take_tx(&mut self) -> Vec<u8> {
        self.tx_out.drain(..).collect()
    }

    /// Move characters both ways between the line and a transport (a TCP terminal, ...).
    pub fn pump(&mut self, transport: &mut dyn Transport) {
        let out = self.take_tx();
        if !out.is_empty() {
            transport.send(&out);
        }
        let incoming = transport.recv();
        if !incoming.is_empty() {
            self.push_rx(&incoming);
        }
    }

    /// DSR, status bit 7: whether something is attached to the line.
    pub fn set_dsr(&mut self, on: bool) {
        self.dsr = on;
    }

    /// CTS: while it's off the transmitter holds its character.
    pub fn set_cts(&mut self, on: bool) {
        self.cts = on;
    }

    /// The line's baud rate (the default is [`DEFAULT_BAUD`]).
    pub fn set_baud(&mut self, baud: u32) {
        self.baud = baud.max(50);
    }

    pub fn baud(&self) -> u32 {
        self.baud
    }

    /// The last mode and command bytes written, and the last value written to 4CH.
    pub fn mode(&self) -> u8 {
        self.mode
    }
    pub fn command(&self) -> u8 {
        self.command
    }
    pub fn rate_select(&self) -> Option<u8> {
        self.rate_select
    }

    /// Characters from the terminal not yet delivered to the CPU.
    pub fn rx_waiting(&self) -> usize {
        self.rx_in.len() + self.rx_shift.is_some() as usize + self.rx_buf.is_some() as usize
    }

    pub fn status(&self) -> u8 {
        let mut s = self.errors;
        if self.tx_hold.is_none() {
            s |= ST_TXRDY;
            if self.tx_shift.is_none() {
                s |= ST_TXEMPTY;
            }
        }
        if self.rx_buf.is_some() {
            s |= ST_RXRDY;
        }
        if self.dsr {
            s |= ST_DSR;
        }
        s
    }

    /// Cycles one character takes on the line: start bit, data bits, parity, stop bits.
    pub fn char_cycles(&self) -> u32 {
        // Counted in half bits, for 1.5 stop bits.
        let data = 5 + ((self.mode >> 2) & 3) as u32;
        let parity = ((self.mode >> 4) & 1) as u32;
        let stop_halves = match self.mode >> 6 {
            2 => 3,
            3 => 4,
            _ => 2,
        };
        let halves = 2 * (1 + data + parity) + stop_halves;
        ((CPU_HZ as u64 * halves as u64) / (2 * self.baud as u64)).max(1) as u32
    }

    fn tx_enabled(&self) -> bool {
        self.command & CMD_TXEN != 0 && self.cts
    }

    fn rx_enabled(&self) -> bool {
        self.command & CMD_RXE != 0
    }

    fn write_control(&mut self, val: u8) {
        match self.expect {
            Expect::Mode => {
                self.mode = val;
                // Synchronous mode (baud factor 00) is followed by one or two sync characters.
                self.expect = if val & 3 == 0 { Expect::Sync1 } else { Expect::Command };
            }
            Expect::Sync1 => {
                self.expect = if self.mode & 0x80 != 0 { Expect::Command } else { Expect::Sync2 };
            }
            Expect::Sync2 => self.expect = Expect::Command,
            Expect::Command => {
                if val & CMD_IR != 0 {
                    // Internal reset: back to expecting a mode byte.
                    self.expect = Expect::Mode;
                    self.command = 0;
                    self.tx_hold = None;
                    self.tx_shift = None;
                    self.rx_shift = None;
                    self.rx_buf = None;
                    self.errors = 0;
                    return;
                }
                if val & CMD_SBRK != 0 && self.command & CMD_SBRK == 0 {
                    self.breaks += 1;
                }
                if val & CMD_ER != 0 {
                    self.errors = 0;
                }
                self.command = val;
            }
        }
    }
}

impl Device for Usart8251 {
    fn ports(&self) -> &[u8] {
        &self.ports
    }

    fn io_in(&mut self, port: u8) -> u8 {
        match port.wrapping_sub(self.base) {
            0 => self.status(),
            1 => {
                if let Some(b) = self.rx_buf.take() {
                    self.rx_last = b;
                }
                self.rx_last
            }
            _ => self.rate_select.unwrap_or(0),
        }
    }

    fn io_out(&mut self, port: u8, val: u8) {
        let off = port.wrapping_sub(self.base);
        if off != 1 && self.control_log.len() < 256 {
            self.control_log.push((port, val));
        }
        match off {
            0 => self.write_control(val),
            1 => self.tx_hold = Some(val),
            _ => self.rate_select = Some(val),
        }
    }

    fn tick(&mut self, cycles: u32) {
        // Transmitter: the holding register moves onto the line when it's free.
        if self.tx_shift.is_none() && self.tx_enabled() {
            if let Some(b) = self.tx_hold.take() {
                self.tx_shift = Some((b, self.char_cycles()));
            }
        }
        if let Some((b, left)) = self.tx_shift {
            if left <= cycles {
                self.tx_shift = None;
                if self.tx_out.len() >= TX_QUEUE_LIMIT {
                    self.tx_out.pop_front();
                }
                self.tx_out.push_back(b);
                self.sent += 1;
            } else {
                self.tx_shift = Some((b, left - cycles));
            }
        }

        // Receiver.
        if !self.rx_enabled() {
            self.rx_in.clear();
            self.rx_shift = None;
            return;
        }
        if self.rx_shift.is_none() && self.rx_buf.is_none() {
            if let Some(b) = self.rx_in.pop_front() {
                self.rx_shift = Some((b, self.char_cycles()));
            }
        }
        if let Some((b, left)) = self.rx_shift {
            if left <= cycles {
                self.rx_shift = None;
                if self.rx_buf.is_some() {
                    self.errors |= ST_OE;
                }
                self.rx_buf = Some(b);
                self.received += 1;
            } else {
                self.rx_shift = Some((b, left - cycles));
            }
        }
    }

    fn take_irq(&mut self) -> Option<u8> {
        if self.rx_buf.is_some() && self.rx_enabled() {
            if let Some(v) = self.rx_vector {
                return Some(v);
            }
        }
        if self.tx_hold.is_none() && self.tx_enabled() && self.tx_armed {
            if let Some(v) = self.tx_vector {
                if self.ack_port.is_some() {
                    self.tx_armed = false;
                }
                self.tx_interrupts += 1;
                return Some(v);
            }
        }
        None
    }

    fn observe_out(&mut self, port: u8, val: u8) {
        if let (Some(ack), Some(v)) = (self.ack_port, self.tx_vector) {
            if port == ack && val & (1 << v) != 0 {
                self.tx_armed = true;
            }
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
