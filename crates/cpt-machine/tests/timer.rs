//! The task-switch timer: a fixed 100 Hz, non-programmable periodic interrupt, independent of
//! the display's vertical retrace.

use cpt_machine::{Device, DisplayDevice, Executive, Machine, MachineConfig, TimerDevice};
use i8080_cpu::{Bus, Cpu};

fn cfg() -> MachineConfig {
    MachineConfig::default()
}

#[test]
fn raises_an_interrupt_every_period_and_not_before() {
    let c = cfg();
    let mut t = TimerDevice::new(c.timer_period_cycles, c.timer_vector);
    t.tick(c.timer_period_cycles - 1);
    assert_eq!(t.take_irq(), None, "no interrupt before a full period");
    t.tick(1);
    assert_eq!(t.take_irq(), Some(c.timer_vector));
    assert_eq!(t.take_irq(), None, "the request is cleared once taken");
    assert_eq!(t.tick_count, 1);
}

#[test]
fn default_rate_is_100_hz_on_a_2mhz_clock() {
    let c = cfg();
    assert_eq!(c.timer_period_cycles, 2_000_000 / 100);
    let mut t = TimerDevice::new(c.timer_period_cycles, c.timer_vector);
    let mut fired = 0;
    // One second of CPU time, fed in instruction-sized steps.
    for _ in 0..(2_000_000 / 4) {
        t.tick(4);
        if t.take_irq().is_some() {
            fired += 1;
        }
    }
    assert_eq!(fired, 100);
}

#[test]
fn ticks_that_pile_up_coalesce_into_one_interrupt() {
    let mut t = TimerDevice::new(100, 4);
    t.tick(350); // three periods elapse before the CPU takes anything
    assert_eq!(t.take_irq(), Some(4));
    assert_eq!(t.take_irq(), None, "an interrupt line, not a queue");
}

#[test]
fn is_not_programmable_it_has_no_ports() {
    let mut t = TimerDevice::new(100, 4);
    assert!(t.ports().is_empty());
    // Port traffic cannot disturb it.
    t.io_out(0x10, 0xFF);
    assert_eq!(t.io_in(0x10), 0);
    t.tick(100);
    assert_eq!(t.take_irq(), Some(4));
}

/// Install a handler at `vector * 8` that increments the byte at `counter` and returns.
fn install_counting_handler(m: &mut Machine, vector: u8, counter: u16) {
    let at = vector as u16 * 8;
    let code = [
        0x21, (counter & 0xFF) as u8, (counter >> 8) as u8, // LXI H,counter
        0x34, // INR M
        0xFB, // EI
        0xC9, // RET
    ];
    m.mem.load_bank0(at, &code);
}

/// End to end: the CPU idles with interrupts enabled; the RST-4 handler counts timer ticks.
/// One second of CPU time must give 100 task-switch ticks.
#[test]
fn cpu_counts_100_ticks_per_second() {
    let c = cfg();
    let mut m = Machine::new(c.clone());
    m.attach_device(Box::new(TimerDevice::new(c.timer_period_cycles, c.timer_vector)));
    install_counting_handler(&mut m, c.timer_vector, 0x0200);
    // 0x0100: EI ; JMP 0x0101 (spin with interrupts on)
    m.mem.load_bank0(0x0100, &[0xFB, 0xC3, 0x01, 0x01]);
    let mut cpu = Cpu::new();
    cpu.pc = 0x0100;
    cpu.sp = 0x7F00;
    let mut exec = Executive::new(cpu, m);
    exec.run_cycles(2_000_000);
    let ticks = exec.machine.read(0x0200);
    assert!((99..=100).contains(&ticks), "expected ~100 ticks in a second, got {}", ticks);
}

