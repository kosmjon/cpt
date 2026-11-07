# cpt-wasm — WebAssembly display

A minimal WebAssembly boundary that runs the CPT machine core in a browser and paints the
page-white display on a canvas, inside a styled CRT case. No `wasm-bindgen`: the module uses a
plain C ABI, and the JS glue reads the 80×60 character grid straight out of wasm linear memory.

## Build and run

```sh
# 1. Build the wasm module (requires: rustup target add wasm32-unknown-unknown)
cargo build -p cpt-wasm --target wasm32-unknown-unknown --release

# 2. Copy it next to the web files
cp target/wasm32-unknown-unknown/release/cpt_wasm.wasm crates/cpt-wasm/web/

# 3. Serve the web/ directory over HTTP (fetch() of .wasm needs a server, not file://)
#    Any static server works, e.g.:
npx serve crates/cpt-wasm/web
#    or:  python -m http.server --directory crates/cpt-wasm/web 8080
```

Open the served page; you'll see the CPT page-white screen draw the demo (title, text,
typing-line ruler, status line) and scroll via the emulated vertical-retrace interrupt.

## Quick check without a browser

```sh
node crates/cpt-wasm/web/check.mjs   # instantiates the wasm and prints decoded rows
```

## Exports (C ABI)

Display: `cpt_init`, `cpt_run_frame`, `cpt_page_ptr`, `cpt_page_len`, `cpt_cols`, `cpt_rows`,
`cpt_retraces`, and layout-row accessors. The page buffer at `cpt_page_ptr()` is
`cpt_page_len()` bytes of character codes, row-major.

Disk drives (stations 0 = A, 1 = B):
- `cpt_xfer_reserve(len) -> ptr` / `cpt_xfer_ptr()` / `cpt_xfer_len()` — a scratch buffer for
  moving disk bytes between JS and wasm.
- `cpt_disk_mount(station)` — mount the current transfer-buffer bytes as a disk in `station`.
- `cpt_disk_eject(station) -> len` — flush + detach; the image bytes land in the transfer
  buffer for download.
- `cpt_disk_mounted(station) -> 0|1` — bay state for the UI.

The web page shows two drive bays beside the screen with Insert (file picker) / Eject
(download) buttons.

## Speaker (live audio)

On the page, latch **CODE** on the special-key panel and then press **WORDPAK**: the speaker
demo (`speaker/melody.mol`, a C-major scale and a beep, assembled inside the wasm module)
plays through the Web Audio API. That click is also the user gesture browsers require before
they allow audio, so there is nothing else to turn on. The key events still reach the machine as
usual; the chord itself is a host shortcut handled by the page.

How it works:
- `cpt_run_frame` leaves the frame's speaker events in a buffer of f64 pairs (CPU cycle,
  divisor). `cpt_speaker_count()` / `cpt_speaker_ptr()` read it; `cpt_cpu_hz()`,
  `cpt_tone_hz()` and `cpt_cycle_now()` give the clocks. `cpt_play_melody()` assembles and
  starts the demo (screen, disks and keyboard are left alone; it restarts only the CPU).
- `web/speaker-audio.js` (`SpeakerAudio`) schedules those events on one square-wave
  oscillator: pitch = tone clock / (2 * divisor), divisor 0 = silence. Times come from the
  emulated clock, so the tempo is right whatever the frame pacing; if the page falls behind it
  re-anchors a little ahead of "now".
- `node crates/cpt-wasm/web/check-audio.mjs` checks the scheduling (against a mock audio
  context) and the real wasm melody end to end, with no browser.

The page was not exercised in a real browser while building this; the browser-facing code is
checked only through the Node script above.

## Notes

- Glyphs are drawn with a monospaced web font for now; the authentic CPT character-ROM font is
  a `[VERIFY]` item awaiting a ROM dump and drops in as the glyph source later.
- This is a display proof; keyboard input and the full React UI come later. The React app will
  consume the same exports.
