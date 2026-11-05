// JS glue for the CPT wasm display. Loads the wasm module (no wasm-bindgen — plain C ABI),
// runs a frame each animation tick, reads the 80x60 character grid out of wasm linear memory,
// and paints it black-on-white on the canvas. The CRT case is styled in HTML/CSS.

const CELL_W = 10; // px per character cell
const CELL_H = 16;

async function main() {
  const resp = await fetch("cpt_wasm.wasm");
  const bytes = await resp.arrayBuffer();
  // No imports needed: the module is self-contained (panic=abort, no std I/O).
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const w = instance.exports;

  w.cpt_init();

  const cols = w.cpt_cols();
  const rows = w.cpt_rows();
  const titleRow = w.cpt_title_row();
  const typingRow = w.cpt_typing_row();
  const statusRow = w.cpt_status_row();

  const canvas = document.getElementById("screen");
  canvas.width = cols * CELL_W;
  canvas.height = rows * CELL_H;
  const ctx = canvas.getContext("2d");
  ctx.font = `${CELL_H - 2}px "Courier New", monospace`;
  ctx.textBaseline = "top";

  const pagePtr = w.cpt_page_ptr();
  const pageLen = w.cpt_page_len();

  function readPage() {
    // The page buffer lives in wasm linear memory; view it fresh each frame in case memory grew.
    return new Uint8Array(w.memory.buffer, pagePtr, pageLen);
  }

  const statusEl = document.getElementById("status");

  function render() {
    w.cpt_run_frame();
    const page = readPage();

    // Page-white: pale background, dark text.
    ctx.fillStyle = "#e8f0f0"; // page-white (slight cyan tint, like the CPT 9000)
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    ctx.fillStyle = "#101418"; // near-black text
    for (let r = 0; r < rows; r++) {
      const y = r * CELL_H;
      for (let c = 0; c < cols; c++) {
        const code = page[r * cols + c];
        if (code >= 0x20 && code <= 0x7e) {
          ctx.fillText(String.fromCharCode(code), c * CELL_W + 1, y + 1);
        }
      }
    }

    // Faint region guides so the CPT page structure is visible.
    ctx.strokeStyle = "rgba(0,0,0,0.12)";
    ctx.beginPath();
    for (const row of [titleRow + 1, typingRow, typingRow + 1, statusRow]) {
      const y = row * CELL_H + 0.5;
      ctx.moveTo(0, y);
      ctx.lineTo(canvas.width, y);
    }
    ctx.stroke();

    statusEl.textContent = `retraces: ${w.cpt_retraces()}`;
    requestAnimationFrame(render);
  }

  requestAnimationFrame(render);
}

main().catch((e) => {
  document.getElementById("status").textContent = "error: " + e;
  console.error(e);
});
