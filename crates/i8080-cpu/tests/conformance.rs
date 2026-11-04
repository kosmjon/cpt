//! Exhaustive conformance tests using the classic CP/M 8080 diagnostic programs.
//!
//! These public-domain `.COM` programs (in `tests/roms/`) are the gold standard for 8080
//! emulator validation — they exercise every instruction and flag and print a success or
//! failure message via CP/M console calls. We run them under a minimal CP/M harness:
//!
//!   * Program loads at 0x0100 and starts there.
//!   * A `CALL 5` (CP/M BDOS entry at 0x0005) is intercepted:
//!       - function 2 (C=2): print the character in E.
//!       - function 9 (C=9): print the `$`-terminated string at DE.
//!   * The program terminates by returning to 0x0000 (we seed the stack / detect PC==0).
//!
//! A test passes if the program completes and its captured output does NOT contain a failure
//! marker (and, for the ones that print it, contains the success phrase).

use i8080_cpu::{Bus, Cpu, FlatMemory};

/// Run a CP/M `.COM` image under the minimal harness, returning the captured console output.
fn run_cpm(rom: &[u8], max_cycles: u64) -> String {
    let mut cpu = Cpu::new();
    let mut mem = FlatMemory::new();

    // CP/M programs are loaded at 0x0100 and expect execution to start there.
    mem.load(0x0100, rom);
    cpu.pc = 0x0100;
    cpu.sp = 0xF000; // a sane stack well out of the program's way

    // Put a RET at the BDOS entry (0x0005) so an un-intercepted CALL 5 returns cleanly, and
    // a JMP 0x0000 convention: many of these tests `JMP 0` to exit. We detect PC == 0.
    mem.mem[0x0005] = 0xC9; // RET
    mem.mem[0x0000] = 0x76; // HLT as a backstop at the warm-boot vector

    let mut out = String::new();

    loop {
        // Intercept the BDOS call by PC: when about to execute at 0x0005, service it.
        if cpu.pc == 0x0005 {
            match cpu.c {
                2 => {
                    // Print character in E.
                    out.push(cpu.e as char);
                }
                9 => {
                    // Print `$`-terminated string at DE.
                    let mut addr = cpu.de();
                    loop {
                        let ch = mem.read(addr);
                        if ch == b'$' {
                            break;
                        }
                        out.push(ch as char);
                        addr = addr.wrapping_add(1);
                    }
                }
                _ => {}
            }
            // Perform the RET that the BDOS stub would do.
            cpu.pc = cpu.step_ret_hack(&mut mem);
            continue;
        }

        // Termination: CP/M programs exit by jumping to 0x0000 (warm boot).
        if cpu.pc == 0x0000 {
            break;
        }

        cpu.step(&mut mem);

        if cpu.cycles > max_cycles {
            panic!(
                "test did not terminate within {} cycles; output so far:\n{}",
                max_cycles, out
            );
        }
    }

    out
}

// Small extension used only by the harness: execute a RET to unwind the BDOS call frame.
trait RetHack {
    fn step_ret_hack(&mut self, bus: &mut dyn Bus) -> u16;
}
impl RetHack for Cpu {
    fn step_ret_hack(&mut self, bus: &mut dyn Bus) -> u16 {
        // Pop the return address (the CALL 5 pushed it) and return it as the new PC.
        let lo = bus.read(self.sp) as u16;
        let hi = bus.read(self.sp.wrapping_add(1)) as u16;
        self.sp = self.sp.wrapping_add(2);
        (hi << 8) | lo
    }
}

fn load(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/roms/{}", env!("CARGO_MANIFEST_DIR"), name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {}", path, e))
}

#[test]
fn test_8080pre() {
    let out = run_cpm(&load("8080PRE.COM"), 1_000_000);
    println!("8080PRE output: {:?}", out);
    assert!(
        out.contains("8080 Preliminary tests complete"),
        "8080PRE did not report success; got: {:?}",
        out
    );
}

#[test]
fn test_tst8080() {
    let out = run_cpm(&load("TST8080.COM"), 10_000_000);
    println!("TST8080 output: {}", out);
    assert!(
        out.contains("CPU IS OPERATIONAL"),
        "TST8080 did not report success; got: {:?}",
        out
    );
}

#[test]
fn test_cputest() {
    let out = run_cpm(&load("CPUTEST.COM"), 500_000_000);
    println!("CPUTEST output: {}", out);
    assert!(
        out.contains("CPU TESTS OK"),
        "CPUTEST did not report success; got: {:?}",
        out
    );
}

/// 8080EXM is the exhaustive exerciser; it prints a CRC per instruction group and flags any
/// mismatch with "ERROR". It is slow, so it is marked `#[ignore]`; run with
/// `cargo test -p i8080-cpu -- --ignored --nocapture` to exercise it.
#[test]
#[ignore = "exhaustive exerciser; very slow (billions of cycles)"]
fn test_8080exm() {
    let out = run_cpm(&load("8080EXM.COM"), 50_000_000_000);
    println!("8080EXM output:\n{}", out);
    assert!(
        !out.contains("ERROR"),
        "8080EXM reported an error:\n{}",
        out
    );
}
