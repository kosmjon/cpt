// Node check of the live-audio path, with no browser:
//  1. the wasm exports the speaker interface and plays the melody frame by frame;
//  2. SpeakerAudio turns those events into correctly timed Web Audio calls (against a mock
//     AudioContext that records what was scheduled).
// Needs the wasm built and copied here first (see run-web.bat).
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import assert from "node:assert/strict";

const require = createRequire(import.meta.url);
const { SpeakerAudio, SPEAKER_LEAD } = require("./speaker-audio.js");

// ---- a mock Web Audio context that records every scheduled change ----
class MockContext {
  constructor() {
    this.currentTime = 10.0;
    this.destination = {};
    this.calls = []; // { what, value, time }
    const self = this;
    this.osc = {
      type: "",
      frequency: {
        value: 0,
        setValueAtTime: (v, t) => self.calls.push({ what: "freq", value: v, time: t }),
      },
      connect() {},
      start() { self.started = true; },
    };
    this.gainNode = {
      connect() {},
      gain: {
        value: 0,
        setTargetAtTime: (v, t) => self.calls.push({ what: "gain", value: v, time: t }),
        cancelScheduledValues() {},
        setValueAtTime() {},
      },
    };
  }
  createOscillator() { return this.osc; }
  createGain() { return this.gainNode; }
  resume() { this.resumed = true; }
}

function makeAudio(cpuHz = 2_000_000, toneHz = 1_000_000) {
  let ctx;
  const Ctor = function () { ctx = new MockContext(); return ctx; };
  const audio = new SpeakerAudio(Ctor, cpuHz, toneHz);
  audio.enable();
  return { audio, ctx };
}
const evs = (...pairs) => new Float64Array(pairs.flat());
const near = (a, b, eps = 1e-9) => assert.ok(Math.abs(a - b) < eps, `${a} !~ ${b}`);

// ---- 1. scheduling against the mock ----
{
  const { audio, ctx } = makeAudio();
  assert.equal(ctx.osc.type, "square");
  assert.ok(ctx.started && ctx.resumed);
  // A 440 Hz tone (divisor 1136) at cycle 1,000,000, silenced a quarter second (500,000 cycles) later.
  audio.schedule(evs([1_000_000, 1136], [1_500_000, 0]), 2);
  const f = ctx.calls.filter((c) => c.what === "freq");
  const g = ctx.calls.filter((c) => c.what === "gain");
  assert.equal(f.length, 1);
  near(f[0].value, 1_000_000 / 2 / 1136);
  near(f[0].time, 10.0 + SPEAKER_LEAD, 1e-9); // anchored just ahead of "now"
  assert.equal(g.length, 2);
  assert.ok(g[0].value > 0 && g[1].value === 0);
  near(g[1].time - g[0].time, 0.25, 1e-9); // emulated time, not wall time
  console.log("ok: tone starts at the anchor, stops 250 ms (500,000 cycles) later");
}

// ---- 2. later events keep their emulated spacing; being ahead of real time is fine ----
{
  const { audio, ctx } = makeAudio();
  audio.schedule(evs([0, 1911]), 1);
  ctx.currentTime += 0.001; // the page is polled again almost immediately
  audio.schedule(evs([500_000, 0], [560_000, 1703]), 2);
  const f = ctx.calls.filter((c) => c.what === "freq");
  near(f[1].time - f[0].time, 0.28, 1e-9);
  near(f[1].value, 1_000_000 / 2 / 1703);
  console.log("ok: gaps between events follow the CPU cycle clock");
}

// ---- 3. falling behind real time re-anchors instead of playing in the past ----
{
  const { audio, ctx } = makeAudio();
  audio.schedule(evs([0, 1000]), 1);
  ctx.currentTime = 50.0; // the page stalled for 40 seconds
  audio.schedule(evs([1_000_000, 500]), 1);
  const f = ctx.calls.filter((c) => c.what === "freq");
  assert.ok(f[1].time >= 50.0, `late event was scheduled in the past (${f[1].time})`);
  near(f[1].time, 50.0 + SPEAKER_LEAD, 1e-9);
  console.log("ok: a stalled page re-anchors to now");
}

// ---- 4. disabled audio schedules nothing ----
{
  const { audio, ctx } = makeAudio();
  audio.disable();
  audio.schedule(evs([0, 1136]), 1);
  assert.equal(ctx.calls.length, 0);
  console.log("ok: nothing is scheduled while sound is off");
}

// ---- 5. the real wasm module feeding the real scheduler ----
const bytes = await readFile(new URL("./cpt_wasm.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(bytes, {});
const w = instance.exports;
for (const name of ["cpt_play_melody", "cpt_speaker_count", "cpt_speaker_ptr", "cpt_cpu_hz", "cpt_tone_hz", "cpt_cycle_now"]) {
  assert.ok(name in w, `missing export ${name}`);
}
w.cpt_init();
assert.equal(w.cpt_cpu_hz(), 2_000_000);
assert.equal(w.cpt_tone_hz(), 1_000_000);

const { audio, ctx } = makeAudio(w.cpt_cpu_hz(), w.cpt_tone_hz());
for (let i = 0; i < 20; i++) { // the screen demo is silent
  w.cpt_run_frame();
  assert.equal(w.cpt_speaker_count(), 0);
}
assert.equal(w.cpt_play_melody(), 1);
audio.reanchor();
let total = 0;
for (let i = 0; i < 200; i++) { // ~3.3 s of 60 Hz frames
  w.cpt_run_frame();
  const n = w.cpt_speaker_count();
  if (n > 0) {
    audio.schedule(new Float64Array(w.memory.buffer, w.cpt_speaker_ptr(), n * 2), n);
    total += n;
  }
}
assert.equal(total, 18, "nine notes, each on and off");
const freqs = ctx.calls.filter((c) => c.what === "freq").map((c) => c.value);
const want = [1911, 1703, 1517, 1432, 1276, 1136, 1012, 956, 400].map((d) => 1_000_000 / 2 / d);
assert.equal(freqs.length, 9);
freqs.forEach((v, i) => near(v, want[i], 1e-6));
const starts = ctx.calls.filter((c) => c.what === "gain" && c.value > 0).map((c) => c.time);
near(starts[1] - starts[0], 0.28, 0.002); // 250 ms note + 30 ms gap
near(starts[8] - starts[0], 8 * 0.28, 0.01);
console.log("ok: wasm melody -> 9 notes at", freqs.map((f) => f.toFixed(1)).join(", "), "Hz");
console.log("all audio checks passed");
