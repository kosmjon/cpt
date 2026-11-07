//! End-to-end tests of the PL/M-80 version of the multitasking monitor (`monitor/monitor.plm`),
//! the same checks as `monitor.rs` makes of the MOL80 version.
//!
//! The binary is built by `monitor/build-plm.ps1` with the third-party Intel PL/M-80 toolchain,
//! which `cargo test` cannot build itself. If `monitor/plm-build/monitor.bin` is missing these
//! tests print a note and pass without running.

use cpt_machine::{Executive, Machine, MachineConfig, SerialDevice, TimerDevice};
use i8080_cpu::{Bus, Cpu};

const TCB0: u16 = 0x0800;
const TCB_SIZE: u16 = 16;
const QUANTUM_OFF: u16 = 10;
const LOCKOFF: u16 = 0x0C00; // nonzero = lock off (the reverse of the MOL80 version's LOCKFLAG)
const TICKS: u16 = 0x0C04;
const FREESTK: u16 = 0x0C06;
const SPAWNREQ: u16 = 0x0C0A;
const SPAWNQ: u16 = 0x0C0C;
const ENTRY_TBL: u16 = 0x0C20; // (0) = TASKE's address, set at start-up
const NSTK: usize = 4;
const DEFAULT_QUANTUM: u8 = 5;

const FREE: u8 = 0;
const READY: u8 = 1;
const BLOCKED: u8 = 2;

fn binary() -> Option<Vec<u8>> {
    let path = format!("{}/../../monitor/plm-build/monitor.bin", env!("CARGO_MANIFEST_DIR"));
    match std::fs::read(&path) {
        Ok(b) => Some(b),
        Err(_) => {
            eprintln!("skipping: run monitor/build-plm.ps1 to build {}", path);
            None
        }
    }
}

struct Rig {
    exec: Executive,
    serial_dev: usize,
}

