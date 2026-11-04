//! CLI trace runner for the CPT machine.
//!
//! Loads a raw 8080 binary into the machine's bank 0, runs it on the executive, and (with
//! `--trace`) prints a disassembled instruction trace alongside register/flag state. This ties
//! together everything built so far: the `i8080-cpu` core, the shared `i8080-isa` disassembler,
//! and the `cpt-machine` banked memory + executive.
//!
//! Usage:
//!   cpt-run <file.bin>                     Run until a terminal HLT (or the step cap).
//!   cpt-run --trace <file.bin>             Print a disassembled trace, one line per step.
//!   cpt-run --load 0x0100 <file.bin>       Load (and start) at a given address.
//!   cpt-run --steps 5000 <file.bin>        Cap the number of instructions.
//!
//! Options may be combined; the file is the last argument.

use cpt_machine::{Executive, Machine, MachineConfig};
use i8080_cpu::{Bus, Cpu};
use std::process::ExitCode;

mod demo;

struct Options {
    file: String,
    trace: bool,
    display: bool,
    demo: bool,
    load_addr: u16,
    max_steps: u64,
}

fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut opts = Options {
        file: String::new(),
        trace: false,
        display: false,
        demo: false,
        load_addr: 0x0000,
        max_steps: 1_000_000,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--trace" => opts.trace = true,
            "--display" => opts.display = true,
            "--demo" => {
                opts.demo = true;
                opts.display = true;
            }
            "--load" => {
                i += 1;
                opts.load_addr = parse_u16(args.get(i).ok_or("--load needs an address")?)?;
            }
            "--steps" => {
                i += 1;
                opts.max_steps = args
                    .get(i)
                    .ok_or("--steps needs a number")?
                    .parse()
                    .map_err(|_| "invalid --steps value")?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {}", other)),
            _ => opts.file = args[i].clone(),
        }
        i += 1;
    }
    if opts.file.is_empty() && !opts.demo {
        return Err("no input file given".into());
    }
    Ok(opts)
}

fn parse_u16(s: &str) -> Result<u16, String> {
    let s = s.trim();
    let v = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16)
    } else {
        s.parse()
    };
    v.map_err(|_| format!("invalid address '{}'", s))
}

fn main() -> ExitCode {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {}", e);
            eprintln!("usage: cpt-run [--trace] [--load <addr>] [--steps <n>] <file.bin>");
            return ExitCode::from(2);
        }
    };

    // The demo supplies its own built-in program; otherwise read the file.
    let (program, load_addr) = if opts.demo {
        (demo::display_demo(), 0x0100u16)
    } else {
        match std::fs::read(&opts.file) {
            Ok(p) => (p, opts.load_addr),
            Err(e) => {
                eprintln!("error: cannot read {}: {}", opts.file, e);
                return ExitCode::FAILURE;
            }
        }
    };
    let opts = Options { load_addr, ..opts };

    // Build the machine and load the program into bank 0 at the requested address.
    let cfg = MachineConfig::default();
    let mut machine = Machine::new(cfg.clone());
    if opts.display {
        machine.attach_display(cpt_machine::DisplayDevice::new(
            cfg.retrace_period_cycles,
            cfg.retrace_vector,
        ));
    }
    if (opts.load_addr as usize + program.len()) > 0x8000 {
        eprintln!(
            "error: program of {} bytes at {:#06X} does not fit in bank 0 (0x0000-0x7FFF)",
            program.len(),
            opts.load_addr
        );
        return ExitCode::FAILURE;
    }
    machine.mem.load_bank0(opts.load_addr, &program);

    let mut cpu = Cpu::new();
    cpu.pc = opts.load_addr;
    cpu.sp = 0x7F00; // a default stack high in bank 0
    let mut exec = Executive::new(cpu, machine);

    if opts.trace {
        println!(
            "{:<24}  {:<20}  {}",
            "ADDR: BYTES", "INSTR", "A  B  C  D  E  H  L   SP   FLAGS"
        );
    }

    let mut steps = 0u64;
    while steps < opts.max_steps {
        if exec.is_terminally_halted() {
            break;
        }
        if opts.trace {
            print_trace_line(&mut exec);
        }
        exec.step();
        steps += 1;
    }

    // Render the display frame if one was attached.
    if opts.display {
        if let Some(d) = exec.machine.display_mut() {
            let retraces = d.retrace_count;
            let frame = d.snapshot().render_text();
            println!("\n=== display ({} vertical-retrace interrupts) ===", retraces);
            print!("{}", frame);
        }
    }

    // Summary.
    let halted = exec.is_terminally_halted();
    println!(
        "\n--- {} after {} instruction(s), {} cycles ---",
        if halted { "halted" } else { "step cap reached" },
        steps,
        exec.cpu.cycles
    );
    print_registers(&exec.cpu);
    ExitCode::SUCCESS
}

/// Print one trace line: address, raw bytes, disassembly, and register/flag state *before*
/// the instruction executes.
fn print_trace_line(exec: &mut Executive) {
    let pc = exec.cpu.pc;
    // Peek up to 3 bytes for disassembly (reads are side-effect-free for memory).
    let bytes = [
        exec.machine.read(pc),
        exec.machine.read(pc.wrapping_add(1)),
        exec.machine.read(pc.wrapping_add(2)),
    ];
    let (text, len) = i8080_isa::disassemble(&bytes);
    let raw: String = bytes[..len as usize]
        .iter()
        .map(|b| format!("{:02X} ", b))
        .collect();

    let c = &exec.cpu;
    let f = &c.flags;
    let flagstr = format!(
        "{}{}{}{}{}",
        if f.sign { 'S' } else { '-' },
        if f.zero { 'Z' } else { '-' },
        if f.aux_carry { 'A' } else { '-' },
        if f.parity { 'P' } else { '-' },
        if f.carry { 'C' } else { '-' },
    );

    println!(
        "{:04X}: {:<9}  {:<20}  {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:04X} {}",
        pc, raw.trim_end(), text, c.a, c.b, c.c, c.d, c.e, c.h, c.l, c.sp, flagstr
    );
}

fn print_registers(c: &Cpu) {
    println!(
        "A={:02X} B={:02X} C={:02X} D={:02X} E={:02X} H={:02X} L={:02X}  SP={:04X} PC={:04X}",
        c.a, c.b, c.c, c.d, c.e, c.h, c.l, c.sp, c.pc
    );
    let f = &c.flags;
    println!(
        "flags: S={} Z={} AC={} P={} CY={}",
        f.sign as u8, f.zero as u8, f.aux_carry as u8, f.parity as u8, f.carry as u8
    );
}