/// The timer and the display retrace are separate, independent interrupt sources.
#[test]
fn timer_and_retrace_fire_independently_on_their_own_vectors() {
    let c = cfg();
    assert_ne!(c.timer_vector, c.retrace_vector);
    let mut m = Machine::new(c.clone());
    m.attach_display(DisplayDevice::new(c.retrace_period_cycles, c.retrace_vector));
    m.attach_device(Box::new(TimerDevice::new(c.timer_period_cycles, c.timer_vector)));
    install_counting_handler(&mut m, c.retrace_vector, 0x0200);
    install_counting_handler(&mut m, c.timer_vector, 0x0201);
    m.mem.load_bank0(0x0100, &[0xFB, 0xC3, 0x01, 0x01]);
    let mut cpu = Cpu::new();
    cpu.pc = 0x0100;
    cpu.sp = 0x7F00;
    let mut exec = Executive::new(cpu, m);
    exec.run_cycles(2_000_000); // one second
    let retraces = exec.machine.read(0x0200);
    let ticks = exec.machine.read(0x0201);
    assert!((59..=60).contains(&retraces), "retrace ~60 Hz, got {}", retraces);
    assert!((99..=100).contains(&ticks), "timer ~100 Hz, got {}", ticks);
}

/// The INT line is level-sensitive: a tick that arrives while interrupts are disabled stays
/// pending (coalescing with later ones) and is delivered as soon as the CPU enables them.
#[test]
fn a_masked_tick_stays_pending_until_interrupts_are_enabled() {
    let c = cfg();
    let mut m = Machine::new(c.clone());
    m.attach_device(Box::new(TimerDevice::new(c.timer_period_cycles, c.timer_vector)));
    install_counting_handler(&mut m, c.timer_vector, 0x0200);
    // 0x0100: DI ; 0x0101: JMP 0x0101 (spin with interrupts masked)
    m.mem.load_bank0(0x0100, &[0xF3, 0xC3, 0x01, 0x01]);
    let mut cpu = Cpu::new();
    cpu.pc = 0x0100;
    cpu.sp = 0x7F00;
    let mut exec = Executive::new(cpu, m);
    exec.run_cycles(50_000); // two and a half timer periods pass while masked
    assert_eq!(exec.machine.read(0x0200), 0, "no handler runs while interrupts are masked");
    // Patch the spin loop into `EI ; JMP 0x0102` (the CPU is parked on the JMP at 0x0101).
    exec.machine.mem.load_bank0(0x0101, &[0xFB, 0xC3, 0x02, 0x01]);
    exec.run_cycles(200);
    assert_eq!(
        exec.machine.read(0x0200),
        1,
        "the pending tick (coalesced) is delivered once interrupts are enabled"
    );
}

/// Two periodic sources whose periods nearly coincide (retrace 33,333 and timer 20,000 cycles
/// meet every ~100,000 cycles) must both be serviced when they land on the same instruction:
/// the second waits for the first handler to finish instead of being dropped.
#[test]
fn simultaneous_sources_are_both_serviced() {
    let c = cfg();
    let mut m = Machine::new(c.clone());
    // Periods chosen to coincide exactly every 3 and 5 steps' worth of cycles.
    m.attach_display(DisplayDevice::new(30_000, c.retrace_vector));
    m.attach_device(Box::new(TimerDevice::new(20_000, c.timer_vector)));
    install_counting_handler(&mut m, c.retrace_vector, 0x0200);
    install_counting_handler(&mut m, c.timer_vector, 0x0201);
    m.mem.load_bank0(0x0100, &[0xFB, 0xC3, 0x01, 0x01]);
    let mut cpu = Cpu::new();
    cpu.pc = 0x0100;
    cpu.sp = 0x7F00;
    let mut exec = Executive::new(cpu, m);
    exec.run_cycles(600_000);
    assert!((19..=20).contains(&exec.machine.read(0x0200)), "retrace 600000/30000 = 20");
    assert!((29..=30).contains(&exec.machine.read(0x0201)), "timer 600000/20000 = 30");
}
