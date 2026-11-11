// Node check: boot the real CPT program disk in the wasm module through real-cpt.js (the same
// code the page uses), run the turbo start-up, open the LEGAL document with PC key codes, and
// check the decoded screen. Needs disk/cpt.img, made by setup-disks.bat (skipped if it is
// missing).
import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import assert from "node:assert/strict";

const require = createRequire(import.meta.url);
const { RealCpt, CPT_KEY } = require("./real-cpt.js");

const imagePath = new URL("../../../disk/cpt.img", import.meta.url);
if (!existsSync(imagePath)) {
  console.log("skipping: disk/cpt.img not found (run setup-disks.bat or python tools/setup_disks.py)");
  process.exit(0);
}

const wasm = await readFile(new URL("./cpt_wasm.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(wasm, {});
const w = instance.exports;
const real = new RealCpt(w);

// Not a CPT disk: refused.
assert.equal(real.boot(new Uint8Array(1000)), false, "a wrong-size image is refused");
assert.equal(real.running, false);

const image = new Uint8Array(await readFile(imagePath));
assert.equal(image.length, w.cpt_real_disk_bytes());
assert.equal(real.boot(image), true, "boots the program disk");

// Turbo start-up, frame by frame, as the page does it.
const t0 = performance.now();
let frames = 0;
while (!real.settled) {
  real.frame();
  frames++;
}
const secs = (performance.now() - t0) / 1000;
console.log(`start-up: ${(real.cycles / 2e6).toFixed(1)} s emulated in ${secs.toFixed(2)} s (${frames} frames)`);

let rows = real.screenText();
assert.equal(rows.length, 47, "the editing screen has 47 rows (the status is in the title line)");
assert.ok(rows[0].includes(" 1    0"), `status at the title's right end: ${rows[0]}`);
assert.ok(rows[46].startsWith("^...:....|....:....|"), `ruler: ${rows[46]}`);
const s = real.screen();
assert.ok(s.attrs[46 * s.cols] & 2, "the ruler is white on black");

// Type IN, "legal", RETURN with PC key codes, at a typist's pace in emulated time.
function tap(key) {
  real.press(key);
  real.runCycles(40_000);
  real.release(key);
  real.runCycles(120_000);
}
tap(CPT_KEY.IN);
for (const code of ["KeyL", "KeyE", "KeyG", "KeyA", "KeyL"]) {
  const k = real.keyForPcCode(code);
  assert.ok(k !== null, `a CPT key for ${code}`);
  tap(k);
}
tap(real.keyForPcCode("Enter"));
real.runCycles(20_000_000);

rows = real.screenText();
console.log("rows 1, 46, 48:");
for (const r of [0, 45, 47]) console.log(`  ${String(r + 1).padStart(2)} |${rows[r]}`);
assert.ok(rows[0].startsWith("IN         1 LEGAL"), rows[0]);
assert.ok(rows[45].startsWith("T.017-5  REV (G-1-1)    USA LEGAL"), rows[45]);
assert.ok(rows[47].startsWith("1   20  22  23"), rows[47]);
assert.equal(real.keyForPcCode("ArrowDown"), CPT_KEY.DOWN);
assert.equal(real.keyForPcCode("ArrowUp"), CPT_KEY.UP);
assert.equal(real.keyForPcCode("Escape"), CPT_KEY.SKIP, "Esc is SKIP");
assert.equal(real.keyForPcCode("Delete"), CPT_KEY.ERASE, "Delete is ERASE");
assert.equal(real.keyForPcCode("Insert"), null, "Insert: INSERT's key isn't known yet");
assert.equal(real.keyForPcCode("Tab"), CPT_KEY.TAB, "Tab is TAB (while the screen has focus)");
assert.equal(real.drive(0).loaded, true);
assert.equal(real.drive(1).loaded, false);

// Shift is a real key: a held SHIFT gives capitals.
assert.equal(real.keyForPcCode("ShiftLeft"), CPT_KEY.SHIFT);
assert.equal(real.keyForPcCode("Digit1"), 0x66);
assert.equal(real.keyForPcCode("BracketLeft"), null, "no [ key on the CPT");

// The program disk can come out once booted: the system keeps running.
const out = real.eject(0);
assert.equal(out.length, image.length, "ejecting drive 1 returns its image");
assert.equal(real.drive(0).loaded, false);
real.runCycles(4_000_000);
assert.equal(real.running, true, "the system runs on without its program disk");
assert.ok(real.screenText()[46].startsWith("^...:....|"), "and keeps its screen");

// Power off (halt) keeps the disks in their drives; start boots from drive 1, or waits.
real.insert(1, image);
real.halt();
assert.equal(real.running, false);
assert.equal(real.drive(1).loaded, true, "drive 2's disk stays in while the power is off");
assert.equal(real.start(), false, "no disk in drive 1: waits");
assert.equal(real.running, false);
real.insert(0, image);
assert.equal(real.start(), true, "boots from drive 1");
assert.equal(real.drive(1).loaded, true, "drive 2's disk is still in after the boot");
assert.equal(real.cycles < 1_000_000, true, "a fresh boot");

// The blank data disk the page links (web/blank-data.img) takes a page in drive 2, and keeps it.
const blank = new Uint8Array(await readFile(new URL("./blank-data.img", import.meta.url)));
assert.equal(blank.length, image.length, "the blank data disk is a full-size CPT disk");
function typeText(text) {
  for (const ch of text) tap(real.keyForPcCode(ch === " " ? "Space" : "Key" + ch.toUpperCase()));
}
function settle() {
  while (!real.settled) real.frame();
}
real.halt();
assert.equal(real.insert(0, image), true);
assert.equal(real.insert(1, blank), true, "drive 2 takes the blank data disk");
assert.equal(real.start(), true);
settle();
typeText("ab");
tap(real.keyForPcCode("Enter"));
typeText("cd");
tap(real.keyForPcCode("Enter"));
real.runCycles(2_000_000);
// OUT, then backspace twice over the station digit and type 2: "OUT 2 ZZ".
tap(CPT_KEY.OUT);
real.runCycles(3_000_000);
tap(real.keyForPcCode("Backspace"));
tap(real.keyForPcCode("Backspace"));
tap(real.keyForPcCode("Digit2"));
typeText("zz");
tap(real.keyForPcCode("Enter"));
real.runCycles(16_000_000);
assert.ok(real.screenText()[0].startsWith("OUT        2 ZZ"), `OUT to station 2: ${real.screenText()[0]}`);
const written = real.eject(1);
assert.equal(written.length, blank.length);
const changed = [];
for (let p = 0; p < 1232; p++) {
  for (let i = 0; i < 256; i++) {
    if (written[p * 256 + i] !== blank[p * 256 + i]) {
      changed.push(p);
      break;
    }
  }
}
assert.deepEqual(changed, [3, 13, 18], "the map (t0s3), the directory root (t0s13) and the page's block (t1s2)");
assert.deepEqual([...written.slice(18 * 256, 18 * 256 + 4)], [0x01, 0x5a, 0x5a, 0x02], "01 ZZ 02");
// Reboot with the written disk in drive 2 and bring the page back.
real.halt();
assert.equal(real.insert(1, written), true);
assert.equal(real.start(), true);
settle();
tap(CPT_KEY.IN);
real.runCycles(3_000_000);
tap(real.keyForPcCode("Backspace"));
tap(real.keyForPcCode("Backspace"));
tap(real.keyForPcCode("Digit2"));
typeText("zz");
tap(real.keyForPcCode("Enter"));
real.runCycles(16_000_000);
rows = real.screenText();
assert.ok(rows[0].startsWith("IN         2 ZZ"), rows[0]);
assert.ok(rows[45].startsWith("ab"), rows[45]);
assert.ok(rows[47].startsWith("cd"), rows[47]);
console.log("blank data disk: OUT 2 ZZ wrote 3 sectors; after a reboot IN 2 ZZ shows ab / cd");
console.log("real system checks OK");
