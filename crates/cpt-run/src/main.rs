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
//!   cpt-run --disasm <file> <addr> [<len>] [<origin>]
//!                                          Disassemble <len> bytes (default 256) of a raw file
//!                                          starting at byte offset <addr>, labelling addresses
//!                                          from <origin> (default 0). Prints and exits.
//!   cpt-run --cpt-boot <image> [<cycles>] [<break addr>|-] [<read delay>] [<mem out>]
//!           [--keys "<script>"] [--keys-at <cycle>] [--screen-at <cycle>]...
//!           [--brk-after <cycle>] [--brk-read <t41s14|L660|670>] [--profile]
//!           [--disk2 <img>] [--save-disk1 <out>] [--save-disk2 <out>]
//!           [--serial-listen <addr> [--serial-raw]] [--serial-in "<script>"]
//!           [--serial-in-at <cycle>] [--live]
//!                                          Boot a real CPT program disk on the real hardware
//!                                          model, type a key script into it (see
//!                                          `key_schedule`), and print the decoded screen.
//!                                          Example: --keys "Hello{14}world" types two lines.
//!                                          The serial port (8251 at 4AH/4BH) can be a Telnet
//!                                          server for a terminal (--serial-listen; raw TCP
//!                                          with --serial-raw; 0 cycles = until it
//!                                          disconnects; real time after start-up) or get
//!                                          scripted input (`serial_schedule`). --live redraws
//!                                          the screen as it changes.
//!   cpt-run --timer <file.bin>             Attach the 100 Hz task-switch timer interrupt (RST 4).
//!   cpt-run --speaker-wav out.wav <file.bin>
//!                                          Attach the speaker; when the run ends, write what it
//!                                          played to a 22.05 kHz mono WAV file.
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
    /// Attach the speaker and write what it played to this WAV file when the run ends.
    speaker_wav: Option<String>,
    /// Disk image mounted in drive 1 / drive 2 (file-backed; writes persist on exit/flush).
    disk1: Option<String>,
    disk2: Option<String>,
}

/// `cpt-run --disasm <file> <offset> [<len>] [<origin>]`: a static disassembly of part of a
/// raw file (e.g. a sector of a disk image), using the shared `i8080-isa` tables.
fn disasm_command(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("--disasm needs a file")?;
    let offset = parse_u16(args.get(1).ok_or("--disasm needs a byte offset")?)? as usize;
    let len = match args.get(2) {
        Some(s) => parse_u16(s)? as usize,
        None => 256,
    };
    let origin = match args.get(3) {
        Some(s) => parse_u16(s)?,
        None => 0,
    };
    let data = std::fs::read(path).map_err(|e| format!("cannot read {}: {}", path, e))?;
    let end = (offset + len).min(data.len());
    let bytes = data.get(offset..end).ok_or("offset is past the end of the file")?;
    let mut i = 0usize;
    while i < bytes.len() {
        let window = [bytes[i], *bytes.get(i + 1).unwrap_or(&0), *bytes.get(i + 2).unwrap_or(&0)];
        let (text, n) = i8080_isa::disassemble(&window);
        let n = (n as usize).max(1).min(bytes.len() - i);
        let raw: String = bytes[i..i + n].iter().map(|b| format!("{:02X} ", b)).collect();
        let ascii: String = bytes[i..i + n]
            .iter()
            .map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '.' })
            .collect();
        println!("{:04X}: {:<9}  {:<20}  {}", origin as usize + i, raw.trim_end(), text, ascii);
        i += n;
    }
    Ok(())
}

