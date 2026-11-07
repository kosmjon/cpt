//! The single-channel speaker: port interface, event capture, PCM rendering, WAV output.

use cpt_machine::speaker::{divisor_to_hz, hz_to_divisor};
use cpt_machine::{
    wav_bytes, Device, Executive, Machine, MachineConfig, SpeakerDevice, SpeakerEvent, ToneRenderer,
};
use i8080_cpu::Cpu;

const BASE: u8 = 0x80;
const TONE_CLOCK: u32 = 1_000_000;
const CPU_CLOCK: u32 = 2_000_000;
const RATE: u32 = 22_050;

fn ev(cycle: u64, divisor: u16) -> SpeakerEvent {
    SpeakerEvent { cycle, divisor }
}

/// Count the rising edges (negative to positive transitions) in `samples`: the number of
/// complete periods of a square wave.
fn rising_edges(samples: &[i16]) -> usize {
    samples.windows(2).filter(|w| w[0] <= 0 && w[1] > 0).count()
}

fn render(events: &[SpeakerEvent], until_cycle: u64) -> Vec<i16> {
    let mut r = ToneRenderer::new(RATE, CPU_CLOCK, TONE_CLOCK);
    r.push_events(events);
    let mut out = Vec::new();
    r.render_until(until_cycle, &mut out);
    out
}

#[test]
fn the_low_byte_is_latched_and_the_high_byte_commits_the_tone() {
    let mut s = SpeakerDevice::new(BASE, TONE_CLOCK);
    s.io_out(BASE, 0x70);
    assert_eq!(s.divisor(), 0, "writing the low byte alone changes nothing audible");
    assert!(s.take_events().is_empty());
    s.io_out(BASE + 1, 0x04);
    assert_eq!(s.divisor(), 0x0470, "high byte commits {{high, latched low}}");
    assert!(s.is_on());
    assert_eq!(s.take_events(), vec![ev(0, 0x0470)]);
}

#[test]
fn divisor_zero_is_silence_and_status_reads_back() {
    let mut s = SpeakerDevice::new(BASE, TONE_CLOCK);
    assert_eq!(s.io_in(BASE), 0);
    s.io_out(BASE, 0x70);
    s.io_out(BASE + 1, 0x04);
    assert_eq!(s.io_in(BASE), 1, "status bit 0 = tone sounding");
    s.io_out(BASE, 0);
    s.io_out(BASE + 1, 0);
    assert!(!s.is_on());
    assert_eq!(s.frequency_hz(), 0.0);
    assert_eq!(s.io_in(BASE), 0);
}

#[test]
fn claims_two_ports() {
    let s = SpeakerDevice::new(BASE, TONE_CLOCK);
    assert_eq!(s.ports(), &[BASE, BASE + 1]);
}

#[test]
fn frequency_is_the_tone_clock_over_twice_the_divisor() {
    // A440 on a 1 MHz tone clock is divisor 1136 (1,000,000 / 2 / 440 = 1136.4).
    assert_eq!(hz_to_divisor(440.0, TONE_CLOCK), 1136);
    let hz = divisor_to_hz(1136, TONE_CLOCK);
    assert!((hz - 440.0).abs() < 0.5, "got {}", hz);
    assert_eq!(divisor_to_hz(0, TONE_CLOCK), 0.0);
}

#[test]
fn events_are_stamped_with_the_cpu_cycle() {
    let mut s = SpeakerDevice::new(BASE, TONE_CLOCK);
    s.tick(100);
    s.io_out(BASE, 1);
    s.io_out(BASE + 1, 0);
    s.tick(250);
    s.io_out(BASE, 0);
    s.io_out(BASE + 1, 0);
    assert_eq!(s.take_events(), vec![ev(100, 1), ev(350, 0)]);
    assert!(s.take_events().is_empty(), "events are taken once");
}

#[test]
fn a_440_hz_tone_renders_440_periods_per_second() {
    let samples = render(&[ev(0, 1136)], CPU_CLOCK as u64); // one second
    assert_eq!(samples.len(), RATE as usize);
    let periods = rising_edges(&samples);
    assert!((438..=442).contains(&periods), "got {} periods", periods);
    assert!(samples.iter().all(|&s| s == 8000 || s == -8000), "a square wave");
}

