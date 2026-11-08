//! The real CPT interval timer and interrupt acknowledge port, as found by running the boot of a
//! real program disk (`.kiro/specs/cpt8000-emulator/program-disk.md`).
//!
//! * **Intel 8253 interval timer at 1CH-1FH** (counters 0-2, control word at 1FH). The system
//!   programs counter 1 to 16000 and counter 2 to 20000 (mode 2, rate generator); with a 2 MHz
//!   input that is 125 Hz and 100 Hz. Each counter's output can raise an RST interrupt.
//!   Which counter drives which RST is [VERIFY]; the defaults below are a guess (counter 2, the
//!   100 Hz task-switch tick, -> RST 7, whose handler has the task-switch logic; counter 1 ->
//!   RST 5).
//! * **Interrupt acknowledge at 00H.** Every interrupt handler ends by writing a single bit to
//!   port 00H matching its own RST number (RST 2 writes 04H, RST 3 08H, RST 5 20H, RST 7 80H),
//!   so the port clears the corresponding request. Interrupts are delivered as 8080 RST
//!   instructions (the vectors at 0000H-0038H are 8 bytes apart).
//!
//! Requests here are edge-like, as elsewhere in the emulator: a request is taken when the CPU
//! accepts it. The acknowledge writes are recorded but have no further effect.

use crate::device::Device;
use std::any::Any;

// The speaker [CONFIRMED by disassembly]: the sound routine at 153AH plays a note by writing
// control word 36H (counter 0, LSB then MSB, mode 3 = square wave) and a count to 1CH, waits,
// then writes 30H (counter 0, mode 0), which silences it. Counter 0 is not wired to an
// interrupt; its output drives the speaker.

#[derive(Clone, Copy, Default)]
struct Counter {
    /// Reload value (0 means 65536).
    reload: u32,
    /// Cycles left until the next output pulse.
    left: u32,
    /// Programmed and counting.
    running: bool,
    /// For a 16-bit load: true once the low byte has arrived.
    have_low: bool,
    low: u8,
    /// Read/load mode from the control word: 1 = LSB only, 2 = MSB only, 3 = LSB then MSB.
    rl: u8,
    mode: u8,
    /// RST vector raised on each output pulse, if wired.
    vector: Option<u8>,
    pending: bool,
}

/// An Intel 8253 with three counters clocked from the CPU clock (input divided by `prescale`).
pub struct Pit8253 {
    base: u8,
    ports: [u8; 4],
    counters: [Counter; 3],
    /// CPU cycles per counter clock (1 when the 8253 is clocked at the CPU's 2 MHz).
    prescale: u32,
    sub: u32,
    /// Output pulses per counter, for diagnostics.
    pub pulses: [u64; 3],
    /// CPU cycles since the 8253 was created (the time stamp for speaker events).
    cycle: u64,
    /// The speaker's current tone: counter 0's count while it runs in mode 3, else None.
    tone: Option<u32>,
    /// Speaker changes not yet taken, as (cycle, count): the speaker plays a square wave of
    /// 2 MHz / count from that cycle on, or is silent if count is 0.
    pub speaker: Vec<(u64, u32)>,
}

/// The 8253's input clock (Hz): the CPU's 2 MHz (counter 1's 16000 gives the 125 Hz tick).
pub const PIT_CLOCK_HZ: u32 = 2_000_000;

impl Pit8253 {
    /// An 8253 at `base`..`base+3`, counters 0-2 raising the given RST vectors.
    pub fn new(base: u8, vectors: [Option<u8>; 3]) -> Self {
        let mut counters = [Counter::default(); 3];
        for (c, v) in counters.iter_mut().zip(vectors) {
            c.vector = v;
            c.rl = 3;
        }
        Pit8253 { base, ports: [base, base + 1, base + 2, base + 3], counters, prescale: 1, sub: 0,
                  pulses: [0; 3], cycle: 0, tone: None, speaker: Vec::new() }
    }

    /// Take the speaker changes since the last call.
    pub fn take_speaker(&mut self) -> Vec<(u64, u32)> {
        std::mem::take(&mut self.speaker)
    }

    /// The speaker is sounding a tone now.
    pub fn tone_on(&self) -> bool {
        self.tone.is_some()
    }

