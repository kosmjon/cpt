//! WebAssembly boundary for the CPT machine — a minimal, `wasm-bindgen`-free surface.
//!
//! The browser needs very little from the core: run some cycles, then read an 80x60 grid of
//! character codes to paint. That interface is just integers plus a byte buffer, so we expose
//! a plain C ABI and let the JS glue read the page buffer out of wasm linear memory directly.
//! This avoids the `wasm-bindgen` toolchain while keeping the Rust core unchanged.
//!
//! Exposed functions (see `web/cpt.js` for the JS side):
//!   * `cpt_init()`            — create the machine, attach the display, load the demo program.
//!   * `cpt_run_frame()`       — run ~one video frame of CPU cycles (drives a retrace).
//!   * `cpt_page_ptr()`        — pointer to the page buffer (ROWS*COLS bytes of char codes).
//!   * `cpt_cols()/cpt_rows()` — display dimensions.
//!   * layout row accessors    — so the UI can mark title/typing/status regions.
//!   * `cpt_retraces()`        — count of retrace interrupts raised (a frame counter).

use cpt_machine::disk::Station;
use cpt_machine::{
    DiskController, DiskImage, DisplayDevice, Executive, Geometry, KeyboardDevice, Machine,
    MachineConfig, SpeakerDevice,
};
use i8080_cpu::Cpu;
use std::cell::RefCell;

/// The page buffer the JS side reads. Updated by `cpt_run_frame`. Fixed size = ROWS*COLS.
const COLS: usize = cpt_machine::COLS;
const ROWS: usize = cpt_machine::ROWS;

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static PAGE: RefCell<Vec<u8>> = RefCell::new(vec![0u8; ROWS * COLS]);
    /// Scratch buffer for transferring disk bytes between JS and wasm (mount in / eject out).
    static XFER: RefCell<Vec<u8>> = RefCell::new(Vec::new());
    /// The speaker events produced by the most recent frame, as (cycle, divisor) f64 pairs.
    /// Replaced every frame, so it never grows if the page ignores it.
    static SPK: RefCell<Vec<f64>> = RefCell::new(Vec::new());
}

/// The speaker demo: a C-major scale and a beep, in MOL80, assembled at run time.
const MELODY_SRC: &str = include_str!("../../../speaker/melody.mol");

struct State {
    exec: Executive,
    cycles_per_frame: u64,
    /// Index of the disk controller among the machine's devices.
    disk_dev: usize,
    /// Index of the keyboard device among the machine's devices.
    kbd_dev: usize,
    /// Index of the speaker device among the machine's devices.
    speaker_dev: usize,
    cpu_clock_hz: u32,
    tone_clock_hz: u32,
    geom: Geometry,
}

impl State {
    fn disk(&mut self) -> Option<&mut DiskController> {
        self.exec
            .machine
            .device_mut(self.disk_dev)?
            .as_any_mut()
            .downcast_mut::<DiskController>()
    }

    fn speaker(&mut self) -> Option<&mut SpeakerDevice> {
        self.exec
            .machine
            .device_mut(self.speaker_dev)?
            .as_any_mut()
            .downcast_mut::<SpeakerDevice>()
    }

    fn keyboard(&mut self) -> Option<&mut KeyboardDevice> {
        self.exec
            .machine
            .device_mut(self.kbd_dev)?
            .as_any_mut()
            .downcast_mut::<KeyboardDevice>()
    }
}

fn station_of(n: u32) -> Station {
    if n == 0 {
        Station::A
    } else {
        Station::B
    }
}

