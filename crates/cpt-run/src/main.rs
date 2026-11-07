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
//!   cpt-run --timer <file.bin>             Attach the 100 Hz task-switch timer interrupt (RST 4).
//!   cpt-run --serial-listen 127.0.0.1:2323 <file.bin>
//!                                          Serial port as a Telnet server (the guest echoes;
//!                                          add --serial-raw for a plain TCP byte pipe).
//!
//! Options may be combined; the file is the last argument.

mod serial_demo;
mod tcp_serial;

use cpt_machine::{Executive, Machine, MachineConfig, SerialDevice, Transport};
use i8080_cpu::{Bus, Cpu};
use std::process::ExitCode;

/// How to back the serial port, if requested.
enum SerialMode {
    None,
    Listen(String),
    Connect(String),
}

struct Options {
    file: String,
    trace: bool,
    display: bool,
    demo: bool,
    serial_demo: bool,
    load_addr: u16,
    max_steps: u64,
    serial: SerialMode,
    /// With `--serial-listen`: skip Telnet negotiation/filtering (for raw TCP clients).
    serial_raw: bool,
    /// Attach the 100 Hz task-switch timer (off by default: a program that enables interrupts
    /// without a handler at the timer's RST vector would run into page-zero garbage).
    timer: bool,
    /// Disk image mounted in drive 1 / drive 2 (file-backed; writes persist on exit/flush).
    disk1: Option<String>,
    disk2: Option<String>,
}

fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut opts = Options {
        file: String::new(),
        trace: false,
        display: false,
        demo: false,
        serial_demo: false,
        load_addr: 0x0000,
        max_steps: 1_000_000,
        serial: SerialMode::None,
        serial_raw: false,
        timer: false,
        disk1: None,
        disk2: None,
    };
    let mut steps_set = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--trace" => opts.trace = true,
            "--display" => opts.display = true,
            "--demo" => {
                opts.demo = true;
                opts.display = true;
            }
            "--serial-listen" => {
                i += 1;
                opts.serial = SerialMode::Listen(
                    args.get(i).ok_or("--serial-listen needs an address")?.clone(),
                );
            }
            "--serial-raw" => opts.serial_raw = true,
            "--timer" => opts.timer = true,
            "--serial-connect" => {
                i += 1;
                opts.serial = SerialMode::Connect(
                    args.get(i).ok_or("--serial-connect needs an address")?.clone(),
                );
            }
            "--serial-demo" => {
                // Built-in serial sign-on + echo program; defaults to listening on :2323 if no
                // serial mode was otherwise given.
                opts.serial_demo = true;
                opts.max_steps = u64::MAX; // run until the connection closes / interrupted
                if matches!(opts.serial, SerialMode::None) {
                    opts.serial = SerialMode::Listen("127.0.0.1:2323".to_string());
                }
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
                steps_set = true;
            }
            "--disk1" => {
                i += 1;
                opts.disk1 = Some(args.get(i).ok_or("--disk1 needs a disk image file")?.clone());
            }
            "--disk2" => {
                i += 1;
                opts.disk2 = Some(args.get(i).ok_or("--disk2 needs a disk image file")?.clone());
            }
            other if other.starts_with("--") => return Err(format!("unknown option {}", other)),
            _ => opts.file = args[i].clone(),
        }
        i += 1;
    }
    if opts.file.is_empty() && !opts.demo && !opts.serial_demo {
        return Err("no input file given".into());
    }
    // An interactive serial session should run until the peer disconnects or the user hits
    // Ctrl+C, so don't apply the 1M-step cap unless --steps was given explicitly.
    if !steps_set && !matches!(opts.serial, SerialMode::None) {
        opts.max_steps = u64::MAX;
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
            eprintln!(
                "usage: cpt-run [--trace] [--load <addr>] [--steps <n>] \
                 [--serial-listen <addr> [--serial-raw]|--serial-connect <addr>] [--timer] \
                 [--disk1 <image>] [--disk2 <image>] <file.bin>"
            );
            return ExitCode::from(2);
        }
    };

    // The demos supply their own built-in program; otherwise read the file.
    let (program, load_addr) = if opts.serial_demo {
        (serial_demo::serial_demo(), 0x0000u16)
    } else if opts.demo {
        (cpt_machine::demo::display_demo(), 0x0100u16)
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

    if opts.timer {
        machine.attach_device(Box::new(cpt_machine::TimerDevice::new(
            cfg.timer_period_cycles,
            cfg.timer_vector,
        )));
    }

    // Disk drives: if either image was given, attach the controller and mount the images
    // (drive 1 = station A, drive 2 = station B). Images are file-backed, so writes the guest
    // flushes (or that are pending at a clean exit) persist to the host file.
    let mut disk_dev: Option<usize> = None;
    if opts.disk1.is_some() || opts.disk2.is_some() {
        let geom = cpt_machine::Geometry::default();
        let mut ctrl = cpt_machine::DiskController::new(cfg.disk_base_port, geom);
        let drives = [
            (cpt_machine::Station::A, "disk1", &opts.disk1),
            (cpt_machine::Station::B, "disk2", &opts.disk2),
        ];
        for (station, label, path) in drives {
            let Some(path) = path else { continue };
            // open_file would silently create a new blank image on a typo'd path; refuse.
            if !std::path::Path::new(path).exists() {
                eprintln!("error: --{} image '{}' does not exist", label, path);
                return ExitCode::FAILURE;
            }
            match cpt_machine::DiskImage::open_file(path, geom) {
                Ok(img) => ctrl.mount(station, img),
                Err(e) => {
                    eprintln!("error: cannot open --{} image '{}': {}", label, path, e);
                    return ExitCode::FAILURE;
                }
            }
        }
        disk_dev = Some(machine.device_count());
        machine.attach_device(Box::new(ctrl));
    }

    // Serial port: attach the device and open a TCP transport if requested.
    let mut serial: Option<(usize, Box<dyn Transport>)> = None;
    let transport: Option<Box<dyn Transport>> = match &opts.serial {
        SerialMode::None => None,
        SerialMode::Listen(addr) => match tcp_serial::TcpTransport::listen_with(addr, !opts.serial_raw) {
            Ok(t) => Some(Box::new(t)),
            Err(e) => {
                eprintln!("error: serial listen on {} failed: {}", addr, e);
                return ExitCode::FAILURE;
            }
        },
        SerialMode::Connect(addr) => match tcp_serial::TcpTransport::connect(addr) {
            Ok(t) => Some(Box::new(t)),
            Err(e) => {
                eprintln!("error: serial connect to {} failed: {}", addr, e);
                return ExitCode::FAILURE;
            }
        },
    };
    if let Some(t) = transport {
        let dev = machine.device_count();
        machine.attach_device(Box::new(SerialDevice::new(
            cfg.serial_base_port,
            Some(cfg.serial_vector),
        )));
        serial = Some((dev, t));
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

        // Pump the serial transport periodically (not every instruction — once per ~1000 steps
        // keeps overhead low while staying responsive).
        if let Some((dev, transport)) = serial.as_mut() {
            if steps % 1000 == 0 {
                if let Some(s) = exec
                    .machine
                    .device_mut(*dev)
                    .and_then(|d| d.as_any_mut().downcast_mut::<SerialDevice>())
                {
                    cpt_machine::serial_pump(s, transport.as_mut());
                }
            }
        }
    }

    // Eject (and thereby flush) any mounted disks so pending writes reach the image files.
    if let Some(dev) = disk_dev {
        if let Some(c) = exec
            .machine
            .device_mut(dev)
            .and_then(|d| d.as_any_mut().downcast_mut::<cpt_machine::DiskController>())
        {
            let _ = c.eject(cpt_machine::Station::A);
            let _ = c.eject(cpt_machine::Station::B);
        }
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