/// `cpt-run --cpt-boot <image> [<cycles>]`: boot a real CPT program disk image (77 x 16 x 256,
/// e.g. from `tools/td0.py --raw`) on the real hardware model as far as it is known, and report
/// what the code did. The image is read into memory only and never written back.
///
/// The boot ROM is not available, so this does what the boot code shows it must have done:
/// load track 1 sector 0 (the sector with the deleted-data mark) to 0000H and run it with
/// A = 0 and D = 0 (drive 0).
fn cpt_boot_command(args: &[String]) -> Result<(), String> {
    use cpt_machine::{CptFloppy, CptSystem};
    // Named options (anywhere after the image): --keys "<script>", --keys-at <cycle>,
    // --screen-at <cycle> (repeatable). The rest are positional.
    let mut keys_script: Option<String> = None;
    let mut keys_at: u64 = 30_000_000;
    let mut screen_at: Vec<u64> = Vec::new();
    let mut profile = false;
    let mut brk_after: u64 = 0;
    // The serial port (8251 at 4AH/4BH): a terminal over TCP, and/or scripted input.
    let mut serial_listen: Option<String> = None;
    let mut serial_raw = false;
    let mut serial_in: Option<String> = None;
    let mut serial_in_at: Option<u64> = None;
    let mut live = false;
    // Stop when the floppy reads this sector (track, physical sector).
    let mut brk_read: Option<(u8, u8)> = None;
    // A second disk in station 2, and where to save the drives' images at the end.
    let mut disk2: Option<String> = None;
    let mut save_disk1: Option<String> = None;
    let mut save_disk2: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--serial-listen" => serial_listen = Some(it.next().ok_or("--serial-listen needs an address, e.g. 127.0.0.1:2323")?.clone()),
            "--serial-raw" => serial_raw = true,
            "--serial-in" => serial_in = Some(it.next().ok_or("--serial-in needs a script")?.clone()),
            "--serial-in-at" => serial_in_at = Some(it.next().ok_or("--serial-in-at needs a cycle count")?
                .parse().map_err(|_| "invalid --serial-in-at")?),
            "--live" => live = true,
            "--disk2" => disk2 = Some(it.next().ok_or("--disk2 needs a disk image")?.clone()),
            "--save-disk1" => save_disk1 = Some(it.next().ok_or("--save-disk1 needs a file name")?.clone()),
            "--save-disk2" => save_disk2 = Some(it.next().ok_or("--save-disk2 needs a file name")?.clone()),
            "--brk-read" => brk_read = Some(parse_sector(it.next().ok_or("--brk-read needs a sector: t41s14, L660 or 656")?)?),
            "--keys" => keys_script = Some(it.next().ok_or("--keys needs a script")?.clone()),
            "--keys-at" => keys_at = it.next().ok_or("--keys-at needs a cycle count")?
                .parse().map_err(|_| "invalid --keys-at")?,
            "--profile" => profile = true,
            "--brk-after" => brk_after = it.next().ok_or("--brk-after needs a cycle count")?
                .parse().map_err(|_| "invalid --brk-after")?,
            "--screen-at" => screen_at.push(it.next().ok_or("--screen-at needs a cycle count")?
                .parse().map_err(|_| "invalid --screen-at")?),
            _ => positional.push(a.clone()),
        }
    }
    screen_at.sort_unstable();
    let args = &positional[..];
    let schedule = match &keys_script {
        Some(s) => key_schedule(s, keys_at)?,
        None => Vec::new(),
    };
    let serial_bytes = match &serial_in {
        Some(s) => serial_schedule(s, serial_in_at.unwrap_or(keys_at))?,
        None => Vec::new(),
    };
    let path = args.first().ok_or("--cpt-boot needs a disk image")?;
    let mut budget: u64 = match args.get(1) {
        Some(s) => s.parse().map_err(|_| "invalid cycle count")?,
        None => 20_000_000,
    };
    // With a terminal, 0 cycles means "until the terminal disconnects".
    if budget == 0 && serial_listen.is_some() {
        budget = u64::MAX;
    }
    // Optional breakpoint: stop the first time PC reaches it and show how we got there.
    let brk: Option<u16> = match args.get(2) {
        Some(s) if s != "-" => Some(parse_u16(s)?),
        _ => None,
    };
    // Optional: CPU cycles a sector read takes (0 = instant). The boot code times reads.
    let op_delay: u32 = match args.get(3) {
        Some(s) => s.parse().map_err(|_| "invalid read delay")?,
        None => 0,
    };
    // Optional: save memory at the end: bank 0 then every 32K block (a flat file of
    // 32K * (1 + blocks)), for offline disassembly.
    let mem_out: Option<String> = args.get(4).cloned();
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {}", path, e))?;
    let mut sys = CptSystem::boot(&bytes, op_delay).map_err(|e| format!("{}: {}", path, e))?;
    if let Some(p) = &disk2 {
        let b = std::fs::read(p).map_err(|e| format!("cannot read {}: {}", p, e))?;
        let geom = cpt_machine::cpt_floppy::cpt_geometry();
        if b.len() != geom.total_bytes() {
            return Err(format!("{}: {} bytes; a CPT disk is {}", p, b.len(), geom.total_bytes()));
        }
        let mut d = cpt_machine::DiskImage::blank(geom);
        d.load_bytes(&b);
        sys.floppy().mount(1, d);
    }
    let (fdc_dev, pit_dev, ack_dev, kb_dev, usart_dev) = (sys.fdc, sys.pit, sys.ack, sys.kb, sys.usart);
    // Something is on the line (DSR on) when there's a terminal or a serial input script.
    let serial_on = serial_listen.is_some() || !serial_bytes.is_empty();
    sys.usart().set_dsr(serial_on);
    // Opening the terminal waits for it to connect, before the system starts.
    let mut terminal = match &serial_listen {
        Some(addr) => Some(tcp_serial::TcpTransport::listen_with(addr, !serial_raw)
            .map_err(|e| format!("serial listen on {} failed: {}", addr, e))?),
        None => None,
    };
    let mut exec = sys.exec;
    fn usart_of(m: &mut Machine, dev: usize) -> &mut cpt_machine::Usart8251 {
        m.device_mut(dev).unwrap().as_any_mut().downcast_mut().unwrap()
    }

    let mut next_serial = 0usize;
    let mut next_pump: u64 = 0;
    let mut serial_out: Vec<u8> = Vec::new();
    // Real time with a terminal, once the system has settled (start-up runs as fast as it can).
    let mut pace_from: Option<(std::time::Instant, u64)> = None;
    let mut next_draw = std::time::Instant::now();
    let mut last_screen = String::new();

    // Watch the code move between 256-byte pages: a compact trail of where execution went.
    let mut trail: Vec<(u64, u16)> = Vec::new();
    let mut last_page = 0xFFFFu16;
    let mut banks: Vec<u8> = Vec::new();
    let start = exec.cpu.cycles;
    let mut recent: std::collections::VecDeque<(u16, u8, u16, u16, u16)> = std::collections::VecDeque::new();
    let mut next_key = 0usize;
    // How many sectors had been read when the key script started (to list the reads it caused).
    let mut reads_at_keys: Option<usize> = None;
    let mut next_screen = 0usize;
    // --profile: instructions executed per (bank block, 16-byte line) from --keys-at on.
    let mut hist: std::collections::HashMap<(u8, u16), u64> = std::collections::HashMap::new();
    // How a stop is reported: where, the last instructions, and the top of the stack.
    fn report_break(exec: &mut Executive, what: &str, at: u64,
        recent: &std::collections::VecDeque<(u16, u8, u16, u16, u16)>) {
        println!("{} at cycle {} (PC {:04X}, block {} selected); the last {} instructions (PC A BC DE HL):",
            what, at, exec.cpu.pc, exec.machine.selected_block(), recent.len());
        for chunk in recent.iter().collect::<Vec<_>>().chunks(5) {
            let row: Vec<String> = chunk.iter().map(|&&(pc, a, bc, de, hl)|
                format!("{:04X} {:02X} {:04X} {:04X} {:04X}", pc, a, bc, de, hl)).collect();
            println!("  {}", row.join(" | "));
        }
        let sp = exec.cpu.sp;
        let words: Vec<String> = (0..8u16).map(|k| {
            let a = sp.wrapping_add(2 * k);
            format!("{:04X}", exec.machine.read(a) as u16 | (exec.machine.read(a.wrapping_add(1)) as u16) << 8)
        }).collect();
        println!("  stack at {:04X}: {}", sp, words.join(" "));
    }
    // --brk-read: how many reads the floppy had logged when last checked.
    let mut reads_seen = 0usize;
    while exec.cpu.cycles - start < budget && !exec.is_terminally_halted() {
        if Some(exec.cpu.pc) == brk && exec.cpu.cycles - start >= brk_after {
            let at = exec.cpu.cycles - start;
            let what = format!("breakpoint {:04X} reached", exec.cpu.pc);
            report_break(&mut exec, &what, at, &recent);
            break;
        }
        let c = &exec.cpu;
        recent.push_back((c.pc, c.a, (c.b as u16) << 8 | c.c as u16, (c.d as u16) << 8 | c.e as u16,
            (c.h as u16) << 8 | c.l as u16));
        if recent.len() > 48 {
            recent.pop_front();
        }
        let page = exec.cpu.pc >> 8;
        if page != last_page {
            if trail.len() < 400 && !trail.iter().any(|&(_, p)| p >> 8 == page) {
                trail.push((exec.cpu.cycles - start, exec.cpu.pc));
            }
            last_page = page;
        }
        if profile && exec.cpu.cycles - start >= keys_at {
            let pc = exec.cpu.pc;
            let blk = if pc >= 0x8000 { exec.machine.selected_block() as u8 } else { 0xFF };
            *hist.entry((blk, pc & 0xFFF0)).or_insert(0) += 1;
        }
        exec.step();
        let now = exec.cpu.cycles - start;
        // --brk-read: stop as the read command for the sector is issued (OUT 24H), so the code
        // that asked for it is the one running, with its caller on the stack.
        if let Some((bt, bs)) = brk_read {
            let fdc = exec.machine.device_mut(fdc_dev).unwrap().as_any_mut().downcast_mut::<CptFloppy>().unwrap();
            let log = &fdc.cmd_log;
            let hit = if log.len() > reads_seen && now >= brk_after {
                // Read Sector commands are 8xH/9xH; the head is on the track being read.
                log[reads_seen..].iter().rev()
                    .find(|c| matches!(c.0 >> 4, 0x8 | 0x9) && c.3 == bt && c.2 == bs).copied()
            } else {
                None
            };
            reads_seen = log.len();
            if let Some(c) = hit {
                let what = format!("read of sector t{}s{} (linear {}) issued, DMA to {:04X} ({} bytes)",
                    bt, bs, bt as usize * 16 + bs as usize, c.4, (c.5 & 0x3FFF) + 1);
                report_break(&mut exec, &what, now, &recent);
                break;
            }
        }
        if reads_at_keys.is_none() && !schedule.is_empty() && now >= keys_at {
            let fdc = exec.machine.device_mut(fdc_dev).unwrap().as_any_mut().downcast_mut::<CptFloppy>().unwrap();
            reads_at_keys = Some(fdc.read_log.len());
        }
        while next_key < schedule.len() && schedule[next_key].0 <= now {
            let kb = exec.machine.device_mut(kb_dev).unwrap().as_any_mut()
                .downcast_mut::<cpt_machine::CptKeyboard>().unwrap();
            kb.push_event(schedule[next_key].1);
            next_key += 1;
        }
        // The serial line, every 10 ms of emulated time (about one character at 1200 baud).
        if serial_on && now >= next_pump {
            next_pump = now + 20_000;
            let u = usart_of(&mut exec.machine, usart_dev);
            let mut due = Vec::new();
            while next_serial < serial_bytes.len() && serial_bytes[next_serial].0 <= now {
                due.push(serial_bytes[next_serial].1);
                next_serial += 1;
            }
            u.push_rx(&due);
            let out = u.take_tx();
            if serial_out.len() < 4096 {
                serial_out.extend(out.iter().take(4096 - serial_out.len()));
            }
            if let Some(t) = terminal.as_mut() {
                t.send(&out);
                let incoming = t.recv();
                u.push_rx(&incoming);
                if t.is_closed() {
                    eprintln!("serial: the terminal disconnected");
                    break;
                }
                if now >= cpt_machine::cpt_system::SETTLED_CYCLES {
                    let (wall0, cyc0) = *pace_from.get_or_insert((std::time::Instant::now(), now));
                    let emulated = std::time::Duration::from_micros((now - cyc0) / 2);
                    let wall = wall0.elapsed();
                    if emulated > wall + std::time::Duration::from_millis(2) {
                        std::thread::sleep(emulated - wall);
                    }
                }
            }
            if live && std::time::Instant::now() >= next_draw {
                next_draw = std::time::Instant::now() + std::time::Duration::from_millis(500);
                let s = cpt_machine::cpt_display::render(|a| exec.machine.read(a), 0x7FF8,
                    cpt_machine::cpt_display::SCREEN_ROWS).to_text();
                if s != last_screen {
                    print!("\x1b[H\x1b[2J");
                    print_screen(&mut exec.machine, 0x7FF8);
                    last_screen = s;
                }
            }
        }
        if next_screen < screen_at.len() && screen_at[next_screen] <= now {
            println!("\n=== at cycle {} ({:.2} s) ===", now, now as f64 / 2e6);
            print_screen(&mut exec.machine, 0x7FF8);
            next_screen += 1;
        }
        let b = exec.machine.selected_block() as u8;
        if banks.last() != Some(&b) && banks.len() < 64 {
            banks.push(b);
        }
    }

    let ran = exec.cpu.cycles - start;
    println!("ran {} cycles ({:.2} s at 2 MHz); {}", ran, ran as f64 / 2e6,
        if exec.is_terminally_halted() { "terminally halted (HLT with interrupts off)" }
        else if exec.cpu.halted { "halted, waiting for an interrupt" } else { "still running" });
    println!("PC={:04X} SP={:04X} interrupts {}", exec.cpu.pc, exec.cpu.sp,
        if exec.cpu.int_enabled { "enabled" } else { "disabled" });
    print_registers(&exec.cpu);

    let fdc = exec.machine.device_mut(fdc_dev).unwrap().as_any_mut().downcast_mut::<CptFloppy>().unwrap();
    println!("\nsectors read: {}", fdc.sectors_read);
    let mut runs: Vec<(u8, u8, u8)> = Vec::new(); // (track, first sector, last sector)
    for &(t, s) in &fdc.read_log {
        match runs.last_mut() {
            Some(r) if r.0 == t && r.2.wrapping_add(1) == s => r.2 = s,
            _ => runs.push((t, s, s)),
        }
    }
    let shown: Vec<String> = runs.iter().take(60).map(|&(t, a, b)| {
        if a == b { format!("t{}s{}", t, a) } else { format!("t{}s{}-{}", t, a, b) }
    }).collect();
    println!("  in order: {}{}", shown.join(" "), if runs.len() > 60 { " ..." } else { "" });
    if let Some(n) = reads_at_keys {
        let after: Vec<String> = fdc.read_log[n..].iter()
            .map(|&(t, s)| format!("t{}s{}({})", t, s, t as usize * 16 + s as usize)).collect();
        println!("  after the keys started ({}): {}", after.len(), after.join(" "));
    }
    let errors: Vec<_> = fdc.cmd_log.iter().filter(|c| c.6 != 0).collect();
    println!("  commands: {}, with nonzero status: {}", fdc.cmd_log.len(), errors.len());
    for c in errors.iter().take(10) {
        println!("    cmd {:02X} track reg {} sector {} head on {} dma {:04X} count {:04X} -> status {:02X}",
            c.0, c.1, c.2, c.3, c.4, c.5, c.6);
    }
    println!("  last commands:");
    for c in fdc.cmd_log.iter().rev().take(6).collect::<Vec<_>>().into_iter().rev() {
        println!("    cmd {:02X} track reg {} sector {} head on {} dma {:04X} count {:04X} -> status {:02X}",
            c.0, c.1, c.2, c.3, c.4, c.5, c.6);
    }

    let pit = exec.machine.device_mut(pit_dev).unwrap().as_any_mut().downcast_mut::<cpt_machine::Pit8253>().unwrap();
    println!("\n8253: counter reloads {:?}, output pulses {:?}",
        [pit.reload(0), pit.reload(1), pit.reload(2)], pit.pulses);
    // The speaker (8253 counter 0): each tone as "cycle: Hz" (or "off").
    let spk = pit.take_speaker();
    let shown: Vec<String> = spk.iter().take(40).map(|&(c, n)| {
        if n == 0 { format!("{}: off", c) }
        else { format!("{}: {:.0} Hz", c, cpt_machine::cpt_interrupts::PIT_CLOCK_HZ as f64 / n as f64) }
    }).collect();
    println!("speaker ({} changes): {}", spk.len(), if shown.is_empty() { "(silent)".into() } else { shown.join(", ") });
    let ack = exec.machine.device_mut(ack_dev).unwrap().as_any_mut().downcast_mut::<cpt_machine::InterruptAck>().unwrap();
    let acks: Vec<String> = ack.acks.iter().map(|(v, n)| format!("{:02X}x{}", v, n)).collect();
    println!("interrupt acknowledges on 00H: {}", if acks.is_empty() { "(none)".into() } else { acks.join(" ") });

    let kb = exec.machine.device_mut(kb_dev).unwrap().as_any_mut()
        .downcast_mut::<cpt_machine::CptKeyboard>().unwrap();
    if !schedule.is_empty() {
        let shown: Vec<String> = kb.delivered.iter().take(40).map(|e| format!("{:02X}", e)).collect();
        println!("keyboard: {} of {} events scheduled, {} read by the CPU, {} still waiting: {}",
            next_key, schedule.len(), kb.delivered.len(), kb.pending(), shown.join(" "));
    }

    // The serial port, if the system set it up or anything came or went on the line.
    let u = usart_of(&mut exec.machine, usart_dev);
    serial_out.extend(u.take_tx().into_iter().take(4096usize.saturating_sub(serial_out.len())));
    if !u.control_log.is_empty() || u.sent > 0 || u.received > 0 {
        let writes: Vec<String> = u.control_log.iter().take(24)
            .map(|(p, v)| format!("{:02X}:{:02X}", p, v)).collect();
        println!("serial 8251 (4AH-4CH): mode {:02X}, command {:02X}, 4CH = {}; control writes: {}",
            u.mode(), u.command(), u.rate_select().map_or("-".into(), |v| format!("{:02X}", v)), writes.join(" "));
        println!("  sent {} bytes{}: \"{}\"", u.sent, if u.breaks > 0 { format!(" and {} break(s)", u.breaks) } else { String::new() },
            serial_text(&serial_out));
        println!("  received {} bytes ({} still waiting)", u.received, u.rx_waiting() + serial_bytes.len() - next_serial);
    }

    if profile {
        let total: u64 = hist.values().sum();
        let mut v: Vec<_> = hist.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        println!("\nprofile from cycle {} ({} instructions; bank block shown for 8000H and up):", keys_at, total);
        for ((blk, line), n) in v.iter().take(24) {
            let where_ = if *blk == 0xFF { "      ".to_string() } else { format!("blk {} ", blk) };
            println!("  {}{:04X}  {:>9}  {:5.1}%", where_, line, n, *n as f64 * 100.0 / total.max(1) as f64);
        }
    }

    println!("\nbanks selected, in order: {:?}", banks);
    println!("\nexecution trail (cycle: first PC seen in each new 256-byte page):");
    for chunk in trail.chunks(8) {
        let row: Vec<String> = chunk.iter().map(|&(c, pc)| format!("{:>9}:{:04X}", c, pc)).collect();
        println!("  {}", row.join(" "));
    }

    let mut outs: Vec<(u8, usize, Vec<u8>)> = Vec::new();
    for &(p, v) in &exec.machine.unmapped_io {
        match outs.iter_mut().find(|o| o.0 == p) {
            Some(o) => {
                o.1 += 1;
                if o.2.len() < 8 && !o.2.contains(&v) { o.2.push(v); }
            }
            None => outs.push((p, 1, vec![v])),
        }
    }
    outs.sort();
    println!("\nunmapped OUT ports (count, first values):");
    for (p, n, vals) in &outs {
        let v: Vec<String> = vals.iter().map(|v| format!("{:02X}", v)).collect();
        println!("  {:02X}H x{:<6} {}", p, n, v.join(" "));
    }
    let mut ins = exec.machine.unmapped_in.clone();
    ins.sort();
    let ins: Vec<String> = ins.iter().map(|p| format!("{:02X}H", p)).collect();
    println!("unmapped IN ports: {}", if ins.is_empty() { "(none)".into() } else { ins.join(" ") });

    print_screen(&mut exec.machine, 0x7FF8);

    if let Some(path) = mem_out {
        let m = &mut exec.machine;
        let keep = m.selected_block();
        let mut out: Vec<u8> = (0..0x8000u16).map(|a| m.read(a)).collect();
        for b in 0..m.mem.block_count() {
            m.mem.select_block(b);
            out.extend((0x8000u32..0x10000).map(|a| m.read(a as u16)));
        }
        m.mem.select_block(keep);
        std::fs::write(&path, &out).map_err(|e| format!("cannot write {}: {}", path, e))?;
        println!("\nsaved memory to {}: bank 0, then {} blocks of 32K (block {} was selected)",
            path, m.mem.block_count(), keep);
    }
    // The drives' images as they are at the end (writes included).
    for (drive, path) in [(0usize, &save_disk1), (1usize, &save_disk2)] {
        if let Some(path) = path {
            let fdc = exec.machine.device_mut(fdc_dev).unwrap().as_any_mut().downcast_mut::<CptFloppy>().unwrap();
            match fdc.image(drive) {
                Some(img) => {
                    std::fs::write(path, img.bytes()).map_err(|e| format!("cannot write {}: {}", path, e))?;
                    println!("\nsaved the disk in station {} to {}", drive + 1, path);
                }
                None => println!("\nstation {} is empty; nothing saved to {}", drive + 1, path),
            }
        }
    }
    Ok(())
}