/// Initialize the machine: attach the display and load the built-in demo program.
#[no_mangle]
pub extern "C" fn cpt_init() {
    let cfg = MachineConfig::default();
    let geom = Geometry::default();
    let mut machine = Machine::new(cfg.clone());
    machine.attach_display(DisplayDevice::new(cfg.retrace_period_cycles, cfg.retrace_vector));
    // Attach the disk controller (both stations start empty). It is device index 1 (the display
    // is index 0).
    let disk_dev = machine.device_count();
    machine.attach_device(Box::new(DiskController::new(cfg.disk_base_port, geom)));
    // Attach the interrupt-driven keyboard (device index 2).
    let kbd_dev = machine.device_count();
    machine.attach_device(Box::new(KeyboardDevice::new(
        cfg.keyboard_base_port,
        cfg.keyboard_vector,
    )));
    // Attach the one-channel speaker (guessed interface; see cpt-machine's speaker module).
    let speaker_dev = machine.device_count();
    machine.attach_device(Box::new(SpeakerDevice::new(
        cfg.speaker_base_port,
        cfg.speaker_tone_clock_hz,
    )));
    machine.mem.load_bank0(0x0100, &cpt_machine::demo::display_demo());

    let mut cpu = Cpu::new();
    cpu.pc = 0x0100;
    cpu.sp = 0x7F00;

    let state = State {
        exec: Executive::new(cpu, machine),
        // One video frame of CPU time == one retrace period, so each frame drives ~one scroll.
        cycles_per_frame: cfg.retrace_period_cycles as u64,
        disk_dev,
        kbd_dev,
        speaker_dev,
        cpu_clock_hz: cfg.cpu_clock_hz,
        tone_clock_hz: cfg.speaker_tone_clock_hz,
        geom,
    };
    STATE.with(|s| *s.borrow_mut() = Some(state));
    SPK.with(|b| b.borrow_mut().clear());
}

/// Run approximately one video frame of CPU cycles, then refresh the page buffer.
#[no_mangle]
pub extern "C" fn cpt_run_frame() {
    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.exec.run_cycles(state.cycles_per_frame);
            // Hand this frame's speaker events to the page as (cycle, divisor) pairs.
            let events = state.speaker().map(|s| s.take_events()).unwrap_or_default();
            SPK.with(|b| {
                let mut b = b.borrow_mut();
                b.clear();
                for e in &events {
                    b.push(e.cycle as f64);
                    b.push(e.divisor as f64);
                }
            });
            if let Some(d) = state.exec.machine.display_mut() {
                let snap = d.snapshot();
                PAGE.with(|p| {
                    p.borrow_mut().copy_from_slice(&snap.cells);
                });
            }
        }
    });
}

