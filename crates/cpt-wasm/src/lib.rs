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
