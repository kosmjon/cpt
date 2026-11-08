// Node check of the page itself: runs the real cpt.js against a small fake DOM. It clicks
// drive 1, "chooses" the CPT program disk image, lets animation frames run until the real
// system has started, types IN, "legal" and RETURN on the PC keyboard (with key events), and
// reads back what was drawn on the canvas. Then it ejects drive 1 and checks that the demo
// comes back. Skipped if disk/cpt.img is missing.
import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import assert from "node:assert/strict";

const require = createRequire(import.meta.url);
const imagePath = new URL("../../../disk/cpt.img", import.meta.url);
if (!existsSync(imagePath)) {
  console.log("skipping: disk/cpt.img not found (run tools/td0.py --raw)");
  process.exit(0);
}

// --- a fake DOM: just what cpt.js uses ---
function el(tag = "div") {
  const listeners = {};
  const classes = new Set();
  const e = {
    tag,
    dataset: {},
    title: "",
    innerHTML: "",
    textContent: "",
    value: "",
    children: [],
    classList: {
      add: (c) => classes.add(c),
      remove: (c) => classes.delete(c),
      toggle: (c, on) => {
        const want = on === undefined ? !classes.has(c) : on;
        if (want) classes.add(c);
        else classes.delete(c);
        return want;
      },
      contains: (c) => classes.has(c),
    },
    addEventListener: (t, f) => (listeners[t] ||= []).push(f),
    dispatch: async (t, ev = {}) => {
      for (const f of listeners[t] || []) await f(ev);
    },
    appendChild: (c) => e.children.push(c),
    click: () => e.dispatch("click"),
    querySelector: (sel) => e.sub[sel],
    sub: {},
  };
  return e;
}

// The canvas context records the last frame's text and dark rectangles by cell.
const CELL_W = 10;
const CELL_H = 16;
let frameText = new Map(); // "row,col" -> char
let frameDark = new Set(); // "row,col" of dark (ink) fills
let background = ""; // the last frame's whole-screen fill colour
const ctx = {
  fillStyle: "",
  font: "",
  textBaseline: "",
  strokeStyle: "",
  fillRect(x, y, w, h) {
    if (x === 0 && y === 0 && w > 700) {
      frameText = new Map();
      frameDark = new Set();
      background = this.fillStyle;
      return;
    }
    if (this.fillStyle === "#101418") {
      for (let c = x / CELL_W; c < (x + w) / CELL_W; c++) frameDark.add(`${y / CELL_H},${c}`);
    }
  },
  fillText(t, x, y) {
    frameText.set(`${(y - 1) / CELL_H},${(x - 1) / CELL_W}`, t);
  },
  beginPath() {},
  moveTo() {},
  lineTo() {},
  stroke() {},
};
const canvas = el("canvas");
canvas.getContext = () => ctx;
const status = el();
const fileInput = el("input");
const specialKeys = el();
const powerBtn = el("button");
powerBtn.sub[".led"] = el();
const resetBtn = el("button");
const byId = { screen: canvas, status, fileInput, specialKeys, powerBtn, resetBtn };
const drives = [0, 1].map(() => {
  const d = el();
  d.sub[".led"] = el();
  d.sub[".drive-btn"] = el("button");
  return d;
});
const windowListeners = {};
let rafCallback = null;

const document = {
  getElementById: (id) => byId[id],
  querySelector: (sel) => {
    const m = /data-station="(\d)"/.exec(sel);
    return m ? drives[Number(m[1])] : null;
  },
  createElement: (tag) => el(tag),
};
const fakeWindow = {
  addEventListener: (t, f) => (windowListeners[t] ||= []).push(f),
  AudioContext: undefined,
};

const wasmBytes = await readFile(new URL("./cpt_wasm.wasm", import.meta.url));
Object.assign(globalThis, {
  document,
  window: fakeWindow,
  requestAnimationFrame: (f) => {
    rafCallback = f;
  },
  fetch: async () => ({ arrayBuffer: async () => wasmBytes }),
});
// The page's classic scripts declare globals; load them the same way.
Object.assign(globalThis, require("./real-cpt.js"));
vm.runInThisContext(await readFile(new URL("./cpt.js", import.meta.url), "utf8"), { filename: "cpt.js" });

async function frames(n) {
  for (let i = 0; i < n; i++) {
    const f = rafCallback;
    rafCallback = null;
    assert.ok(f, "the page keeps requesting frames");
    f(performance.now());
    await new Promise((r) => setImmediate(r));
  }
}
function rowText(r) {
  let s = "";
  for (let c = 0; c < 80; c++) s += frameText.get(`${r},${c}`) ?? " ";
  return s.trimEnd();
}
async function key(code, down) {
  const ev = { code, key: "", repeat: false, ctrlKey: false, metaKey: false, altKey: false, preventDefault() {} };
  for (const f of windowListeners[down ? "keydown" : "keyup"] || []) f(ev);
}
// Real-time frames run the wall time since the last one; pause between keys so the system
// sees a typist's taps.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function tap(code) {
  await key(code, true);
  await sleep(40);
  await frames(1);
  await key(code, false);
  await sleep(80);
  await frames(1);
}