/// Pointer to the page buffer in wasm linear memory (ROWS*COLS bytes of character codes).
#[no_mangle]
pub extern "C" fn cpt_page_ptr() -> *const u8 {
    PAGE.with(|p| p.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn cpt_page_len() -> usize {
    ROWS * COLS
}

#[no_mangle]
pub extern "C" fn cpt_cols() -> usize {
    COLS
}

#[no_mangle]
pub extern "C" fn cpt_rows() -> usize {
    ROWS
}

/// Count of retrace interrupts raised so far (frame counter for the UI).
#[no_mangle]
pub extern "C" fn cpt_retraces() -> u64 {
    STATE.with(|s| {
        s.borrow_mut()
            .as_mut()
            .and_then(|st| st.exec.machine.display_mut().map(|d| d.retrace_count))
            .unwrap_or(0)
    })
}

// --- speaker ---
//
// Each frame `cpt_run_frame` leaves the speaker's new tone changes in a buffer of f64 pairs
// (CPU cycle, divisor). The page schedules them on a Web Audio oscillator at the right
// moments: cycle / cpu_hz seconds of emulated time. A divisor of 0 means silence.

/// Number of speaker events produced by the most recent frame.
#[no_mangle]
pub extern "C" fn cpt_speaker_count() -> usize {
    SPK.with(|b| b.borrow().len() / 2)
}

/// Pointer to the speaker event buffer: `cpt_speaker_count()` pairs of f64 (cycle, divisor).
#[no_mangle]
pub extern "C" fn cpt_speaker_ptr() -> *const f64 {
    SPK.with(|b| b.borrow().as_ptr())
}

/// CPU clock in Hz, to convert event cycles to seconds.
#[no_mangle]
pub extern "C" fn cpt_cpu_hz() -> f64 {
    STATE.with(|s| s.borrow().as_ref().map(|st| st.cpu_clock_hz as f64).unwrap_or(0.0))
}

/// The speaker divider's clock in Hz: tone frequency = this / (2 * divisor).
#[no_mangle]
pub extern "C" fn cpt_tone_hz() -> f64 {
    STATE.with(|s| s.borrow().as_ref().map(|st| st.tone_clock_hz as f64).unwrap_or(0.0))
}

/// Total CPU cycles run so far (exact as an f64 for centuries of emulated time).
#[no_mangle]
pub extern "C" fn cpt_cycle_now() -> f64 {
    STATE.with(|s| s.borrow().as_ref().map(|st| st.exec.cpu.cycles as f64).unwrap_or(0.0))
}

/// Start the speaker demo (a C-major scale and a beep, from `speaker/melody.mol`): assemble it,
/// load it, and restart the CPU on it with interrupts off. The display, disks and keyboard are
/// left as they are; the demo just stops the screen demo's CPU program. Returns 1 on success.
#[no_mangle]
pub extern "C" fn cpt_play_melody() -> u32 {
    let out = mol80asm::assemble_source("melody.mol", MELODY_SRC);
    let Some(asm) = out.assembly else { return 0 };
    STATE.with(|s| {
        let mut sref = s.borrow_mut();
        let Some(st) = sref.as_mut() else { return 0 };
        st.exec.machine.mem.load_bank0(asm.origin, &asm.bytes);
        // A fresh CPU: pc = the reset vector, interrupts off, so the demo's HLT is terminal.
        let mut cpu = Cpu::new();
        cpu.pc = asm.origin;
        cpu.sp = 0x7F00;
        cpu.cycles = st.exec.cpu.cycles; // keep the cycle clock monotonic for the speaker
        st.exec.cpu = cpu;
        1
    })
}

// --- layout row accessors, so the UI can style the regions ---

fn layout() -> cpt_machine::Layout {
    // The demo uses the default layout; expose it without needing a live display borrow.
    cpt_machine::Layout::default()
}

#[no_mangle]
pub extern "C" fn cpt_title_row() -> usize {
    layout().title_row
}
#[no_mangle]
pub extern "C" fn cpt_typing_row() -> usize {
    layout().typing_row
}
#[no_mangle]
pub extern "C" fn cpt_status_row() -> usize {
    layout().status.0
}

// --- disk drives A and B ---
//
// Mount flow:  JS calls cpt_xfer_reserve(len) -> gets a pointer -> writes the disk file bytes
// there -> calls cpt_disk_mount(station). Eject flow: JS calls cpt_disk_eject(station), which
// copies the (flushed) image bytes into the transfer buffer and returns their length; JS then
// reads them from cpt_xfer_ptr() to offer a download.

/// Reserve `len` bytes in the transfer buffer and return a pointer JS can write into.
#[no_mangle]
pub extern "C" fn cpt_xfer_reserve(len: usize) -> *mut u8 {
    XFER.with(|x| {
        let mut b = x.borrow_mut();
        b.clear();
        b.resize(len, 0);
        b.as_mut_ptr()
    })
}

/// Pointer to the transfer buffer (valid until the next reserve).
#[no_mangle]
pub extern "C" fn cpt_xfer_ptr() -> *const u8 {
    XFER.with(|x| x.borrow().as_ptr())
}

/// Length of the transfer buffer.
#[no_mangle]
pub extern "C" fn cpt_xfer_len() -> usize {
    XFER.with(|x| x.borrow().len())
}

/// Mount the current transfer-buffer contents as a disk image in `station` (0=A, 1=B).
/// The bytes are padded/truncated to the configured geometry. Returns 1 on success.
#[no_mangle]
pub extern "C" fn cpt_disk_mount(station: u32) -> u32 {
    STATE.with(|s| {
        let mut sref = s.borrow_mut();
        let Some(st) = sref.as_mut() else { return 0 };
        let geom = st.geom;
        let bytes = XFER.with(|x| x.borrow().clone());
        let mut img = DiskImage::blank(geom);
        // Load the mounted bytes WITHOUT marking the image modified (mounting isn't a write).
        img.load_bytes(&bytes);
        if let Some(ctrl) = st.disk() {
            ctrl.mount(station_of(station), img);
            1
        } else {
            0
        }
    })
}

/// Eject `station`, copying its (flushed) image bytes into the transfer buffer. Returns the
/// number of bytes available (0 if nothing was mounted).
#[no_mangle]
pub extern "C" fn cpt_disk_eject(station: u32) -> usize {
    STATE.with(|s| {
        let mut sref = s.borrow_mut();
        let Some(st) = sref.as_mut() else { return 0 };
        let target = station_of(station);
        let Some(ctrl) = st.disk() else { return 0 };
        // Safety: refuse to eject while the drive is actively reading/writing (LED lit),
        // mirroring the physical eject button.
        if ctrl.is_active(target) {
            return 0;
        }
        match ctrl.eject(target) {
            Some(img) => {
                let bytes = img.bytes().to_vec();
                let len = bytes.len();
                XFER.with(|x| *x.borrow_mut() = bytes);
                len
            }
            None => 0,
        }
    })
}

/// 1 if `station` has a disk mounted, else 0.
#[no_mangle]
pub extern "C" fn cpt_disk_mounted(station: u32) -> u32 {
    STATE.with(|s| {
        s.borrow_mut()
            .as_mut()
            .and_then(|st| {
                let target = station_of(station);
                st.disk().map(|c| c.is_mounted(target) as u32)
            })
            .unwrap_or(0)
    })
}

// --- keyboard ---
//
// The host (JS) captures real PC keystrokes for the non-special keys and forwards them as CPT
// key codes via these two functions. The special-key panel calls the same key-down with the
// special key's code. (The PC-key -> CPT-code mapping is [VERIFY]; JS uses a placeholder.)

/// Deliver a key **press** (make) event with CPT key `code`.
#[no_mangle]
pub extern "C" fn cpt_key_down(code: u32) {
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            if let Some(kbd) = st.keyboard() {
                kbd.push_key(code as u8);
            }
        }
    });
}

