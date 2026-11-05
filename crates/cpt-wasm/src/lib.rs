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

use cpt_machine::{DisplayDevice, Executive, Machine, MachineConfig};
use i8080_cpu::Cpu;
use std::cell::RefCell;

/// The page buffer the JS side reads. Updated by `cpt_run_frame`. Fixed size = ROWS*COLS.
const COLS: usize = cpt_machine::COLS;
const ROWS: usize = cpt_machine::ROWS;

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static PAGE: RefCell<Vec<u8>> = RefCell::new(vec![0u8; ROWS * COLS]);
}

struct State {
    exec: Executive,
    cycles_per_frame: u64,
}

/// Initialize the machine: attach the display and load the built-in demo program.
#[no_mangle]
pub extern "C" fn cpt_init() {
    let cfg = MachineConfig::default();
    let mut machine = Machine::new(cfg.clone());
    machine.attach_display(DisplayDevice::new(cfg.retrace_period_cycles, cfg.retrace_vector));
    machine.mem.load_bank0(0x0100, &cpt_machine::demo::display_demo());

    let mut cpu = Cpu::new();
    cpu.pc = 0x0100;
    cpu.sp = 0x7F00;

    let state = State {
        exec: Executive::new(cpu, machine),
        // One video frame of CPU time == one retrace period, so each frame drives ~one scroll.
        cycles_per_frame: cfg.retrace_period_cycles as u64,
    };
    STATE.with(|s| *s.borrow_mut() = Some(state));
}

/// Run approximately one video frame of CPU cycles, then refresh the page buffer.
#[no_mangle]
pub extern "C" fn cpt_run_frame() {
    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.exec.run_cycles(state.cycles_per_frame);
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