/// Decode the real display list starting at `head` (see `cpt_machine::cpt_display`) and print
/// the screen in a frame, one numbered line per row. Runs of blank rows are collapsed. On a
/// terminal the screen is drawn in its own colours (black on white, with white-on-black rows);
/// in plain (redirected) output white-on-black rows are tagged after the frame.
fn print_screen(m: &mut cpt_machine::Machine, head: u16) {
    use std::io::IsTerminal;
    let screen = cpt_machine::cpt_display::render(|a| m.read(a), head, cpt_machine::cpt_display::SCREEN_ROWS);
    let ansi = std::io::stdout().is_terminal();
    let body = if ansi { screen.to_ansi() } else { screen.to_text() };
    let width = cpt_machine::cpt_display::WINDOW_COLS;
    let rule = "-".repeat(width);
    println!("\nscreen from the display list at {:04X}H ({} rows):", head, screen.rows.len());
    println!("     +{}+", rule);
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let run = screen.rows[i..].iter().take_while(|r| r.is_empty()).count();
        if run > 2 {
            println!("     | ({} blank rows){}|", run, " ".repeat(width.saturating_sub(14 + digits(run))));
            i += run;
            continue;
        }
        // Empty white-on-black rows (filler rows with no document text).
        let black = |r: &Vec<cpt_machine::cpt_display::Cell>| !r.is_empty() && r.iter().all(|c| c.reverse && c.code == 0x20);
        let run = screen.rows[i..].iter().take_while(|r| black(r)).count();
        if run > 2 {
            let label = format!(" ({} empty rows, white on black)", run);
            println!("{:>4} |{}{}| white on black", i + 1, label, " ".repeat(width.saturating_sub(label.len())));
            i += run;
            continue;
        }
        // In plain output, tag rows shown white on black (all of it, or the runs that are).
        let tag = if ansi || screen.rows[i].iter().all(|c| !c.reverse) {
            String::new()
        } else if screen.row_is_reverse(i) {
            " white on black".to_string()
        } else {
            let cols: Vec<String> = screen.reverse_runs().iter().filter(|r| r.0 == i)
                .map(|&(_, a, b)| if a == b { format!("{}", a + 1) } else { format!("{}-{}", a + 1, b + 1) })
                .collect();
            format!(" white on black at {}", cols.join(","))
        };
        println!("{:>4} |{}|{}", i + 1, lines[i], tag);
        i += 1;
    }
    println!("     +{}+", rule);
    let ending = match screen.ending {
        cpt_machine::cpt_display::Ending::EndMark(a) => format!("end mark at {:04X}H", a),
        cpt_machine::cpt_display::Ending::Loop(a) => format!("filler loop at {:04X}H", a),
        cpt_machine::cpt_display::Ending::RowLimit => "row limit".into(),
        cpt_machine::cpt_display::Ending::ByteLimit => "byte limit (unterminated list)".into(),
    };
    println!("  ended at: {}", ending);
    let mut rev_rows: Vec<usize> = screen.reverse_runs().iter().map(|r| r.0 + 1).collect();
    rev_rows.dedup();
    // As ranges: "1, 2-45, 47".
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for r in rev_rows {
        match ranges.last_mut() {
            Some(last) if last.1 + 1 == r => last.1 = r,
            _ => ranges.push((r, r)),
        }
    }
    let rev_rows: Vec<String> = ranges.iter()
        .map(|&(a, b)| if a == b { a.to_string() } else { format!("{}-{}", a, b) }).collect();
    println!("  white on black (rows): {}", if rev_rows.is_empty() { "(none)".into() } else { rev_rows.join(", ") });
    if !screen.unknown_codes.is_empty() {
        let u: Vec<String> = screen.unknown_codes.iter().map(|c| format!("{:02X}", c)).collect();
        println!("  codes with no rule yet: {}", u.join(" "));
    }
}

