//! End-to-end test of the speaker: assemble `speaker/melody.mol`, run it on the machine, and
//! check that the right notes were played at the right pitch and length, both from the captured
//! events and from the WAV file `cpt-run --speaker-wav` writes.

use cpt_machine::{Executive, Machine, MachineConfig, SpeakerDevice, SpeakerEvent, ToneRenderer};
use i8080_cpu::Cpu;
use std::process::Command;

const RATE: u32 = 22_050;
const DIVISORS: [u16; 9] = [1911, 1703, 1517, 1432, 1276, 1136, 1012, 956, 400];

fn assemble() -> mol80asm::codegen::Assembly {
    let path = format!("{}/../../speaker/melody.mol", env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {}", path, e));
    let out = mol80asm::assemble_source(&path, &src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.expect("melody should assemble")
}

/// Run the melody to its terminal HLT; returns the captured events and the total cycles.
fn play() -> (Vec<SpeakerEvent>, u64) {
    let cfg = MachineConfig::default();
    let prog = assemble();
    let mut machine = Machine::new(cfg.clone());
    machine.attach_device(Box::new(SpeakerDevice::new(cfg.speaker_base_port, cfg.speaker_tone_clock_hz)));
    machine.mem.load_bank0(prog.origin, &prog.bytes);
    let mut exec = Executive::new(Cpu::new(), machine);
    exec.run_until_halt(50_000_000);
    assert!(exec.is_terminally_halted(), "the melody should finish");
    let events = exec
        .machine
        .device_mut(0)
        .unwrap()
        .as_any_mut()
        .downcast_mut::<SpeakerDevice>()
        .unwrap()
        .take_events();
    (events, exec.cpu.cycles)
}

#[test]
fn plays_the_scale_then_a_beep_each_note_followed_by_silence() {
    let (events, _) = play();
    assert_eq!(events.len(), 18, "nine notes, each switched on and then off");
    for (i, &d) in DIVISORS.iter().enumerate() {
        assert_eq!(events[2 * i].divisor, d, "note {} divisor", i);
        assert_eq!(events[2 * i + 1].divisor, 0, "note {} is followed by silence", i);
    }
}

#[test]
fn each_note_is_a_quarter_second_with_a_short_gap() {
    let (events, _) = play();
    for i in 0..DIVISORS.len() {
        let held = events[2 * i + 1].cycle - events[2 * i].cycle;
        assert!((499_000..=502_000).contains(&held), "note {} held {} cycles (want ~500,000)", i, held);
        if i + 1 < DIVISORS.len() {
            let gap = events[2 * i + 2].cycle - events[2 * i + 1].cycle;
            assert!((59_000..=62_000).contains(&gap), "gap after note {}: {} cycles (want ~60,000)", i, gap);
        }
    }
}

#[test]
fn the_rendered_audio_has_the_right_pitches() {
    let cfg = MachineConfig::default();
    let (events, total) = play();
    let mut r = ToneRenderer::new(RATE, cfg.cpu_clock_hz, cfg.speaker_tone_clock_hz);
    r.push_events(&events);
    let mut samples = Vec::new();
    r.render_until(total, &mut samples);
    for (i, &d) in DIVISORS.iter().enumerate() {
        let start = (events[2 * i].cycle * RATE as u64 / cfg.cpu_clock_hz as u64) as usize;
        let end = (events[2 * i + 1].cycle * RATE as u64 / cfg.cpu_clock_hz as u64) as usize;
        let seg = &samples[start..end];
        let periods = seg.windows(2).filter(|w| w[0] <= 0 && w[1] > 0).count() as f64;
        let hz = periods / (seg.len() as f64 / RATE as f64);
        let want = cfg.speaker_tone_clock_hz as f64 / (2.0 * d as f64);
        assert!((hz - want).abs() / want < 0.02, "note {}: heard {:.1} Hz, wanted {:.1}", i, hz, want);
    }
}

#[test]
fn cpt_run_writes_a_wav_file_of_the_melody() {
    let dir = std::env::temp_dir();
    let bin = dir.join(format!("cpt_melody_{}.bin", std::process::id()));
    let wav = dir.join(format!("cpt_melody_{}.wav", std::process::id()));
    std::fs::write(&bin, assemble().bytes).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_cpt-run"))
        .args(["--steps", "5000000", "--speaker-wav"])
        .arg(&wav)
        .arg(&bin)
        .output()
        .expect("run cpt-run");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("18 tone change(s)"), "got {}", stdout);
    let w = std::fs::read(&wav).expect("the WAV should have been written");
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_file(&wav);
    assert_eq!(&w[0..4], b"RIFF");
    assert_eq!(&w[8..12], b"WAVE");
    let data_bytes = u32::from_le_bytes(w[40..44].try_into().unwrap()) as f64;
    let seconds = data_bytes / 2.0 / RATE as f64;
    // Nine notes at 250 ms plus eight 30 ms gaps, plus the last gap: about 2.5 s.
    assert!((2.4..=2.7).contains(&seconds), "melody lasts {:.2} s", seconds);
}
