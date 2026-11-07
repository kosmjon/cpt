//! The speaker: a single-channel programmable tone generator.
//!
//! The CPT hardware had **one** speaker channel, used for beeps, tones and (programmed from
//! software) simple melodies. How the real board generated the tone is [VERIFY]: a CPU-toggled
//! output bit and a hardware divider are both plausible. This is a **guess**, isolated here so
//! it can be replaced: a programmable divider, which also suits a multitasking monitor because
//! a note plays by itself and leaves the CPU free.
//!
//! ## Guessed register interface (two consecutive output ports from `base`)
//! | port  | dir | meaning                                                              |
//! |-------|-----|----------------------------------------------------------------------|
//! | +0    | out | divisor LOW byte: latched, no audible effect yet                      |
//! | +1    | out | divisor HIGH byte: **commits** `{high, latched low}` and starts a tone |
//! | +0    | in  | status: bit 0 = a tone is sounding                                    |
//!
//! The speaker toggles every `divisor` ticks of a `tone_clock_hz` clock, so the square-wave
//! frequency is `tone_clock_hz / (2 * divisor)`. Divisor 0 means silence.
//!
//! ## Capture
//! Every commit is recorded as a [`SpeakerEvent`] with the CPU cycle at which it happened.
//! That is all a host needs to reconstruct the sound, whatever generated it; the
//! [`ToneRenderer`] turns the event stream into PCM samples (for Web Audio, or [`wav_bytes`]
//! for a file).

use crate::device::Device;
use std::any::Any;
use std::collections::VecDeque;

/// One change of the speaker's tone, stamped with the CPU cycle count when it was committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeakerEvent {
    pub cycle: u64,
    /// The new divisor; 0 = silence.
    pub divisor: u16,
}

pub struct SpeakerDevice {
    ports: [u8; 2],
    tone_clock_hz: u32,
    latch_lo: u8,
    divisor: u16,
    cycles: u64,
    events: Vec<SpeakerEvent>,
}

impl SpeakerDevice {
    /// A speaker on ports `base` and `base + 1`, whose divider counts a `tone_clock_hz` clock.
    pub fn new(base: u8, tone_clock_hz: u32) -> Self {
        assert!(tone_clock_hz > 0, "tone clock must be non-zero");
        SpeakerDevice {
            ports: [base, base + 1],
            tone_clock_hz,
            latch_lo: 0,
            divisor: 0,
            cycles: 0,
            events: Vec::new(),
        }
    }

    /// The committed divisor (0 = silent).
    pub fn divisor(&self) -> u16 {
        self.divisor
    }

    /// True if a tone is currently sounding.
    pub fn is_on(&self) -> bool {
        self.divisor != 0
    }

    /// The current tone's frequency in Hz (0.0 when silent).
    pub fn frequency_hz(&self) -> f64 {
        divisor_to_hz(self.divisor, self.tone_clock_hz)
    }

    /// The tone clock the divider counts, in Hz.
    pub fn tone_clock_hz(&self) -> u32 {
        self.tone_clock_hz
    }

    /// CPU cycles seen by the device so far.
    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    /// Take the events recorded since the last call.
    pub fn take_events(&mut self) -> Vec<SpeakerEvent> {
        std::mem::take(&mut self.events)
    }
}

/// The frequency a divisor produces on a `tone_clock_hz` clock (0.0 for divisor 0).
pub fn divisor_to_hz(divisor: u16, tone_clock_hz: u32) -> f64 {
    if divisor == 0 {
        0.0
    } else {
        tone_clock_hz as f64 / (2.0 * divisor as f64)
    }
}

/// The divisor closest to `hz` on a `tone_clock_hz` clock (clamped to 1..=65535).
pub fn hz_to_divisor(hz: f64, tone_clock_hz: u32) -> u16 {
    let d = (tone_clock_hz as f64 / (2.0 * hz)).round();
    d.clamp(1.0, 65535.0) as u16
}

impl Device for SpeakerDevice {
    fn ports(&self) -> &[u8] {
        &self.ports
    }

    /// `+0`: status, bit 0 = a tone is sounding. `+1`: reads as 0.
    fn io_in(&mut self, port: u8) -> u8 {
        if port == self.ports[0] {
            self.is_on() as u8
        } else {
            0
        }
    }

    fn io_out(&mut self, port: u8, val: u8) {
        if port == self.ports[0] {
            self.latch_lo = val; // latched only; the tone changes when the high byte arrives
        } else {
            self.divisor = (val as u16) << 8 | self.latch_lo as u16;
            self.events.push(SpeakerEvent { cycle: self.cycles, divisor: self.divisor });
        }
    }

    fn tick(&mut self, cycles: u32) {
        self.cycles += cycles as u64;
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Turns [`SpeakerEvent`]s into mono 16-bit PCM, as a stream: feed events with
/// [`push_events`](Self::push_events) and ask for samples up to a CPU cycle with
/// [`render_until`](Self::render_until). Phase is kept across calls, so rendering in chunks gives
/// exactly the same samples as rendering all at once.
pub struct ToneRenderer {
    sample_rate: u32,
    cpu_clock_hz: u64,
    tone_clock_hz: u32,
    amplitude: i16,
    /// Index of the next sample to produce.
    next_sample: u64,
    divisor: u16,
    /// Position within the square wave's period, 0.0..1.0 (first half high, second low).
    phase: f64,
    pending: VecDeque<SpeakerEvent>,
}

impl ToneRenderer {
    pub fn new(sample_rate: u32, cpu_clock_hz: u32, tone_clock_hz: u32) -> Self {
        ToneRenderer {
            sample_rate,
            cpu_clock_hz: cpu_clock_hz as u64,
            tone_clock_hz,
            amplitude: 8000,
            next_sample: 0,
            divisor: 0,
            phase: 0.0,
            pending: VecDeque::new(),
        }
    }

    /// Queue events (in cycle order) to be applied as rendering reaches their cycle.
    pub fn push_events(&mut self, events: &[SpeakerEvent]) {
        self.pending.extend(events.iter().copied());
    }

    /// Produce samples for every sample instant before CPU cycle `until_cycle`.
    pub fn render_until(&mut self, until_cycle: u64, out: &mut Vec<i16>) {
        let step = 1.0 / self.sample_rate as f64;
        loop {
            // The cycle at which the next sample is taken.
            let cycle = self.next_sample * self.cpu_clock_hz / self.sample_rate as u64;
            if cycle >= until_cycle {
                break;
            }
            while let Some(ev) = self.pending.front() {
                if ev.cycle <= cycle {
                    if ev.divisor != self.divisor {
                        self.phase = 0.0; // a new tone starts at the top of its wave
                    }
                    self.divisor = ev.divisor;
                    self.pending.pop_front();
                } else {
                    break;
                }
            }
            let hz = divisor_to_hz(self.divisor, self.tone_clock_hz);
            if hz == 0.0 {
                out.push(0);
            } else {
                out.push(if self.phase < 0.5 { self.amplitude } else { -self.amplitude });
                self.phase = (self.phase + hz * step).fract();
            }
            self.next_sample += 1;
        }
    }
}

/// A complete 16-bit mono PCM WAV file for `samples`.
pub fn wav_bytes(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    v.extend_from_slice(&2u16.to_le_bytes()); // block align
    v.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}