/// Turn a key script into timed raw keyboard events (cycle, event), starting at `at`.
///
/// - A plain character taps the key that types it, with SHIFT held around it if needed (keys
///   come from the program's own table; see `cpt_machine::cpt_keyboard`).
/// - `{XX}` taps key number XX (hex). `{+XX}` presses it and `{-XX}` releases it, for chords
///   such as `{+77}x{-77}` (CODE held while x is tapped).
/// - `{code}`, `{shift}`, `{lock}` stand for the CODE, SHIFT and LOCK key numbers in any of the
///   forms above, e.g. `{+code}`, `{lock}`.
/// - `{ms N}` waits N milliseconds; `{up}` sends the all-keys-up event (80H).
///
/// Timing is a typist's: a tap is held 20 ms, and keys are 60 ms apart (at 2 MHz).
fn key_schedule(script: &str, at: u64) -> Result<Vec<(u64, u8)>, String> {
    use cpt_machine::cpt_keyboard::{key_for_char, KEY_CODE, KEY_LOCK, KEY_SHIFT};
    const MS: u64 = 2_000;
    let mut out = Vec::new();
    let mut t = at;
    let tap = |out: &mut Vec<(u64, u8)>, t: &mut u64, key: u8| {
        out.push((*t, key));
        out.push((*t + 20 * MS, key | 0x80));
        *t += 60 * MS;
    };
    let mut chars = script.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '{' {
            let (key, shift) = key_for_char(c).ok_or(format!("no key types {:?}", c))?;
            if shift {
                out.push((t, KEY_SHIFT));
                t += 10 * MS;
                tap(&mut out, &mut t, key);
                out.push((t, KEY_SHIFT | 0x80));
                t += 30 * MS;
            } else {
                tap(&mut out, &mut t, key);
            }
            continue;
        }
        let body: String = chars.by_ref().take_while(|&c| c != '}').collect();
        let body = body.trim().to_ascii_lowercase();
        if let Some(n) = body.strip_prefix("ms") {
            t += n.trim().parse::<u64>().map_err(|_| format!("bad {{{}}}", body))? * MS;
            continue;
        }
        if body == "up" {
            out.push((t, 0x80));
            t += 60 * MS;
            continue;
        }
        let (op, name) = match body.chars().next() {
            Some(o @ ('+' | '-')) => (o, &body[1..]),
            _ => (' ', &body[..]),
        };
        let key = match name {
            "code" => KEY_CODE,
            "shift" => KEY_SHIFT,
            "lock" => KEY_LOCK,
            hex => u8::from_str_radix(hex, 16).ok().filter(|k| *k < 0x80)
                .ok_or(format!("bad key {{{}}} (want hex 01-7F or code/shift/lock)", body))?,
        };
        match op {
            '+' => { out.push((t, key)); t += 30 * MS; }
            '-' => { out.push((t, key | 0x80)); t += 30 * MS; }
            _ => tap(&mut out, &mut t, key),
        }
    }
    out.sort_by_key(|e| e.0);
    Ok(out)
}