fn rig(bin: &[u8], lock_on: bool) -> Rig {
    let cfg = MachineConfig::default();
    let mut machine = Machine::new(cfg.clone());
    machine.attach_device(Box::new(TimerDevice::new(cfg.timer_period_cycles, cfg.timer_vector)));
    let serial_dev = machine.device_count();
    machine.attach_device(Box::new(SerialDevice::new(cfg.serial_base_port, None)));
    machine.mem.load_bank0(0, bin);
    if !lock_on {
        machine.write(LOCKOFF, 1);
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
    fn quantum(&mut self, slot: u16) -> u8 {
        self.exec.machine.read(TCB0 + slot * TCB_SIZE + QUANTUM_OFF)
    }
    /// Run until start-up has spawned A, B and C and the refused 4th SPAWN has handed its stack
    /// back (compiled start-up is slower than the MOL80 one, so wait rather than guess a time).
    fn run_until_started(&mut self) {
        for _ in 0..1000 {
            self.exec.run_cycles(5_000);
            let all = (1..4).all(|s| self.status(s) != FREE);
            if all && self.free_stacks() == NSTK - 3 {
                return;
            }
        }
        panic!("start-up never spawned A, B and C");
    }
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
    /// (cycles with interrupts disabled, cycles counted, longest disabled window), counting
    /// from the first moment interrupts are enabled.
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

fn uniform(line: &str, letter: char, len: usize) -> bool {
    line.len() == len && line.chars().all(|c| c == letter)
}
fn is_whole_task_line(l: &str) -> bool {
    uniform(l, 'A', 20) || uniform(l, 'B', 20) || uniform(l, 'C', 10) || uniform(l, 'E', 10)
}
fn is_system_line(l: &str) -> bool {
    l.is_empty() || l.starts_with("PL/M monitor") || l == "task pool full"
}

#[test]
fn boots_prints_banner_and_reports_the_pool_full() {
    let Some(bin) = binary() else { return };
    let mut r = rig(&bin, true);
    r.exec.run_cycles(5_000_000);
    let tx = r.tx();
    assert!(tx.starts_with("PL/M monitor: 3 tasks\r\n"), "got {:?}", &tx[..tx.len().min(200)]);
    assert!(tx.contains("task pool full\r\n"), "4th SPAWN should fail");
}

#[test]
fn the_timer_drives_the_scheduler() {
    let Some(bin) = binary() else { return };
    let mut r = rig(&bin, true);
    r.exec.run_cycles(10_000_000); // 5 s at 2 MHz
    let ticks = r.word(TICKS);
    assert!((480..=500).contains(&ticks), "expected ~500 timer ticks, got {}", ticks);
}

#[test]
fn a_task_that_returns_is_destroyed_and_its_slot_and_stack_freed() {
    let Some(bin) = binary() else { return };
    let mut r = rig(&bin, true);
    r.run_until_started();
    assert_eq!(r.free_stacks(), NSTK - 3, "A, B, C hold three stacks; the 4th was handed back");
    r.exec.run_cycles(20_000_000);
    let tx = r.tx();
    assert_eq!(tx.matches("CCCCCCCCCC\r\n").count(), 3, "task C prints exactly three lines");
    assert_eq!(r.status(0), READY);
    assert!([READY, BLOCKED].contains(&r.status(1)));
    assert!([READY, BLOCKED].contains(&r.status(2)));
    assert_eq!(r.status(3), FREE, "C exited via SYSEXIT, so its slot is free again");
    assert_eq!(r.free_stacks(), NSTK - 2, "and its stack is back on the free list");
}

#[test]
fn the_semaphore_keeps_lines_whole() {
    let Some(bin) = binary() else { return };
    let mut r = rig(&bin, true);
    r.exec.run_cycles(20_000_000);
    let tx = r.tx();
    let mut lines: Vec<&str> = tx.split("\r\n").collect();
    lines.pop();
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

#[test]
fn without_the_semaphore_lines_interleave() {
    let Some(bin) = binary() else { return };
    let mut r = rig(&bin, false);
    r.exec.run_cycles(20_000_000);
    let tx = r.tx();
    let mut lines: Vec<&str> = tx.split("\r\n").collect();
    lines.pop();
    let broken = lines.iter().filter(|l| !is_system_line(l)).filter(|l| !is_whole_task_line(l)).count();
    assert!(broken > 0, "expected interleaved output without the lock");
}

#[test]
fn spawn_records_each_tasks_quantum() {
    let Some(bin) = binary() else { return };
    let mut r = rig(&bin, true);
    r.run_until_started();
    assert_eq!(r.quantum(1), 10);
    assert_eq!(r.quantum(2), 5);
    assert_eq!(r.quantum(3), DEFAULT_QUANTUM, "0 means the default");
}

#[test]
fn cpu_share_follows_the_quantum() {
    let Some(bin) = binary() else { return };
    let mut r = rig(&bin, false);
    r.exec.run_cycles(30_000_000);
    let tx = r.tx();
    let a = tx.chars().filter(|&c| c == 'A').count() as f64;
    let b = tx.chars().filter(|&c| c == 'B').count() as f64;
    let ratio = a / b;
    eprintln!("A={} B={} ratio={:.2}", a, b, ratio);
    assert!(b > 200.0, "B printed only {}", b);
    assert!((1.7..=2.3).contains(&ratio), "expected A:B near 2:1, got {:.2}", ratio);
}

#[test]
fn a_subroutine_can_be_launched_at_run_time_and_set_its_quantum() {
    let Some(bin) = binary() else { return };
    let mut r = rig(&bin, true);
    r.exec.run_cycles(20_000_000);
    let _ = r.tx();
    assert_eq!(r.status(3), FREE);
    let task_e = r.word(ENTRY_TBL);
    assert!(task_e >= 0x2000, "TASKE address published at start-up: {:04X}", task_e);
    r.exec.machine.write(SPAWNQ, 3);
    r.set_word(SPAWNREQ, task_e);
    let mut raised = false;
    for _ in 0..600 {
        r.exec.run_cycles(10_000);
        if r.status(3) != FREE && r.quantum(3) == 20 {
            raised = true;
            break;
        }
    }
    assert!(raised, "TASKE should run and SETQUANT itself to 20 (now {})", r.quantum(3));
    r.exec.run_cycles(20_000_000);
    let tx = r.tx();
    assert_eq!(tx.matches("EEEEEEEEEE\r\n").count(), 2, "TASKE prints two lines then exits");
    assert_eq!(r.word(SPAWNREQ), 0, "mailbox cleared");
    assert_eq!(r.status(3), FREE, "slot reclaimed after it returns");
    assert_eq!(r.free_stacks(), NSTK - 2, "and its stack");
}

#[test]
fn interrupts_are_disabled_only_briefly() {
    let Some(bin) = binary() else { return };
    let mut r = rig(&bin, true);
    r.exec.run_cycles(200_000);
    let (off, total, longest) = r.run_measuring(4_000_000);
    let pct = off as f64 * 100.0 / total as f64;
    eprintln!("PL/M: interrupts off {:.2}% of the time; longest window {} cycles", pct, longest);
    assert!(pct < 5.0, "interrupts were off {:.2}% of the time", pct);
    // The compiled TICK (switch + scheduler scan + reap) is slower than the hand-written MOL80
    // one (653 cycles): about 1,700 cycles, under a tenth of a 20,000-cycle tick.
    assert!(longest < 2000, "longest window with interrupts disabled: {} cycles", longest);
}
