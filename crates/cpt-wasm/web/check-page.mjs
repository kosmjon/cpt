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
  console.log("skipping: disk/cpt.img not found (run setup-disks.bat or python tools/setup_disks.py)");
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
    attrs: {},
    setAttribute: (n, v) => (e.attrs[n] = String(v)),
    getAttribute: (n) => (n in e.attrs ? e.attrs[n] : null),
    appendChild: (c) => e.children.push(c),
    append: (...cs) => e.children.push(...cs),
    replaceChildren: (...cs) => (e.children = cs),
    scrollTop: 0,
    scrollHeight: 0,
    clientHeight: 0,
    style: {},
    getBoundingClientRect: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
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
let frameMarks = new Map(); // "row,col" -> colour of a shape drawn inside that cell
const ctx = {
  fillStyle: "",
  font: "",
  textBaseline: "",
  strokeStyle: "",
  fillRect(x, y, w, h) {
    if (x === 0 && y === 0 && w > 700) {
      frameText = new Map();
      frameDark = new Set();
      frameMarks = new Map();
      background = this.fillStyle;
      return;
    }
    if (this.fillStyle === "#101418" && w % CELL_W === 0) {
      for (let c = x / CELL_W; c < (x + w) / CELL_W; c++) frameDark.add(`${y / CELL_H},${c}`);
    } else if (w < CELL_W) {
      // A shape inside one cell (a ruler mark, a faint dot): note its cell and colour.
      frameMarks.set(`${Math.floor(y / CELL_H)},${Math.floor(x / CELL_W)}`, this.fillStyle);
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
// The printer pane.
const printerPower = el("button");
printerPower.sub[".led"] = el();
printerPower.sub["span:last-child"] = el();
const paperFeed = el();
paperFeed.sub[".empty"] = el();
const printerState = el();
// The sample program disk button, and what fetch("cpt2489.img") serves (set by the check).
const sampleDisk = el("button");
let sampleBytes = null;
const byId = {
  sampleDisk,
  screen: canvas, status, fileInput, specialKeys, powerBtn, resetBtn,
  printerPower, printerTear: el("button"), printerSave: el("button"), paperFeed, printerState,
};
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
// A mock Web Audio context: it records what the page schedules (pitch changes and gain
// changes, with their audio-clock times).
const audioLog = []; // { what: "freq" | "gain", value, t }
class MockAudioContext {
  constructor() {
    this.destination = {};
  }
  get currentTime() {
    return performance.now() / 1000;
  }
  resume() {}
  createOscillator() {
    return {
      type: "",
      frequency: { value: 0, setValueAtTime: (v, t) => audioLog.push({ what: "freq", value: v, t }) },
      connect() {},
      start() {},
    };
  }
  createGain() {
    return {
      gain: {
        value: 0,
        setTargetAtTime: (v, t) => audioLog.push({ what: "gain", value: v, t }),
        setValueAtTime: (v, t) => audioLog.push({ what: "gain", value: v, t }),
        cancelScheduledValues() {},
      },
      connect() {},
    };
  }
}
const fakeWindow = {
  addEventListener: (t, f) => (windowListeners[t] ||= []).push(f),
  AudioContext: MockAudioContext,
};

const wasmBytes = await readFile(new URL("./cpt_wasm.wasm", import.meta.url));
Object.assign(globalThis, {
  document,
  window: fakeWindow,
  requestAnimationFrame: (f) => {
    rafCallback = f;
  },
  // The wasm module, or the sample disk (whatever sampleBytes holds; null = not published).
  fetch: async (url) =>
    url === "cpt2489.img"
      ? sampleBytes
        ? { ok: true, status: 200, arrayBuffer: async () => sampleBytes.buffer.slice(0) }
        : { ok: false, status: 404 }
      : { ok: true, status: 200, arrayBuffer: async () => wasmBytes },
});
// The page's classic scripts declare globals; load them the same way.
Object.assign(globalThis, require("./speaker-audio.js"), require("./real-cpt.js"));
vm.runInThisContext(await readFile(new URL("./cpt.js", import.meta.url), "utf8"), { filename: "cpt.js" });

async function frames(n) {
  for (let i = 0; i < n; i++) {
    const f = rafCallback;
    rafCallback = null;
    assert.ok(f, `the page keeps requesting frames (status: "${status.textContent}")`);
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

// Wait for main() to start (it awaits fetch + instantiate, which is slow on a busy machine).
for (let i = 0; i < 1000 && !rafCallback; i++) await sleep(10);
await frames(3);
// Powered up, idle: a black screen, waiting for a program disk in drive 1 (no demo), with
// "PLEASE INSERT PROGRAM DISK" white on black in the middle: row 27 of 54, columns 28-53.
function insertMessageShown() {
  const msg = "PLEASE INSERT PROGRAM DISK";
  const col = Math.floor((80 - msg.length) / 2);
  return rowText(26) === " ".repeat(col) + msg && ctx.fillStyle === "#e8f0f0" && frameText.size === msg.replace(/ /g, "").length;
}
assert.equal(background, "#000000", "a black screen");
assert.ok(insertMessageShown(), `the message, centred (row 27: "${rowText(26)}")`);
assert.equal(status.textContent, "insert a CPT program disk in drive 1");
assert.ok(powerBtn.sub[".led"].classList.contains("on"), "the power LED is on");
assert.equal(drives[0].classList.contains("loaded"), false);

// The first click on the page is the gesture that lets the page start audio.
for (const f of windowListeners.pointerdown || []) f({});
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

// The start-up chime (sound 1): 2000 Hz for 100 ms, then 1000 Hz for 250 ms, at true length
// even though start-up runs ahead of real time (the page drops to real time while it sounds).
{
  const on = audioLog.filter((e) => e.what === "gain" && e.value > 0);
  const off = audioLog.filter((e) => e.what === "gain" && e.value === 0);
  const freqs = audioLog.filter((e) => e.what === "freq").map((e) => Math.round(e.value));
  assert.deepEqual(freqs.slice(0, 2), [2000, 1000], `chime pitches: ${freqs}`);
  const first = on[0].t;
  const firstOff = off.find((e) => e.t > first).t;
  const second = on.find((e) => e.t > firstOff).t;
  const secondOff = off.find((e) => e.t > second).t;
  const ms = (x) => Math.round(x * 1000);
  console.log(`chime: 2000 Hz for ${ms(firstOff - first)} ms, 1000 Hz for ${ms(secondOff - second)} ms`);
  // Within one 10 ms tick: the system times notes on its 100 Hz tick.
  assert.ok(Math.abs(ms(firstOff - first) - 100) <= 10, "the 2000 Hz note lasts 100 ms");
  assert.ok(Math.abs(ms(secondOff - second) - 250) <= 10, "the 1000 Hz note lasts 250 ms");
}
await frames(1);
// The ruler: the cursor's column mark in column 1 and the every-5th marks (columns 5, 15) are
// drawn as white shapes; the other ticks are characters.
const rulerDrawn = () =>
  rowText(46).startsWith(" ... ....|.... ....|") &&
  ["46,0", "46,4", "46,14"].every((cell) => frameMarks.get(cell) === "#e8f0f0");
assert.ok(rulerDrawn(), `ruler drawn: "${rowText(46)}", column 1 mark ${frameMarks.get("46,0")}`);
const allDark = (r) => [...Array(80).keys()].every((c) => frameDark.has(`${r},${c}`));
assert.ok(allDark(46), "the ruler row is white on black");
assert.ok(allDark(0), "the title line has no black-on-white cells (bug 1)");
assert.ok(/ 1 {4}0/.test(rowText(0)), `the status is at the title's right end (bug 2): "${rowText(0)}"`);
assert.ok(!frameDark.has("45,0"), "the typing line is black on white");
assert.ok(!rowText(47).includes(" 1    0"), "no separate status row under the ruler");

// Type IN (on the special-key panel), then "legal" and RETURN on the PC keyboard. IN clicks
// (sound 0: 1333 Hz for 50 ms), in real time now.
const inKey = specialKeys.children.find((k) => k.dataset && k.dataset.label === "IN");
assert.ok(inKey, "the IN key is on the panel");
const logBefore = audioLog.length;
await inKey.dispatch("click");
await sleep(80);
await frames(2);
for (let i = 0; i < 10 && !audioLog.slice(logBefore).some((e) => e.what === "freq"); i++) {
  await sleep(50);
  await frames(1);
}
const click = audioLog.slice(logBefore).find((e) => e.what === "freq");
assert.ok(click && Math.round(click.value) === 1333, `IN clicks at 1333 Hz (${click && click.value})`);
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
const staple = panelKey("STAPLE");
assert.ok(staple, "the staple key is on the panel, under WORDPAK");
assert.ok(staple.innerHTML.startsWith("<svg"), "drawn as a staple, not written");
assert.equal(staple.getAttribute("aria-label"), "staple", "with a name for screen readers");
assert.equal(staple.title, "staple", "and identified (not 'not yet identified')");
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

// Holding a panel key holds the CPT key, so the CPT's own auto-repeat runs: UP held about a
// second moves on many lines (a tap moves one).
const beforeHold = lineNo();
const upKey = panelKey("UP");
await upKey.dispatch("pointerdown", { button: 0 });
const holdStart = performance.now();
while (performance.now() - holdStart < 1200) {
  await sleep(50);
  await frames(1);
}
await upKey.dispatch("pointerup", {});
await upKey.dispatch("click"); // the click that follows a pointer press is not another tap
for (let i = 0; i < 6; i++) {
  await sleep(100);
  await frames(1);
}
const afterHold = lineNo();
assert.ok(afterHold - beforeHold >= 5, `UP held: repeats (line ${beforeHold} -> ${afterHold}, "${rowText(0)}")`);
// DOWN held repeats too, back to where the hold started (the steps below expect it).
const downKey = panelKey("DOWN");
await downKey.dispatch("pointerdown", { button: 0 });
const downStart = performance.now();
while (lineNo() > beforeHold && performance.now() - downStart < 4000) {
  await sleep(20);
  await frames(1);
}
await downKey.dispatch("pointerup", {});
await downKey.dispatch("click");
for (let i = 0; i < 6; i++) {
  await sleep(100);
  await frames(1);
}
assert.ok(lineNo() <= beforeHold, `DOWN held: back to line ${beforeHold} or before ("${rowText(0)}")`);
console.log(`UP held 1.2 s: line ${beforeHold} -> ${afterHold}; DOWN held: back to ${lineNo()}`);

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

// SKIP (PAGE still selected), from the PC's Esc key: the lower screen's text goes to the bit
// bucket, leaving an empty white lower screen. The page keeps Esc from the browser.
assert.ok(panelKey("SKIP"), "the SKIP key is on the panel");
let escKept = false;
for (const f of windowListeners.keydown || []) {
  f({ code: "Escape", key: "Escape", repeat: false, ctrlKey: false, metaKey: false, altKey: false, preventDefault() { escKept = true; } });
}
await sleep(60);
await frames(1);
await key("Escape", false);
assert.ok(escKept, "Esc is taken by the page (preventDefault)");

// Tab: always the CPT's TAB, wherever the focus is (the page keeps it from the browser).
function tabDown() {
  let kept = false;
  for (const f of windowListeners.keydown || []) {
    f({ code: "Tab", key: "Tab", repeat: false, ctrlKey: false, metaKey: false, altKey: false, preventDefault() { kept = true; } });
  }
  return kept;
}
document.activeElement = resetBtn;
assert.ok(tabDown(), "Tab goes to the CPT even with a button focused");
await key("Tab", false);
document.activeElement = undefined;
console.log("Tab: always the CPT's TAB");
for (let i = 0; i < 80 && findRow(/\S/, 47, 54) !== undefined; i++) {
  await sleep(100);
  await frames(1);
}
assert.equal(findRow(/\S/, 47, 54), undefined, "PAGE, SKIP: no text below the ruler");
for (let r = 47; r < 54; r++) assert.ok(!frameDark.has(`${r},0`), `row ${r + 1} below the ruler is white`);
console.log("PAGE, Esc (SKIP): the lower screen is empty and white");

// Take the program disk out of drive 1: the system keeps running.
await drives[0].sub[".drive-btn"].dispatch("click");
await frames(2);
assert.equal(drives[0].classList.contains("loaded"), false, "drive 1 is empty");
assert.ok(status.textContent.startsWith("CPT system running"), `still running: ${status.textContent}`);
// (Only that the ruler is still there. A single frame can catch the system redrawing it, so
// give it a few frames.)
const rulerThere = () => allDark(46) && rowText(46).includes("....|....") && frameMarks.size > 0;
for (let i = 0; i < 40 && !rulerThere(); i++) {
  await sleep(50);
  await frames(1);
}
assert.ok(rulerThere(), `with its screen: "${rowText(46)}"`);

// RESET with drive 1 empty: the system stops and the screen goes black, waiting for a disk.
await resetBtn.dispatch("click");
await frames(2);
assert.equal(background, "#000000", "reset, no disk: a black screen");
assert.ok(insertMessageShown(), `reset, no disk: the message (row 27: "${rowText(26)}")`);
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

// LOAD SAMPLE PROGRAM DISK. With drive 1 full it says so and leaves the disk alone.
await sampleDisk.dispatch("click");
assert.equal(status.textContent, "drive 1 already has a disk: eject it first");
// Empty drive 1 (power off, eject), power on: the idle screen.
await powerBtn.dispatch("click");
await frames(1);
await drives[0].sub[".drive-btn"].dispatch("click");
await powerBtn.dispatch("click");
await frames(2);
assert.ok(insertMessageShown(), "idle, waiting for a disk");
// Not published (local run without setup-disks): a message, no disk.
await sampleDisk.dispatch("click");
assert.ok(status.textContent.startsWith("could not load cpt2489.img (HTTP 404)"), status.textContent);
assert.equal(drives[0].classList.contains("loaded"), false);
// Published: it goes in drive 1 and the system boots from it.
sampleBytes = image;
await sampleDisk.dispatch("click");
await frames(1);
assert.equal(drives[0].classList.contains("loaded"), true, "the sample disk is in drive 1");
assert.ok(status.textContent.startsWith("starting the CPT system"), `the sample disk boots: ${status.textContent}`);
console.log("sample program disk button: OK");
console.log("page checks OK");
process.exit(0);