/// Deliver a key **release** (break) event with CPT key `code` (N-key rollover).
#[no_mangle]
pub extern "C" fn cpt_key_up(code: u32) {
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            if let Some(kbd) = st.keyboard() {
                kbd.push_key_up(code as u8);
            }
        }
    });
}

/// 1 if `station`'s mounted image has been modified (written) since it was mounted, else 0.
/// The UI uses this to decide whether ejecting should offer the image for download.
#[no_mangle]
pub extern "C" fn cpt_disk_modified(station: u32) -> u32 {
    STATE.with(|s| {
        s.borrow_mut()
            .as_mut()
            .and_then(|st| {
                let target = station_of(station);
                st.disk()
                    .map(|c| c.is_modified(target) as u32)
            })
            .unwrap_or(0)
    })
}

/// 1 if `station`'s activity LED is lit (recent read/write), else 0.
#[no_mangle]
pub extern "C" fn cpt_disk_active(station: u32) -> u32 {
    STATE.with(|s| {
        s.borrow_mut()
            .as_mut()
            .and_then(|st| {
                let target = station_of(station);
                st.disk().map(|c| c.is_active(target) as u32)
            })
            .unwrap_or(0)
    })
}

// --- the real CPT system, booted from a real program disk ---
//
// Separate from the demo machine above (which the page no longer runs). The workstation has
// two drives. While no system is running (before the first boot, or with the power off), the
// disks wait in PARKED; once a system runs, they are in its floppy controller.
//
// - `cpt_real_insert(drive)` puts the transfer-buffer image (77 x 16 x 256 = 315,392 bytes,
//   e.g. from tools/td0.py --raw) in drive 0 or 1; `cpt_real_eject(drive)` takes it out.
//   Disks can go in and out while the system runs (the program disk can come out once it
//   has booted).
// - `cpt_real_start()` boots a fresh system from the disk in drive 1 (power on, or reset);
//   `cpt_real_halt()` stops it and leaves the disks in their drives (power off).
//
// The page runs the system faster than real time until it has settled
// (`cpt_real_settled_cycles`), then in real time. The screen is decoded from the display list
// (`cpt_machine::cpt_display`) into two buffers of REAL_ROWS x REAL_COLS bytes: character
// codes, and attributes (bit 0 = a cell is there, bit 1 = white on black).

use cpt_machine::cpt_display::WINDOW_COLS as REAL_COLS;
use cpt_machine::CptSystem;

/// Rows the screen buffers hold: the real screen's 54 (the title line, 45 rows through the
/// typing line, the ruler and 7 below).
const REAL_ROWS: usize = cpt_machine::cpt_display::SCREEN_ROWS;

thread_local! {
    static REAL: RefCell<Option<CptSystem>> = const { RefCell::new(None) };
    /// The disks in drives 1 and 2 while no system is running.
    static PARKED: RefCell<[Option<DiskImage>; 2]> = const { RefCell::new([None, None]) };
    static REAL_CODES: RefCell<Vec<u8>> = RefCell::new(vec![0u8; REAL_ROWS * REAL_COLS]);
    static REAL_ATTRS: RefCell<Vec<u8>> = RefCell::new(vec![0u8; REAL_ROWS * REAL_COLS]);
}

/// Size of a CPT program disk image in bytes.
#[no_mangle]
pub extern "C" fn cpt_real_disk_bytes() -> usize {
    cpt_machine::cpt_floppy::cpt_geometry().total_bytes()
}