#[test]
fn silence_renders_as_zero_and_a_tone_can_be_switched_off() {
    // Tone for the first half second, then off.
    let samples = render(&[ev(0, 1136), ev(1_000_000, 0)], CPU_CLOCK as u64);
    let half = samples.len() / 2;
    assert!(samples[..half].iter().any(|&s| s != 0));
    assert!(samples[half + 2..].iter().all(|&s| s == 0), "silent after the tone is switched off");
    // Before any event there is silence too.
    assert!(render(&[], 100_000).iter().all(|&s| s == 0));
}

#[test]
fn rendering_in_chunks_matches_rendering_at_once() {
    let events = [ev(0, 1136), ev(300_000, 956), ev(900_000, 0), ev(1_100_000, 1703)];
    let whole = render(&events, 2_000_000);
    let mut r = ToneRenderer::new(RATE, CPU_CLOCK, TONE_CLOCK);
    let mut chunked = Vec::new();
    // Feed the events as they would arrive, in uneven frames.
    let mut fed = 0;
    for until in [150_000u64, 333_333, 700_000, 1_050_000, 1_400_000, 2_000_000] {
        while fed < events.len() && events[fed].cycle < until {
            r.push_events(&events[fed..=fed]);
            fed += 1;
        }
        r.render_until(until, &mut chunked);
    }
    assert_eq!(chunked, whole);
}

#[test]
fn wav_file_has_a_valid_header() {
    let w = wav_bytes(&[1, -2, 3], RATE);
    assert_eq!(w.len(), 44 + 6);
    assert_eq!(&w[0..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 6);
    assert_eq!(&w[8..16], b"WAVEfmt ");
    assert_eq!(u16::from_le_bytes(w[20..22].try_into().unwrap()), 1, "PCM");
    assert_eq!(u16::from_le_bytes(w[22..24].try_into().unwrap()), 1, "mono");
    assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), RATE);
    assert_eq!(&w[36..40], b"data");
    assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 6);
    assert_eq!(i16::from_le_bytes(w[46..48].try_into().unwrap()), -2);
}

/// End to end: the CPU programs a tone with two OUTs, waits, then silences the speaker; the
/// events carry the exact cycle at which each OUT executed.
#[test]
fn cpu_programs_a_tone_through_the_ports() {
    let cfg = MachineConfig::default();
    let mut m = Machine::new(cfg.clone());
    m.attach_device(Box::new(SpeakerDevice::new(cfg.speaker_base_port, cfg.speaker_tone_clock_hz)));
    let program = [
        0x3E, 0x70, //       MVI A,70H        7 cycles
        0xD3, 0x80, //       OUT 80H          10  (divisor low)
        0x3E, 0x04, //       MVI A,04H        7
        0xD3, 0x81, //       OUT 81H          10  (divisor high: commit 0470H = 1136)
        0x01, 0xE8, 0x03, // LXI B,1000       10
        0x0B, //             DCX B            5   <- loop of 24 cycles x 1000
        0x78, //             MOV A,B          5
        0xB1, //             ORA C            4
        0xC2, 0x0B, 0x00, // JNZ 000BH        10
        0xAF, //             XRA A            4
        0xD3, 0x80, //       OUT 80H          10
        0xD3, 0x81, //       OUT 81H          10  (divisor 0: silence)
        0x76, //             HLT              (interrupts are off, so this is terminal)
    ];
    m.mem.load_bank0(0, &program);
    let mut exec = Executive::new(Cpu::new(), m);
    exec.run_until_halt(1_000_000);
    let events = exec
        .machine
        .device_mut(0)
        .unwrap()
        .as_any_mut()
        .downcast_mut::<SpeakerDevice>()
        .unwrap()
        .take_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], ev(7 + 10 + 7, 0x0470), "committed by the second OUT, 24 cycles in");
    assert_eq!(events[1].divisor, 0);
    let held = events[1].cycle - events[0].cycle;
    assert!((24_000..26_000).contains(&held), "tone held about 24k cycles, got {}", held);
}
