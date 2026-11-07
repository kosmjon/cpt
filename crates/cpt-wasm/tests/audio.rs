//! The wasm module's speaker interface, exercised natively through the same `extern "C"`
//! functions the page calls: start the melody, run frames, and read the event buffer.

use cpt_wasm::*;

/// Run frames until the melody has finished; returns every (cycle, divisor) event seen, as the
/// page would read them frame by frame.
fn play_melody() -> Vec<(f64, u16)> {
    cpt_init();
    assert_eq!(cpt_play_melody(), 1, "the melody should assemble inside the module");
    let mut events = Vec::new();
    for _ in 0..400 {
        cpt_run_frame();
        let n = cpt_speaker_count();
        let p = cpt_speaker_ptr();
        for i in 0..n {
            let (cycle, div) = unsafe { (*p.add(2 * i), *p.add(2 * i + 1)) };
            events.push((cycle, div as u16));
        }
    }
    events
}

#[test]
fn clocks_are_exported_for_the_page() {
    cpt_init();
    assert_eq!(cpt_cpu_hz(), 2_000_000.0);
    assert_eq!(cpt_tone_hz(), 1_000_000.0);
}

#[test]
fn the_melody_arrives_frame_by_frame_as_cycle_divisor_pairs() {
    let ev = play_melody();
    let divisors: Vec<u16> = ev.iter().map(|e| e.1).collect();
    assert_eq!(
        divisors,
        vec![1911, 0, 1703, 0, 1517, 0, 1432, 0, 1276, 0, 1136, 0, 1012, 0, 956, 0, 400, 0]
    );
    // Cycles only ever increase, and each note is held about a quarter second (500,000 cycles).
    assert!(ev.windows(2).all(|w| w[0].0 < w[1].0));
    let held = ev[1].0 - ev[0].0;
    assert!((499_000.0..=502_000.0).contains(&held), "held {} cycles", held);
}

#[test]
fn each_frame_hands_over_only_its_own_events() {
    cpt_init();
    cpt_play_melody();
    cpt_run_frame(); // the first frame (33k cycles) starts the first note
    assert_eq!(cpt_speaker_count(), 1);
    cpt_run_frame(); // nothing new happens in the second
    assert_eq!(cpt_speaker_count(), 0, "the buffer is replaced each frame, not appended to");
}

#[test]
fn the_cycle_clock_keeps_running_across_the_switch_to_the_melody() {
    cpt_init();
    for _ in 0..5 {
        cpt_run_frame();
    }
    let before = cpt_cycle_now();
    assert!(before > 100_000.0);
    cpt_play_melody();
    cpt_run_frame();
    let ev_cycle = unsafe { *cpt_speaker_ptr() };
    assert!(ev_cycle > before, "event cycle {} should continue from {}", ev_cycle, before);
}

#[test]
fn nothing_plays_by_default() {
    cpt_init();
    for _ in 0..50 {
        cpt_run_frame();
        assert_eq!(cpt_speaker_count(), 0, "the screen demo makes no sound");
    }
}
