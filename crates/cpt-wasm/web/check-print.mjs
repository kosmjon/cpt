// Node check: print a document on the real CPT in the wasm module through real-cpt.js (the
// same code the page's printer pane uses). The printout grows line by line as the CPT prints,
// both pages of the letter come out, and the paper outlives power off until it is torn off.
// Needs disk/cpt.img, made by setup-disks.bat (skipped if it is missing).
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

const bytes = await readFile(new URL("./cpt_wasm.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(bytes, {});
const w = instance.exports;
const real = new RealCpt(w);

assert.equal(real.printout(), "", "nothing printed before power on");
assert.equal(real.boot(new Uint8Array(await readFile(imagePath))), true);
while (!real.settled) real.frame();

function tap(key) {
  real.press(key);
  real.runCycles(40_000);
  real.release(key);
  real.runCycles(120_000);
}
// IN rc-l1 RETURN, then PRINT RETURN.
tap(CPT_KEY.IN);
for (const code of ["KeyR", "KeyC", "Minus", "KeyL", "Digit1"]) tap(real.keyForPcCode(code));
tap(real.keyForPcCode("Enter"));
real.runCycles(16_000_000);
tap(CPT_KEY.PRINT);
real.runCycles(6_000_000);
tap(real.keyForPcCode("Enter"));

// Line by line: in 2-second steps the printout keeps getting longer.
const lengths = [];
let version = real.printerVersion;
for (let i = 0; i < 8; i++) {
  real.runCycles(4_000_000);
  assert.notEqual(real.printerVersion, version, `more printed in step ${i}`);
  version = real.printerVersion;
  lengths.push(real.printout().split("\n").length);
}
console.log("printout lines after each 2 s:", lengths.join(" "));
assert.ok(lengths.every((n, i) => i === 0 || n >= lengths[i - 1]), "it only grows");
assert.ok(lengths[7] > lengths[0] + 5, "several lines come out over the steps");
assert.ok(real.printout().startsWith("           August 19, 1987\n"), real.printout().slice(0, 80));

// The rest of page 1, a new sheet, and page 2.
real.runCycles(100_000_000);
const text = real.printout();
const pages = text.split("\f");
assert.equal(pages.length, 2, "two pages");
assert.ok(pages[0].includes("\n           Mr. Carl Wales\n"));
assert.ok(pages[1].includes("           ROY CLESTER\n"), pages[1]);

// Power off and on: the paper stays. Tear off clears it.
real.halt();
assert.equal(real.printout(), text, "the printout outlives power off");
real.clearPrinter();
assert.equal(real.printout(), "", "torn off");
console.log("printer checks OK: two pages,", text.length, "characters");
