//! The real CPT keyboard interface, as used by the program disk's code
//! (`.kiro/specs/cpt8000-emulator/program-disk.md`, "The keyboard").
//!
//! **What the software expects [CONFIRMED by disassembly]:**
//! - **RST 2** (handler at 0D07H) when a key event is waiting.
//! - **IN 01H**: bit 7 set means a key event is ready.
//! - **IN 02H**: the event, **inverted**. After `XRI FFH` an event is a key number 01H–7FH for a
//!   press, the same number + 80H for a release, or 80H for "all keys up". 00H (the line
//!   idle at FFH) is ignored. Pressing key 3FH also sets ten flags at 04F1H.
//! - The handler acknowledges with `OUT 00H, 04H`.
//! - The handler puts each event into a 128-byte ring at 058BH. A task (D776H, block 2) takes
//!   them out and translates key numbers through the table at DEFFH (block 2). That table has
//!   the unshifted value at index `(k & 0FH) * 8 + (k >> 4)` and the shifted one 7FH further on.
//!
//! Every key reports its own make and break, so the software follows N-key rollover itself.
//! It holds SHIFT (key 70H), LOCK (72H) and the two CODE keys (07H, 77H) as state.
//!
//! **Modelled here [HYPOTHESIS]:** one event is latched at a time. Reading 02H clears it, and
//! the next queued event is latched `gap_cycles` later. The real serial timing of the keyboard
//! link is unknown.

use crate::device::Device;
use std::any::Any;
use std::collections::VecDeque;

/// The SHIFT key's number.
pub const KEY_SHIFT: u8 = 0x70;
/// The LOCK (shift lock) key's number.
pub const KEY_LOCK: u8 = 0x72;
/// The CODE key's number (there is a second CODE key at 07H).
pub const KEY_CODE: u8 = 0x77;
/// The space bar's number.
pub const KEY_SPACE: u8 = 0x10;

/// The program's key table (DEFFH, block 2), unshifted, by key number. Values below 80H are
/// characters. 80H = CODE, 81H = SHIFT and 82H = LOCK. Other values of A0H and up are function
/// codes whose names are not yet known. 00 = no key.
pub const KEY_TABLE: [u8; 128] = [
    0x00, 0xB3, 0xA8, 0x00, 0x00, 0xB2, 0xB5, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0xBD, 0xAE, 0xA5,
    0x20, 0x00, 0x00, 0x27, 0xB6, 0x7C, 0x2D, 0x3D, 0xE6, 0xE8, 0xEB, 0xEE, 0xE5, 0x00, 0x00, 0xA6,
    0x2E, 0x2F, 0x3B, 0x6C, 0x70, 0x6F, 0x39, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA2, 0xAC, 0xB0,
    0x6D, 0x2C, 0x6B, 0x6A, 0x69, 0x75, 0x37, 0x38, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA1, 0xAB, 0xA4,
    0x62, 0x6E, 0x68, 0x67, 0x79, 0x74, 0x35, 0x36, 0x00, 0xE7, 0xEA, 0xED, 0xE0, 0x00, 0x00, 0xB4,
    0x63, 0x76, 0x66, 0x64, 0x72, 0x65, 0x33, 0x34, 0x00, 0xE9, 0xEC, 0xEF, 0xE1, 0x00, 0x00, 0xAF,
    0x7A, 0x78, 0x73, 0x61, 0x77, 0x71, 0x31, 0x32, 0x00, 0xDF, 0xE4, 0xE2, 0xE3, 0x00, 0x00, 0xFE,
    0x81, 0xB9, 0x82, 0xAA, 0xB7, 0xB8, 0xBA, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA3, 0xAD, 0xB1,
];

/// The same table with SHIFT held.
pub const KEY_TABLE_SHIFTED: [u8; 128] = [
    0x00, 0xB3, 0xA8, 0x00, 0x00, 0xB2, 0xB5, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0xBD, 0xAE, 0xA5,
    0x20, 0x00, 0x00, 0x22, 0xB6, 0x5C, 0x8F, 0x2B, 0xE6, 0xE8, 0xEB, 0xEE, 0xE5, 0x00, 0x00, 0xA6,
    0x2E, 0x3F, 0x3A, 0x4C, 0x50, 0x4F, 0x28, 0x29, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA2, 0xAC, 0xB0,
    0x4D, 0x2C, 0x4B, 0x4A, 0x49, 0x55, 0x26, 0x2A, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA1, 0xAB, 0xA4,
    0x42, 0x4E, 0x48, 0x47, 0x59, 0x54, 0x25, 0x5F, 0x00, 0xE7, 0xEA, 0xED, 0xE0, 0x00, 0x00, 0xB4,
    0x43, 0x56, 0x46, 0x44, 0x52, 0x45, 0x23, 0x24, 0x00, 0xE9, 0xEC, 0xEF, 0xE1, 0x00, 0x00, 0xAF,
    0x5A, 0x58, 0x53, 0x41, 0x57, 0x51, 0x21, 0x40, 0x00, 0xDF, 0xE4, 0xE2, 0xE3, 0x00, 0x00, 0xFE,
    0x81, 0xB9, 0x82, 0xAA, 0xB7, 0xB8, 0xBA, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA3, 0xAD, 0xB1,
];