// Wait for main() to start (it awaits fetch + instantiate).
for (let i = 0; i < 50 && !rafCallback; i++) await new Promise((r) => setImmediate(r));
await frames(3);
// Powered up, idle: a black screen, waiting for a program disk in drive 1 (no demo).
assert.equal(background, "#000000", "a black screen");
assert.equal(frameText.size, 0, "nothing on it");
assert.equal(status.textContent, "insert a CPT program disk in drive 1");
assert.ok(powerBtn.sub[".led"].classList.contains("on"), "the power LED is on");
assert.equal(drives[0].classList.contains("loaded"), false);

// Insert the program disk in drive 1.
await drives[0].sub[".drive-btn"].dispatch("click");
const image = new Uint8Array(await readFile(imagePath));
fileInput.files = [{ name: "cpt.img", arrayBuffer: async () => image.buffer.slice(0) }];
await fileInput.dispatch("change");
assert.equal(drives[0].classList.contains("loaded"), true, "drive 1 shows a disk");

// Frames run the turbo start-up.
const t0 = performance.now();
let n = 0;
while (!status.textContent.startsWith("CPT system running") && n < 2000) {
  await frames(1);
  n++;
}
console.log(`started in ${n} frames, ${((performance.now() - t0) / 1000).toFixed(2)} s; status: "${status.textContent}"`);
assert.ok(status.textContent.startsWith("CPT system running"), status.textContent);
await frames(1);
assert.ok(rowText(46).startsWith("^...:....|....:....|"), `ruler drawn: "${rowText(46)}"`);
const allDark = (r) => [...Array(80).keys()].every((c) => frameDark.has(`${r},${c}`));
assert.ok(allDark(46), "the ruler row is white on black");
assert.ok(allDark(0), "the title line has no black-on-white cells (bug 1)");
assert.ok(/ 1 {4}0/.test(rowText(0)), `the status is at the title's right end (bug 2): "${rowText(0)}"`);
assert.ok(!frameDark.has("45,0"), "the typing line is black on white");
assert.ok(!rowText(47).includes(" 1    0"), "no separate status row under the ruler");

// Type IN (on the special-key panel), then "legal" and RETURN on the PC keyboard.
const inKey = specialKeys.children.find((k) => k.dataset && k.dataset.label === "IN");
assert.ok(inKey, "the IN key is on the panel");
await inKey.dispatch("click");
await sleep(80);
await frames(2);
for (const c of ["KeyL", "KeyE", "KeyG", "KeyA", "KeyL", "Enter"]) await tap(c);
// Let the filer read the document (a few seconds of emulated time, in real time).
for (let i = 0; i < 40 && !rowText(45).startsWith("T.017-5"); i++) {
  await sleep(100);
  await frames(1);
}
console.log(`row 1:  "${rowText(0)}"`);
console.log(`row 46: "${rowText(45)}"`);
assert.ok(rowText(0).startsWith("IN         1 LEGAL"), rowText(0));
assert.ok(rowText(45).startsWith("T.017-5  REV (G-1-1)    USA LEGAL"), rowText(45));
assert.ok(allDark(0), "the title line stays white on black with a prompt in it");

// UP on the panel, twice (on through the document), then DOWN (↓) on the PC keyboard (back
// toward the start): line 0 -> 2 -> 1.
const lineNo = () => Number((/(\d+)\s+(\d+)\s*([A-Z]*)\s*$/.exec(rowText(0).slice(60)) || [])[2]);
const panelKey = (label) => specialKeys.children.find((k) => k.dataset && k.dataset.label === label);
assert.ok(panelKey("UP") && panelKey("DOWN"), "UP and DOWN are on the panel");
for (let i = 0; i < 2; i++) {
  await panelKey("UP").dispatch("click");
  await sleep(300);
  await frames(2);
}
for (let i = 0; i < 10 && !(lineNo() >= 2); i++) {
  await sleep(100);
  await frames(1);
}
// At least 2: on a slow machine a stretched tap can trip the CPT's own key auto-repeat.
const afterUp = lineNo();
assert.ok(afterUp >= 2, `panel UP twice: line 2 or more ("${rowText(0)}")`);
await tap("ArrowDown");
for (let i = 0; i < 10 && !(lineNo() < afterUp); i++) {
  await sleep(100);
  await frames(1);
}
assert.ok(lineNo() < afterUp, `↓ (CPT DOWN) moves back: from ${afterUp} ("${rowText(0)}")`);
console.log(`after UP, UP, DOWN: "${rowText(0)}"`);