/// Turn a serial input script into timed bytes (cycle, byte) for the CPT's receive line,
/// starting at `at`: plain characters are sent as themselves, `{XX}` sends byte XX (hex, e.g.
/// `{0D}` for carriage return), and `{ms N}` waits N milliseconds. Bytes due at the same time
/// arrive one after another at the line's speed.
fn serial_schedule(script: &str, at: u64) -> Result<Vec<(u64, u8)>, String> {
    const MS: u64 = 2_000;
    let mut out = Vec::new();
    let mut t = at;
    let mut chars = script.chars();
    while let Some(c) = chars.next() {
        if c != '{' {
            let mut buf = [0u8; 4];
            for &b in c.encode_utf8(&mut buf).as_bytes() {
                out.push((t, b));
            }
            continue;
        }
        let body: String = chars.by_ref().take_while(|&c| c != '}').collect();
        let body = body.trim().to_ascii_lowercase();
        if let Some(n) = body.strip_prefix("ms") {
            t += n.trim().parse::<u64>().map_err(|_| format!("bad {{{}}}", body))? * MS;
            continue;
        }
        let b = u8::from_str_radix(&body, 16).map_err(|_| format!("bad serial byte {{{}}} (want hex 00-FF)", body))?;
        out.push((t, b));
    }
    Ok(out)
}

