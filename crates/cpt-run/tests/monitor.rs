//! End-to-end test of the MOL80 multitasking monitor (`monitor/monitor.mol`).
//!
//! The monitor is assembled with `mol80asm`, loaded at address 0 and run on the emulated machine
//! with the 100 Hz task-switch timer and a serial port. Tasks A and B print lines forever, task C
//! prints three lines and exits. Everything is judged from what appears on the serial port and
//! from the TCB table, stack free list and interrupt flag in the emulated machine.

use cpt_machine::{Executive, Machine, MachineConfig, SerialDevice, TimerDevice};
use i8080_cpu::{Bus, Cpu};

const TCB0: u16 = 0x0800;
const TCB_SIZE: u16 = 16;
const LOCKFLAG: u16 = 0x0C00;
const TICKS: u16 = 0x0C04;
const FREESTK: u16 = 0x0C06;
const SPAWNREQ: u16 = 0x0C0A;
const SPAWNQ: u16 = 0x0C0C;
const QUANTUM_OFF: u16 = 10; // offset of the quantum byte in a TCB
const DEFAULT_QUANTUM: u8 = 5;
const TASKE: u16 = 0x0E00;
const NSTK: usize = 4;

// TCB status codes.
const FREE: u8 = 0;
const READY: u8 = 1;
const BLOCKED: u8 = 2;

