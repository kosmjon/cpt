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
//!           [--brk-after <cycle>] [--profile]
//!                                          Boot a real CPT program disk on the real hardware
//!                                          model, type a key script into it (see
//!                                          `key_schedule`), and print the decoded screen.
//!                                          Example: --keys "Hello{14}world" types two lines.
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
    use cpt_machine::{cpt_floppy, CptFloppy, DiskImage};
    // Named options (anywhere after the image): --keys "<script>", --keys-at <cycle>,
    // --screen-at <cycle> (repeatable). The rest are positional.
    let mut keys_script: Option<String> = None;
    let mut keys_at: u64 = 30_000_000;
    let mut screen_at: Vec<u64> = Vec::new();
    let mut profile = false;
    let mut brk_after: u64 = 0;
    let mut positional: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
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
    let path = args.first().ok_or("--cpt-boot needs a disk image")?;
    let budget: u64 = match args.get(1) {
        Some(s) => s.parse().map_err(|_| "invalid cycle count")?,
        None => 20_000_000,
    };
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
    let geom = cpt_floppy::cpt_geometry();
    if bytes.len() != geom.total_bytes() {
        return Err(format!("{} is {} bytes; a CPT disk is {}", path, bytes.len(), geom.total_bytes()));
    }
    let mut image = DiskImage::blank(geom);
    image.load_bytes(&bytes);

    let cfg = MachineConfig::cpt_real();
    let mut machine = Machine::new(cfg.clone());
    let mut fdc = CptFloppy::new();
    fdc.mount(0, image);
    fdc.op_delay_cycles = op_delay;
    let fdc_dev = machine.device_count();
    machine.attach_device(Box::new(fdc));
    // The 8253 timer at 1CH-1FH (counter 1 -> RST 5, counter 2 -> RST 7: a guess) and the
    // interrupt-acknowledge port at 00H.
    let pit_dev = machine.device_count();
    machine.attach_device(Box::new(cpt_machine::Pit8253::cpt()));
    let ack_dev = machine.device_count();
    machine.attach_device(Box::new(cpt_machine::InterruptAck::new(0x00)));
    // The display's vertical retrace on RST 6 (its handler drains the queued display-list
    // updates). 60 Hz is a guess.
    machine.attach_device(Box::new(cpt_machine::TimerDevice::new(33_333, 6)));
    // The keyboard on 01H/02H, raising RST 2.
    let kb_dev = machine.device_count();
    machine.attach_device(Box::new(cpt_machine::CptKeyboard::new()));
    let boot = &bytes[16 * 256..17 * 256];
    machine.mem.load_bank0(0, boot);

    let mut cpu = Cpu::new();
    cpu.pc = 0;
    let mut exec = Executive::new(cpu, machine);

    // Watch the code move between 256-byte pages: a compact trail of where execution went.
    let mut trail: Vec<(u64, u16)> = Vec::new();
    let mut last_page = 0xFFFFu16;
    let mut banks: Vec<u8> = Vec::new();
    let start = exec.cpu.cycles;
    let mut recent: std::collections::VecDeque<(u16, u8, u16, u16, u16)> = std::collections::VecDeque::new();
    let mut next_key = 0usize;
    let mut next_screen = 0usize;
    // --profile: instructions executed per (bank block, 16-byte line) from --keys-at on.
    let mut hist: std::collections::HashMap<(u8, u16), u64> = std::collections::HashMap::new();
    while exec.cpu.cycles - start < budget && !exec.is_terminally_halted() {
        if Some(exec.cpu.pc) == brk && exec.cpu.cycles - start >= brk_after {
            println!("breakpoint {:04X} reached at cycle {} (block {} selected); the last {} instructions (PC A BC DE HL):",
                exec.cpu.pc, exec.cpu.cycles - start, exec.machine.selected_block(), recent.len());
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
        while next_key < schedule.len() && schedule[next_key].0 <= now {
            let kb = exec.machine.device_mut(kb_dev).unwrap().as_any_mut()
                .downcast_mut::<cpt_machine::CptKeyboard>().unwrap();
            kb.push_event(schedule[next_key].1);
            next_key += 1;
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
    Ok(())
}

/// Decode the real display list starting at `head` (see `cpt_machine::cpt_display`) and print
/// the screen in a frame, one numbered line per row. Runs of blank rows are collapsed. On a
/// terminal, reverse video and underline are shown with ANSI escapes; reverse-video runs are
/// also listed so they show up in plain (redirected) output.
fn print_screen(m: &mut Machine, head: u16) {
    use std::io::IsTerminal;
    let screen = cpt_machine::cpt_display::render(|a| m.read(a), head, 80);
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
        println!("{:>4} |{}|", i + 1, lines[i]);
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
    let runs: Vec<String> = screen.reverse_runs().iter()
        .map(|&(r, a, b)| if a == b { format!("row {} col {}", r + 1, a + 1) } else { format!("row {} cols {}-{}", r + 1, a + 1, b + 1) })
        .collect();
    println!("  reverse video: {}", if runs.is_empty() { "(none)".into() } else { runs.join(", ") });
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