// The user's case: CODE + IN, Enter builds the table of contents in the upper screen (the
// cursor at its end); then PAGE, DOWN scrolls the upper screen completely into the lower one.
const codeKey = panelKey("CODE");
await codeKey.dispatch("click"); // latch CODE
await frames(1);
await inKey.dispatch("click"); // IN, then CODE is released
await sleep(80);
await frames(2);
await tap("Enter");
for (let i = 0; i < 80 && !rowText(0).startsWith("CONTENTS"); i++) {
  await sleep(100);
  await frames(1);
}
for (let i = 0; i < 80 && !(/TABLE OF CONTENTS/.test(rowText(15)) && lineNo() > 20); i++) {
  await sleep(100);
  await frames(1);
}
assert.ok(/TABLE OF CONTENTS/.test(rowText(15)), `the TOC is in the upper screen: "${rowText(15)}"`);
await panelKey("PAGE").dispatch("click");
await sleep(300);
await frames(2);
await panelKey("DOWN").dispatch("click");
for (let i = 0; i < 80 && lineNo() !== 0; i++) {
  await sleep(100);
  await frames(1);
}
assert.equal(lineNo(), 0, `PAGE, DOWN: back to line 0 ("${rowText(0)}")`);
// The system redraws the lower screen over the following frames. (Where the TOC lands
// depends on what was open: here it went in after LEGAL's first line.)
const findRow = (re, from, to = 80) => [...Array(to - from).keys()].map((i) => i + from).find((r) => re.test(rowText(r)));
for (let i = 0; i < 80 && findRow(/TABLE OF CONTENTS/, 47) === undefined; i++) {
  await sleep(100);
  await frames(1);
}
const tocRow = findRow(/TABLE OF CONTENTS/, 47);
if (tocRow === undefined) {
  for (let r = 0; r < 80; r++) if (rowText(r)) console.log(`  ${String(r + 1).padStart(2)} |${rowText(r)}`);
}
assert.ok(tocRow !== undefined && tocRow < 54, "the TOC moved into the lower screen (below the ruler)");
assert.equal(findRow(/TABLE OF CONTENTS/, 1, 46), undefined, "and left the upper screen");
// The screen is 54 rows: 7 below the ruler, nothing drawn past row 54.
assert.equal(canvas.height, 54 * CELL_H, "the canvas is 54 rows tall");
assert.equal(findRow(/\S/, 54), undefined, "nothing drawn past row 54");
// The upper screen is now empty white-on-black lines (rows 2-45), the typing line is not.
for (let r = 1; r < 45; r++) assert.ok(allDark(r), `upper screen row ${r + 1} is an empty black line`);
assert.ok(!frameDark.has("45,0"), "the typing line stays black on white");
console.log(`PAGE, DOWN: TOC at row ${tocRow + 1}; canvas ${canvas.height / CELL_H} rows`);

// SKIP on the panel (PAGE still selected): the lower screen's text goes to the bit bucket,
// leaving an empty white lower screen.
assert.ok(panelKey("SKIP"), "the SKIP key is on the panel");
await panelKey("SKIP").dispatch("click");
for (let i = 0; i < 80 && findRow(/\S/, 47, 54) !== undefined; i++) {
  await sleep(100);
  await frames(1);
}
assert.equal(findRow(/\S/, 47, 54), undefined, "PAGE, SKIP: no text below the ruler");
for (let r = 47; r < 54; r++) assert.ok(!frameDark.has(`${r},0`), `row ${r + 1} below the ruler is white`);
console.log("PAGE, SKIP: the lower screen is empty and white");

// Take the program disk out of drive 1: the system keeps running.
await drives[0].sub[".drive-btn"].dispatch("click");
await frames(2);
assert.equal(drives[0].classList.contains("loaded"), false, "drive 1 is empty");
assert.ok(status.textContent.startsWith("CPT system running"), `still running: ${status.textContent}`);
assert.ok(rowText(46).startsWith("^...:....|"), "with its screen");

// RESET with drive 1 empty: the system stops and the screen goes black, waiting for a disk.
await resetBtn.dispatch("click");
await frames(2);
assert.equal(background, "#000000", "reset, no disk: a black screen");
assert.equal(status.textContent, "insert a CPT program disk in drive 1");

// POWER off: a dark tube; a disk inserted while off waits in its drive.
await powerBtn.dispatch("click");
await frames(2);
assert.equal(status.textContent, "power is off");
assert.ok(!powerBtn.sub[".led"].classList.contains("on"), "the power LED is off");
assert.ok(resetBtn.disabled, "RESET does nothing with the power off");
await drives[0].sub[".drive-btn"].dispatch("click");
fileInput.files = [{ name: "cpt.img", arrayBuffer: async () => image.buffer.slice(0) }];
await fileInput.dispatch("change");
await frames(2);
assert.equal(drives[0].classList.contains("loaded"), true, "the disk is in drive 1");
assert.equal(status.textContent, "power is off", "and the power is still off");

// POWER on with the disk in drive 1: it boots.
await powerBtn.dispatch("click");
await frames(1);
assert.ok(status.textContent.startsWith("starting the CPT system"), `boots: ${status.textContent}`);
for (let i = 0; i < 2000 && !status.textContent.startsWith("CPT system running"); i++) await frames(1);
assert.ok(status.textContent.startsWith("CPT system running"), status.textContent);

// RESET with the disk in: it boots again.
await resetBtn.dispatch("click");
await frames(1);
assert.ok(status.textContent.startsWith("starting the CPT system"), `reset boots again: ${status.textContent}`);
console.log("power and reset: OK");
console.log("page checks OK");
process.exit(0);
