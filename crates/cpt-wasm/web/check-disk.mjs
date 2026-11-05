// Node check for the disk API across the wasm boundary: mount bytes into A, verify mounted,
// eject and read them back.
import { readFile } from "node:fs/promises";

const bytes = await readFile(new URL("./cpt_wasm.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(bytes, {});
const w = instance.exports;
w.cpt_init();

const A = 0, B = 1;
console.log("A mounted at start:", w.cpt_disk_mounted(A)); // 0

// Build a fake disk image: 4 blocks * 128 bytes, filled with 0xC7.
const SIZE = 4 * 128;
const ptr = w.cpt_xfer_reserve(SIZE);
new Uint8Array(w.memory.buffer, ptr, SIZE).fill(0xc7);
const ok = w.cpt_disk_mount(A);
console.log("mount A ok:", ok, " mounted:", w.cpt_disk_mounted(A)); // 1, 1

// Eject A: bytes come back in the transfer buffer (padded to full geometry).
const len = w.cpt_disk_eject(A);
const out = new Uint8Array(w.memory.buffer, w.cpt_xfer_ptr(), len);
console.log("eject A len:", len, " mounted now:", w.cpt_disk_mounted(A)); // >0, 0
console.log("first bytes:", out[0].toString(16), out[1].toString(16)); // c7 c7
console.log("all first 512 are 0xC7:", out.slice(0, 512).every((b) => b === 0xc7));
