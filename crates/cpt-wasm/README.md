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

## Notes

- Glyphs are drawn with a monospaced web font for now; the authentic CPT character-ROM font is
  a `[VERIFY]` item awaiting a ROM dump and drops in as the glyph source later.
- This is a display proof; keyboard input and the full React UI come later. The React app will
  consume the same exports.