/// A CPT disk sector for `--brk-read`, as (track, physical sector):
/// - `t41s14`: track 41, physical sector 14 (as the report's read lists show them);
/// - `L660`: logical sector 660, as directory pointers number them (track * 16 + s, with the
///   filesystem's skew: physical = (3 * s + 2 * track) mod 16; see tools/cptfs.py);
/// - `670`: linear physical sector 670 (track * 16 + physical sector).
fn parse_sector(s: &str) -> Result<(u8, u8), String> {
    let bad = || format!("bad sector {:?} (want t41s14, L660 or 670)", s);
    let lower = s.trim().to_ascii_lowercase();
    let (t, p) = if let Some(rest) = lower.strip_prefix('t') {
        let (t, p) = rest.split_once('s').ok_or_else(bad)?;
        (t.parse::<u32>().map_err(|_| bad())?, p.parse::<u32>().map_err(|_| bad())?)
    } else if let Some(n) = lower.strip_prefix('l') {
        let n: u32 = n.parse().map_err(|_| bad())?;
        let (t, s) = (n / 16, n % 16);
        (t, (3 * s + 2 * t) % 16)
    } else {
        let n: u32 = lower.parse().map_err(|_| bad())?;
        (n / 16, n % 16)
    };
    if t >= 77 || p >= 16 {
        return Err(bad());
    }
    Ok((t as u8, p as u8))
}