fn assemble() -> mol80asm::codegen::Assembly {
    let path = format!("{}/../../monitor/monitor.mol", env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {}", path, e));
    let out = mol80asm::assemble_source(&path, &src);
    assert!(out.diags.is_empty(), "{}", out.diags.render());
    out.assembly.expect("monitor should assemble")
}

struct Rig {
    exec: Executive,
    serial_dev: usize,
}

fn rig(lock_on: bool) -> Rig {
    let cfg = MachineConfig::default();
    let prog = assemble();
    assert_eq!(prog.origin, 0);
    let mut machine = Machine::new(cfg.clone());
    machine.attach_device(Box::new(TimerDevice::new(cfg.timer_period_cycles, cfg.timer_vector)));
    let serial_dev = machine.device_count();
    machine.attach_device(Box::new(SerialDevice::new(cfg.serial_base_port, None)));
    machine.mem.load_bank0(prog.origin, &prog.bytes);
    if !lock_on {
        machine.write(LOCKFLAG, 0);
    }
    let mut cpu = Cpu::new();
    cpu.pc = 0;
    cpu.sp = 0x7F00;
    Rig { exec: Executive::new(cpu, machine), serial_dev }
}

impl Rig {
    fn tx(&mut self) -> String {
        let i = self.serial_dev;
        let s = self
            .exec
            .machine
            .device_mut(i)
            .unwrap()
            .as_any_mut()
            .downcast_mut::<SerialDevice>()
            .unwrap();
        String::from_utf8_lossy(&s.take_tx()).into_owned()
    }
    fn word(&mut self, addr: u16) -> u16 {
        self.exec.machine.read(addr) as u16 | (self.exec.machine.read(addr + 1) as u16) << 8
    }
    fn set_word(&mut self, addr: u16, v: u16) {
        self.exec.machine.write(addr, v as u8);
        self.exec.machine.write(addr + 1, (v >> 8) as u8);
    }
    fn status(&mut self, slot: u16) -> u8 {
        self.exec.machine.read(TCB0 + slot * TCB_SIZE)
    }
    /// A task's quantum (ticks), read from its TCB.
    fn quantum(&mut self, slot: u16) -> u8 {
        self.exec.machine.read(TCB0 + slot * TCB_SIZE + QUANTUM_OFF)
    }
    /// Number of blocks on the stack free list.
    fn free_stacks(&mut self) -> usize {
        let mut n = 0;
        let mut p = self.word(FREESTK);
        while p != 0 {
            n += 1;
            assert!(n <= NSTK, "free list is longer than the pool: corrupted");
            p = self.word(p);
        }
        n
    }
    /// Run `budget` cycles, measuring how long interrupts stay disabled. Counting starts at the
    /// first moment interrupts are enabled, so the reset-time start-up is not included. Returns
    /// (cycles with interrupts disabled, cycles counted, longest single disabled window).
    fn run_measuring(&mut self, budget: u64) -> (u64, u64, u64) {
        let (mut off, mut total, mut longest, mut window) = (0u64, 0u64, 0u64, 0u64);
        let mut started = false;
        let start = self.exec.cpu.cycles;
        while self.exec.cpu.cycles - start < budget {
            let disabled = !self.exec.cpu.int_enabled;
            let c = self.exec.step() as u64;
            if !started {
                started = self.exec.cpu.int_enabled;
                continue;
            }
            total += c;
            if disabled {
                off += c;
                window += c;
                longest = longest.max(window);
            } else {
                window = 0;
            }
        }
        (off, total, longest)
    }
}

/// True if `line` is a complete line of one repeated letter.
fn uniform(line: &str, letter: char, len: usize) -> bool {
    line.len() == len && line.chars().all(|c| c == letter)
}

fn is_whole_task_line(l: &str) -> bool {
    uniform(l, 'A', 20) || uniform(l, 'B', 20) || uniform(l, 'C', 10) || uniform(l, 'E', 10)
}

fn is_system_line(l: &str) -> bool {
    l.is_empty() || l.starts_with("MOL80") || l == "task pool full"
}

#[test]
fn boots_prints_banner_and_reports_the_pool_full() {
    let mut r = rig(true);
    r.exec.run_cycles(5_000_000);
    let tx = r.tx();
    assert!(tx.starts_with("MOL80 monitor: 3 tasks\r\n"), "got {:?}", tx);
    assert!(tx.contains("task pool full\r\n"), "4th SPAWN should fail: {:?}", tx);
}

#[test]
fn tasks_share_the_cpu_fairly() {
    let mut r = rig(true);
    r.exec.run_cycles(20_000_000);
    let tx = r.tx();
    let a = tx.matches("AAAAAAAAAAAAAAAAAAAA\r\n").count();
    let b = tx.matches("BBBBBBBBBBBBBBBBBBBB\r\n").count();
    assert!(a >= 10 && b >= 10, "both tasks should make steady progress: A={} B={}", a, b);
    // Under the lock the two alternate by handoff, so their line counts stay level. A has a longer quantum, so it can pull slightly ahead; allow 10%.
    assert!((a as i64 - b as i64).abs() <= (a.max(b) as i64) / 10, "FIFO handoff keeps them level: A={} B={}", a, b);
}

#[test]
fn the_timer_drives_the_scheduler() {
    let mut r = rig(true);
    r.exec.run_cycles(10_000_000); // 5 s at 2 MHz -> ~500 ticks
    let ticks = r.word(TICKS);
    assert!((480..=500).contains(&ticks), "expected ~500 timer ticks, got {}", ticks);
}

#[test]
fn a_task_that_returns_is_destroyed_and_its_slot_freed() {
    let mut r = rig(true);
    r.exec.run_cycles(20_000_000);
    let tx = r.tx();
    assert_eq!(tx.matches("CCCCCCCCCC\r\n").count(), 3, "task C prints exactly three lines");
    // A live task is READY, or momentarily BLOCKED on the print lock when we sample it.
    assert_eq!(r.status(0), READY, "slot 0 (idle) is always ready");
    assert!([READY, BLOCKED].contains(&r.status(1)), "task A still alive, status {}", r.status(1));
    assert!([READY, BLOCKED].contains(&r.status(2)), "task B still alive, status {}", r.status(2));
    assert_eq!(r.status(3), FREE, "task C exited via SYSEXIT, so its slot is free again");
}

#[test]
fn stacks_come_from_a_pool_and_are_returned_when_a_task_exits() {
    let mut r = rig(true);
    r.exec.run_cycles(250_000); // start-up done (banner, 4 SPAWNs); A, B, C alive, none done
    // 4 blocks; A, B, C hold 3. The 4th SPAWN took the last block, found no TCB slot, and
    // handed the block back.
    assert_eq!(r.free_stacks(), NSTK - 3);
    r.exec.run_cycles(20_000_000); // C has exited
    assert_eq!(r.free_stacks(), NSTK - 2, "C's stack block went back on the free list");
}

#[test]
fn a_subroutine_can_be_launched_as_a_task_at_run_time() {
    let mut r = rig(true);
    r.exec.run_cycles(20_000_000); // C is gone; there is a free slot and a free stack
    let _ = r.tx();
    assert_eq!(r.status(3), FREE);
    // Point the launcher mailbox at a subroutine; the idle task SPAWNs it.
    r.set_word(SPAWNREQ, TASKE);
    r.exec.run_cycles(20_000_000);
    let tx = r.tx();
    assert_eq!(tx.matches("EEEEEEEEEE\r\n").count(), 2, "TASKE prints two lines then exits");
    assert_eq!(r.word(SPAWNREQ), 0, "mailbox cleared by the idle task");
    assert_eq!(r.status(3), FREE, "its slot is reclaimed again after it returns");
    assert_eq!(r.free_stacks(), NSTK - 2, "and so is its stack");
}

#[test]
fn the_semaphore_keeps_lines_whole() {
    let mut r = rig(true);
    r.exec.run_cycles(20_000_000);
    let tx = r.tx();
    let mut lines: Vec<&str> = tx.split("\r\n").collect();
    lines.pop(); // the trailing partial line
    let mut checked = 0;
    for l in lines {
        if is_system_line(l) {
            continue;
        }
        assert!(is_whole_task_line(l), "interleaved output with the lock on: {:?}", l);
        checked += 1;
    }
    assert!(checked > 20, "only {} lines checked", checked);
}

/// With the lock switched off, a task preempted mid-line lets the other one print into the
/// middle of it: proves the time slices really do land inside a line, so the test above is
/// meaningful.
#[test]
fn without_the_semaphore_lines_interleave() {
    let mut r = rig(false);
    r.exec.run_cycles(20_000_000);
    let tx = r.tx();
    let mut lines: Vec<&str> = tx.split("\r\n").collect();
    lines.pop();
    let broken = lines.iter().filter(|l| !is_system_line(l)).filter(|l| !is_whole_task_line(l)).count();
    assert!(broken > 0, "expected interleaved output without the lock; got {:?}", &tx[..tx.len().min(400)]);
}

/// Interrupts must stay enabled as much as possible. The only long disabled stretch should be
/// the context switch itself; spawning, printing and everything the tasks do run with
/// interrupts on.
#[test]
fn interrupts_are_disabled_only_briefly() {
    let mut r = rig(true);
    r.exec.run_cycles(200_000); // get past start-up
    let (off, total, longest) = r.run_measuring(4_000_000);
    let pct = off as f64 * 100.0 / total as f64;
    eprintln!("interrupts off {:.2}% of the time; longest disabled window {} cycles", pct, longest);
    assert!(pct < 3.0, "interrupts were off {:.2}% of the time", pct);
    assert!(longest < 800, "longest window with interrupts disabled: {} cycles", longest);
}

/// The same while tasks are being created and destroyed and a semaphore is contended.
#[test]
fn interrupts_stay_enabled_while_spawning() {
    let mut r = rig(true);
    r.exec.run_cycles(20_000_000); // C has exited
    r.set_word(SPAWNREQ, TASKE);
    let (off, total, longest) = r.run_measuring(8_000_000);
    let pct = off as f64 * 100.0 / total as f64;
    eprintln!("with spawn/exit: off {:.2}%; longest window {} cycles", pct, longest);
    assert!(pct < 3.0, "interrupts were off {:.2}% of the time", pct);
    assert!(longest < 800, "longest window with interrupts disabled: {} cycles", longest);
}

// ---- per-task quanta -------------------------------------------------------------------

#[test]
fn spawn_records_each_tasks_quantum_and_zero_means_the_default() {
    let mut r = rig(true);
    r.exec.run_cycles(250_000); // start-up done: A, B, C spawned with 10, 5, 0
    assert_eq!(r.quantum(1), 10, "task A was spawned with quantum 10");
    assert_eq!(r.quantum(2), 5, "task B was spawned with quantum 5");
    assert_eq!(r.quantum(3), DEFAULT_QUANTUM, "task C asked for 0, so it gets the default");
    assert_eq!(r.quantum(0), DEFAULT_QUANTUM, "the idle task has the default");
}

/// With the print lock off, A, B and C never wait for each other, so they are all CPU-bound
/// and each character on the serial port is a unit of CPU time. A has a quantum of 10 ticks
/// and B of 5, so A should get about twice as much CPU, and print about twice the characters.
#[test]
fn cpu_share_follows_the_quantum() {
    let mut r = rig(false);
    r.exec.run_cycles(30_000_000);
    let tx = r.tx();
    let a = tx.chars().filter(|&c| c == 'A').count() as f64;
    let b = tx.chars().filter(|&c| c == 'B').count() as f64;
    let ratio = a / b;
    eprintln!("A={} B={} ratio={:.2}", a, b, ratio);
    assert!(b > 200.0, "B printed only {} characters", b);
    assert!((1.7..=2.3).contains(&ratio), "expected A:B near 2:1 for quanta 10:5, got {:.2}", ratio);
}

/// A task can change its own quantum with SETQUANT, and SPAWN's mailbox takes a quantum too.
#[test]
fn a_task_can_change_its_quantum_with_setquant() {
    let mut r = rig(true);
    r.exec.run_cycles(20_000_000); // C is gone; slot 3 is free
    let _ = r.tx();
    r.set_word(SPAWNREQ, TASKE);
    r.exec.machine.write(SPAWNQ, 3);
    // Wait for the spawn, and check it got the requested quantum.
    let mut spawned = false;
    for _ in 0..400 {
        r.exec.run_cycles(10_000);
        if r.status(3) != FREE {
            spawned = true;
            break;
        }
    }
    assert!(spawned, "TASKE was never spawned");
    let first = r.quantum(3);
    assert!(first == 3 || first == 20, "spawned with quantum 3 (or already raised): got {}", first);
    // Then it raises its own quantum to 20 while still alive.
    let mut raised = false;
    for _ in 0..600 {
        r.exec.run_cycles(10_000);
        if r.quantum(3) == 20 && r.status(3) != FREE {
            raised = true;
            break;
        }
    }
    assert!(raised, "TASKE never raised its quantum to 20 (now {})", r.quantum(3));
}