/// Put the transfer-buffer image in `drive` (0 = drive 1, 1 = drive 2), replacing any disk
/// there. Returns 1 on success, 0 if the bytes are not a CPT disk image.
#[no_mangle]
pub extern "C" fn cpt_real_insert(drive: u32) -> u32 {
    let bytes = XFER.with(|x| x.borrow().clone());
    let geom = cpt_machine::cpt_floppy::cpt_geometry();
    if bytes.len() != geom.total_bytes() {
        return 0;
    }
    let mut img = DiskImage::blank(geom);
    img.load_bytes(&bytes);
    let d = drive as usize & 1;
    REAL.with(|r| match r.borrow_mut().as_mut() {
        Some(sys) => sys.floppy().mount(d, img),
        None => PARKED.with(|p| p.borrow_mut()[d] = Some(img)),
    });
    1
}

/// Take the disk out of `drive`; its bytes go to the transfer buffer. Returns the length, or 0
/// if the drive was empty. The system (if running) carries on without it.
#[no_mangle]
pub extern "C" fn cpt_real_eject(drive: u32) -> usize {
    let d = drive as usize & 1;
    let img = REAL.with(|r| match r.borrow_mut().as_mut() {
        Some(sys) => sys.floppy().eject(d),
        None => PARKED.with(|p| p.borrow_mut()[d].take()),
    });
    match img {
        Some(img) => {
            let bytes = img.bytes().to_vec();
            let len = bytes.len();
            XFER.with(|x| *x.borrow_mut() = bytes);
            len
        }
        None => 0,
    }
}

/// Stop the system (power off, or the first half of a reset). The disks stay in their drives.
#[no_mangle]
pub extern "C" fn cpt_real_halt() {
    REAL.with(|r| {
        if let Some(mut sys) = r.borrow_mut().take() {
            let disks = sys.take_disks();
            PARKED.with(|p| *p.borrow_mut() = disks);
        }
    });
}

/// Boot a fresh system from the disk in drive 1 (power on, or reset). Any running system is
/// stopped first. Returns 1 if it booted, 0 if drive 1 is empty (the screen stays dark,
/// waiting for a program disk).
#[no_mangle]
pub extern "C" fn cpt_real_start() -> u32 {
    cpt_real_halt();
    PARKED.with(|p| {
        let mut p = p.borrow_mut();
        let Some(disk) = p[0].take() else { return 0 };
        let mut sys = CptSystem::boot_disk(disk, cpt_machine::cpt_system::READ_DELAY_CYCLES);
        if let Some(second) = p[1].take() {
            sys.floppy().mount(1, second);
        }
        REAL.with(|r| *r.borrow_mut() = Some(sys));
        1
    })
}

/// 1 if the real system is running.
#[no_mangle]
pub extern "C" fn cpt_real_running() -> u32 {
    REAL.with(|r| r.borrow().is_some() as u32)
}

/// Run the real system for about `cycles` CPU cycles.
#[no_mangle]
pub extern "C" fn cpt_real_run(cycles: f64) {
    REAL.with(|r| {
        if let Some(sys) = r.borrow_mut().as_mut() {
            sys.exec.run_cycles(cycles.max(0.0) as u64);
        }
    });
}

/// CPU cycles the real system has run.
#[no_mangle]
pub extern "C" fn cpt_real_cycles() -> f64 {
    REAL.with(|r| r.borrow().as_ref().map(|s| s.exec.cpu.cycles as f64).unwrap_or(0.0))
}

/// Cycles after which the started system has settled and takes keys.
#[no_mangle]
pub extern "C" fn cpt_real_settled_cycles() -> f64 {
    cpt_machine::cpt_system::SETTLED_CYCLES as f64
}

/// Decode the current screen into the code/attribute buffers. Returns the number of rows.
#[no_mangle]
pub extern "C" fn cpt_real_screen() -> usize {
    REAL.with(|r| {
        let mut rref = r.borrow_mut();
        let Some(sys) = rref.as_mut() else { return 0 };
        let screen = sys.screen(REAL_ROWS);
        REAL_CODES.with(|c| {
            REAL_ATTRS.with(|a| {
                let (mut c, mut a) = (c.borrow_mut(), a.borrow_mut());
                c.fill(0);
                a.fill(0);
                for (ri, row) in screen.rows.iter().enumerate().take(REAL_ROWS) {
                    for (ci, cell) in row.iter().enumerate().take(REAL_COLS) {
                        c[ri * REAL_COLS + ci] = cell.code;
                        a[ri * REAL_COLS + ci] = 1 | (cell.reverse as u8) << 1;
                    }
                }
            })
        });
        screen.rows.len().min(REAL_ROWS)
    })
}