    /// Record a speaker change if counter 0's tone changed: a square wave (mode 3) while it
    /// runs, silence otherwise. The speaker is driven by counter 0's output (the CPT's sound
    /// routine at 153AH plays notes this way; see program-disk.md).
    fn speaker_update(&mut self) {
        let c = &self.counters[0];
        let tone = if c.running && c.mode & 3 == 3 { Some(c.reload) } else { None };
        if tone != self.tone {
            self.tone = tone;
            self.speaker.push((self.cycle, tone.unwrap_or(0)));
        }
    }

    /// The CPT wiring (guessed): counter 1 -> RST 5, counter 2 -> RST 7, at 1CH.
    pub fn cpt() -> Self {
        Pit8253::new(0x1C, [None, Some(5), Some(7)])
    }

    /// The current reload value of counter `n` (0 if never programmed).
    pub fn reload(&self, n: usize) -> u32 {
        self.counters[n].reload
    }

    fn load(&mut self, n: usize, val: u8) {
        let c = &mut self.counters[n];
        let value = match c.rl {
            1 => Some(val as u32),
            2 => Some((val as u32) << 8),
            _ => {
                if c.have_low {
                    c.have_low = false;
                    Some((val as u32) << 8 | c.low as u32)
                } else {
                    c.low = val;
                    c.have_low = true;
                    None
                }
            }
        };
        if let Some(v) = value {
            c.reload = if v == 0 { 65536 } else { v };
            c.left = c.reload;
            c.running = true;
        }
        if n == 0 {
            self.speaker_update();
        }
    }
}

impl Device for Pit8253 {
    fn ports(&self) -> &[u8] {
        &self.ports
    }

    fn io_in(&mut self, port: u8) -> u8 {
        let n = (port - self.base) as usize;
        if n < 3 {
            // Reading the count back: return the low byte of what is left.
            (self.counters[n].left & 0xFF) as u8
        } else {
            0xFF
        }
    }

    fn io_out(&mut self, port: u8, val: u8) {
        let n = (port - self.base) as usize;
        if n < 3 {
            self.load(n, val);
            return;
        }
        // Control word: SC1 SC0 | RL1 RL0 | M2 M1 M0 | BCD.
        let sel = (val >> 6) as usize;
        if sel > 2 {
            return;
        }
        let rl = (val >> 4) & 3;
        if rl == 0 {
            return; // counter latch command: nothing to latch in this model
        }
        let c = &mut self.counters[sel];
        c.rl = rl;
        c.mode = (val >> 1) & 7;
        c.have_low = false;
        c.running = false; // a new control word stops the counter until it is loaded
        if sel == 0 {
            self.speaker_update();
        }
    }

    fn tick(&mut self, cycles: u32) {
        self.cycle += cycles as u64;
        self.sub += cycles;
        let clocks = self.sub / self.prescale;
        self.sub %= self.prescale;
        for (i, c) in self.counters.iter_mut().enumerate() {
            if !c.running || c.reload == 0 {
                continue;
            }
            let mut t = clocks;
            while t >= c.left {
                t -= c.left;
                c.left = c.reload;
                self.pulses[i] += 1;
                if c.vector.is_some() {
                    c.pending = true;
                }
                // Modes 2 and 3 repeat; other modes are one-shots.
                if !matches!(c.mode, 2 | 3 | 6 | 7) {
                    c.running = false;
                    break;
                }
            }
            if c.running {
                c.left -= t;
            }
        }
    }

    fn take_irq(&mut self) -> Option<u8> {
        // Higher RST numbers first.
        let mut best: Option<usize> = None;
        for (i, c) in self.counters.iter().enumerate() {
            if c.pending {
                if best.map_or(true, |b| c.vector > self.counters[b].vector) {
                    best = Some(i);
                }
            }
        }
        let i = best?;
        self.counters[i].pending = false;
        self.counters[i].vector
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The interrupt acknowledge port (00H): handlers write bit n to clear RST n's request. This
/// model records the writes (so they are visible, not "unmapped").
pub struct InterruptAck {
    ports: [u8; 1],
    /// Acknowledge writes seen per value, for diagnostics.
    pub acks: Vec<(u8, u64)>,
}

impl InterruptAck {
    pub fn new(port: u8) -> Self {
        InterruptAck { ports: [port], acks: Vec::new() }
    }
}

impl Device for InterruptAck {
    fn ports(&self) -> &[u8] {
        &self.ports
    }
    fn io_in(&mut self, _port: u8) -> u8 {
        0
    }
    fn io_out(&mut self, _port: u8, val: u8) {
        match self.acks.iter_mut().find(|a| a.0 == val) {
            Some(a) => a.1 += 1,
            None => self.acks.push((val, 1)),
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
