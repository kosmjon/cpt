// Quick Node check: instantiate the wasm and verify the expected exports + a couple of frames.
import { readFile } from "node:fs/promises";

const bytes = await readFile(new URL("./cpt_wasm.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(bytes, {});
const w = instance.exports;

const required = [
  "cpt_init", "cpt_run_frame", "cpt_page_ptr", "cpt_page_len",
  "cpt_cols", "cpt_rows", "cpt_retraces", "memory",
];
for (const name of required) {
  if (!(name in w)) throw new Error(`missing export: ${name}`);
}

w.cpt_init();
const cols = w.cpt_cols(), rows = w.cpt_rows();
console.log(`display: ${cols}x${rows}`);

// Run enough frames to cross a few retraces and scroll.
for (let i = 0; i < 10; i++) w.cpt_run_frame();

const page = new Uint8Array(w.memory.buffer, w.cpt_page_ptr(), w.cpt_page_len());
// Decode the title row (row 0) and the first top-text row (row 1).
const rowText = (r) => {
  let s = "";
  for (let c = 0; c < cols; c++) {
    const code = page[r * cols + c];
    s += code >= 0x20 && code <= 0x7e ? String.fromCharCode(code) : " ";
  }
  return s.trimEnd();
};
console.log("retraces:", Number(w.cpt_retraces()));
console.log("row 0 :", JSON.stringify(rowText(0)));
console.log("row 1 :", JSON.stringify(rowText(1)));
console.log("row 45:", JSON.stringify(rowText(45)));
console.log("row 54:", JSON.stringify(rowText(54)));