/// Show bytes from the serial line as text: printable ASCII as is, CR/LF as \r \n, others as
/// {XX}.
fn serial_text(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        match b {
            b'\r' => s.push_str("\\r"),
            b'\n' => s.push_str("\\n"),
            0x20..=0x7E => s.push(b as char),
            _ => s.push_str(&format!("{{{:02X}}}", b)),
        }
    }
    s
}

fn digits(n: usize) -> usize {
    n.to_string().len()
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
        speaker_wav: None,
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
            "--speaker-wav" => {
                i += 1;
                opts.speaker_wav = Some(args.get(i).ok_or("--speaker-wav needs a file name")?.clone());
            }
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
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("--cpt-boot") {
        return match cpt_boot_command(&argv[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {}", e);
                eprintln!("usage: cpt-run --cpt-boot <image> [<cycles>]");
                ExitCode::from(2)
            }
        };
    }
    if argv.first().map(String::as_str) == Some("--disasm") {
        return match disasm_command(&argv[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {}", e);
                eprintln!("usage: cpt-run --disasm <file> <offset> [<len>] [<origin>]");
                ExitCode::from(2)
            }
        };
    }
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {}", e);
            eprintln!(
                "usage: cpt-run [--trace] [--load <addr>] [--steps <n>] \
                 [--serial-listen <addr> [--serial-raw]|--serial-connect <addr>] [--timer] [--speaker-wav <out.wav>] \
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

    // Speaker: record what the program plays, to be written out as a WAV at the end.
    let mut speaker_dev: Option<usize> = None;
    if opts.speaker_wav.is_some() {
        speaker_dev = Some(machine.device_count());
        machine.attach_device(Box::new(cpt_machine::SpeakerDevice::new(
            cfg.speaker_base_port,
            cfg.speaker_tone_clock_hz,
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

    // Render what the speaker played to a WAV file.
    if let (Some(dev), Some(path)) = (speaker_dev, opts.speaker_wav.as_ref()) {
        const RATE: u32 = 22_050;
        let events = exec
            .machine
            .device_mut(dev)
            .and_then(|d| d.as_any_mut().downcast_mut::<cpt_machine::SpeakerDevice>())
            .map(|s| s.take_events())
            .unwrap_or_default();
        let mut r = cpt_machine::ToneRenderer::new(RATE, cfg.cpu_clock_hz, cfg.speaker_tone_clock_hz);
        r.push_events(&events);
        let mut samples = Vec::new();
        r.render_until(exec.cpu.cycles, &mut samples);
        match std::fs::write(path, cpt_machine::wav_bytes(&samples, RATE)) {
            Ok(()) => println!(
                "speaker: {} tone change(s), {:.2} s of audio written to {}",
                events.len(),
                samples.len() as f64 / RATE as f64,
                path
            ),
            Err(e) => eprintln!("error: cannot write {}: {}", path, e),
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