#[no_mangle]
pub extern "C" fn cpt_real_codes_ptr() -> *const u8 {
    REAL_CODES.with(|c| c.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn cpt_real_attrs_ptr() -> *const u8 {
    REAL_ATTRS.with(|a| a.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn cpt_real_cols() -> usize {
    REAL_COLS
}

#[no_mangle]
pub extern "C" fn cpt_real_max_rows() -> usize {
    REAL_ROWS
}

/// Send a key event: `key` is a CPT key number (01-7F), `down` 1 = press, 0 = release.
#[no_mangle]
pub extern "C" fn cpt_real_key(key: u32, down: u32) {
    REAL.with(|r| {
        if let Some(sys) = r.borrow_mut().as_mut() {
            let k = key as u8 & 0x7F;
            if down != 0 { sys.keyboard().press(k) } else { sys.keyboard().release(k) }
        }
    });
}

/// The CPT key that types character `ch` (from the program's own key table): the key number,
/// plus 0x100 if SHIFT is needed. -1 if no key types it.
#[no_mangle]
pub extern "C" fn cpt_real_key_for_char(ch: u32) -> i32 {
    match char::from_u32(ch).and_then(cpt_machine::cpt_keyboard::key_for_char) {
        Some((k, shift)) => k as i32 | (shift as i32) << 8,
        None => -1,
    }
}

// --- the real system's speaker (8253 counter 0) ---
//
// `cpt_real_speaker_take()` moves the speaker changes since the last call into a buffer of f64
// pairs (CPU cycle, count) and returns how many there are; `cpt_real_speaker_ptr()` points at
// it. A count of 0 means silence; otherwise the speaker plays a square wave of
// 2 MHz / count. With `cpt_real_tone_hz()` (4 MHz) the page's SpeakerAudio plays
// tone_hz / (2 * count), the same pitch.

thread_local! {
    static REAL_SPK: RefCell<Vec<f64>> = RefCell::new(Vec::new());
}

#[no_mangle]
pub extern "C" fn cpt_real_speaker_take() -> usize {
    let events = REAL.with(|r| match r.borrow_mut().as_mut() {
        Some(sys) => sys.pit().take_speaker(),
        None => Vec::new(),
    });
    REAL_SPK.with(|b| {
        let mut b = b.borrow_mut();
        b.clear();
        for (cycle, count) in &events {
            b.push(*cycle as f64);
            b.push(*count as f64);
        }
    });
    events.len()
}

/// 1 if the speaker is sounding a tone now (the page runs in real time while it is).
#[no_mangle]
pub extern "C" fn cpt_real_speaker_on() -> u32 {
    REAL.with(|r| r.borrow_mut().as_mut().map_or(0, |sys| sys.pit().tone_on() as u32))
}

#[no_mangle]
pub extern "C" fn cpt_real_speaker_ptr() -> *const f64 {
    REAL_SPK.with(|b| b.borrow().as_ptr())
}

/// Twice the 8253's clock, so SpeakerAudio's tone_hz / (2 * count) is the speaker's pitch.
#[no_mangle]
pub extern "C" fn cpt_real_tone_hz() -> f64 {
    2.0 * cpt_machine::cpt_interrupts::PIT_CLOCK_HZ as f64
}

/// Drive state for the UI: bit 0 = a disk is in, bit 1 = it has been written, bit 2 = it is
/// being read or written right now. `drive` 0 = drive 1, 1 = drive 2. Works with or without a
/// running system.
#[no_mangle]
pub extern "C" fn cpt_real_drive_state(drive: u32) -> u32 {
    let d = drive as usize & 1;
    REAL.with(|r| match r.borrow_mut().as_mut() {
        Some(sys) => {
            let f = sys.floppy();
            let Some(img) = f.image(d) else { return 0 };
            let active = f.busy() && f.selected_drive() == d;
            1 | (img.is_modified() as u32) << 1 | (active as u32) << 2
        }
        None => PARKED.with(|p| match &p.borrow()[d] {
            Some(img) => 1 | (img.is_modified() as u32) << 1,
            None => 0,
        }),
    })
}
