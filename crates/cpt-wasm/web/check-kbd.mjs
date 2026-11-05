// Node check: key events pushed through the wasm boundary reach the keyboard device queue.
// (We can't easily observe the internal queue from JS, but we can confirm the exports exist
// and run without error, and that the machine keeps running after key input.)
import { readFile } from "node:fs/promises";

const bytes = await readFile(new URL("./cpt_wasm.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(bytes, {});
const w = instance.exports;

for (const name of ["cpt_key_down", "cpt_key_up"]) {
  if (!(name in w)) throw new Error(`missing export: ${name}`);
}

w.cpt_init();
// Simulate typing "HI" and a special key.
w.cpt_key_down(0x48); w.cpt_key_up(0x48); // H
w.cpt_key_down(0x49); w.cpt_key_up(0x49); // I
w.cpt_key_down(0xe0); w.cpt_key_up(0xe0); // a special key code

// Run a few frames; nothing should crash and the display still advances.
for (let i = 0; i < 5; i++) w.cpt_run_frame();
console.log("key exports OK; retraces:", Number(w.cpt_retraces()));