/// The key (and whether SHIFT is needed) that types `c`, if the keyboard has it.
pub fn key_for_char(c: char) -> Option<(u8, bool)> {
    let c = u8::try_from(c as u32).ok()?;
    if c == 0 || c >= 0x80 {
        return None;
    }
    if let Some(k) = KEY_TABLE.iter().position(|&v| v == c) {
        return Some((k as u8, false));
    }
    KEY_TABLE_SHIFTED.iter().position(|&v| v == c).map(|k| (k as u8, true))
}

/// The CPT keyboard on ports 01H (status) and 02H (inverted event), raising RST 2.
pub struct CptKeyboard {
    ports: [u8; 2],
    queue: VecDeque<u8>,
    /// The latched event (not inverted), if any.
    latched: Option<u8>,
    irq_pending: bool,
    /// Cycles until the next queued event may be latched.
    wait: u32,
    /// Cycles between one event being read and the next being latched.
    pub gap_cycles: u32,
    /// Every event the CPU has read, for diagnostics.
    pub delivered: Vec<u8>,
}

impl Default for CptKeyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl CptKeyboard {
    pub fn new() -> Self {
        CptKeyboard {
            ports: [0x01, 0x02],
            queue: VecDeque::new(),
            latched: None,
            irq_pending: false,
            wait: 0,
            gap_cycles: 2_000,
            delivered: Vec::new(),
        }
    }

    /// Queue a raw event: a key number for a press, key | 80H for a release, or 80H for all up.
    pub fn push_event(&mut self, event: u8) {
        self.queue.push_back(event);
    }

    pub fn press(&mut self, key: u8) {
        self.push_event(key & 0x7F);
    }

    pub fn release(&mut self, key: u8) {
        self.push_event((key & 0x7F) | 0x80);
    }

    /// Events not yet read by the CPU (queued plus latched).
    pub fn pending(&self) -> usize {
        self.queue.len() + self.latched.is_some() as usize
    }

    fn latch_next(&mut self) {
        if self.latched.is_none() && self.wait == 0 {
            if let Some(e) = self.queue.pop_front() {
                self.latched = Some(e);
                self.irq_pending = true;
            }
        }
    }
}

impl Device for CptKeyboard {
    fn ports(&self) -> &[u8] {
        &self.ports
    }

    fn io_in(&mut self, port: u8) -> u8 {
        match port {
            0x01 => {
                if self.latched.is_some() { 0x80 } else { 0x00 }
            }
            _ => match self.latched.take() {
                Some(e) => {
                    self.delivered.push(e);
                    self.wait = self.gap_cycles;
                    !e
                }
                None => 0xFF, // idle line: reads as event 00, which the handler ignores
            },
        }
    }

    fn io_out(&mut self, _port: u8, _val: u8) {}

    fn tick(&mut self, cycles: u32) {
        self.wait = self.wait.saturating_sub(cycles);
        self.latch_next();
    }

    fn take_irq(&mut self) -> Option<u8> {
        if self.irq_pending && self.latched.is_some() {
            self.irq_pending = false;
            Some(2)
        } else {
            None
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_inverted_and_flagged_on_bit_7() {
        let mut kb = CptKeyboard::new();
        assert_eq!(kb.io_in(0x01), 0x00);
        assert_eq!(kb.io_in(0x02), 0xFF, "idle reads as event 00");
        kb.press(0x63);
        kb.tick(1);
        assert_eq!(kb.take_irq(), Some(2));
        assert_eq!(kb.take_irq(), None, "one request per event");
        assert_eq!(kb.io_in(0x01), 0x80);
        assert_eq!(kb.io_in(0x02), !0x63);
        assert_eq!(kb.io_in(0x01), 0x00, "reading the data clears ready");
    }

    #[test]
    fn the_next_event_waits_for_the_gap() {
        let mut kb = CptKeyboard::new();
        kb.gap_cycles = 100;
        kb.press(0x63);
        kb.release(0x63);
        kb.tick(1);
        assert_eq!(kb.io_in(0x02), !0x63);
        kb.tick(50);
        assert_eq!(kb.io_in(0x01), 0x00);
        kb.tick(50);
        assert_eq!(kb.io_in(0x01), 0x80);
        assert_eq!(kb.take_irq(), Some(2));
        assert_eq!(kb.io_in(0x02), !0xE3);
        assert_eq!(kb.delivered, vec![0x63, 0xE3]);
    }

    #[test]
    fn characters_map_to_keys_through_the_programs_table() {
        assert_eq!(key_for_char('a'), Some((0x63, false)));
        assert_eq!(key_for_char('A'), Some((0x63, true)));
        assert_eq!(key_for_char('l'), Some((0x23, false)));
        assert_eq!(key_for_char(' '), Some((KEY_SPACE, false)));
        assert_eq!(key_for_char('?'), Some((0x21, true)));
        assert_eq!(key_for_char('1'), Some((0x66, false)));
        assert_eq!(key_for_char('!'), Some((0x66, true)));
        assert_eq!(key_for_char('<'), None, "no < key in the table");
        assert_eq!(KEY_TABLE[KEY_SHIFT as usize], 0x81);
        assert_eq!(KEY_TABLE[KEY_LOCK as usize], 0x82);
        assert_eq!(KEY_TABLE[KEY_CODE as usize], 0x80);
        assert_eq!(KEY_TABLE[0x07], 0x80, "the second CODE key");
    }
}
