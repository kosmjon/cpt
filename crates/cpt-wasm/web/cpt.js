// The CPT 8000 workstation in the browser: the real CPT system (real-cpt.js + the wasm
// module, no wasm-bindgen: plain C ABI), its screen on a canvas, two 8-inch drives, the
// special-key panel, the PC keyboard, and POWER / RESET.
//
// Like the real machine, it powers up idle with a black screen, waiting for a program disk in
// drive 1. Inserting one boots it. Once booted, the program disk can come out and the system
// keeps running. POWER off stops the machine (the disks stay in); POWER on and RESET boot
// again from the disk in drive 1, or wait for one.

const CELL_W = 10; // px per character cell
const CELL_H = 16;

const PAGE = "#e8f0f0"; // the screen's white (slight cyan tint)
const INK = "#101418"; // its black
const DOT = "#7a8486"; // the faint dot of a system-inserted blank line
const GLASS_OFF = "#22272a"; // a powered-off tube

async function main() {
  const resp = await fetch("cpt_wasm.wasm");
  const bytes = await resp.arrayBuffer();
  // No imports needed: the module is self-contained (panic=abort, no std I/O).
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const w = instance.exports;
  const real = new RealCpt(w);

  const canvas = document.getElementById("screen");
  const ctx = canvas.getContext("2d");
  canvas.width = w.cpt_real_cols() * CELL_W;
  canvas.height = REAL_SCREEN_ROWS * CELL_H;
  ctx.font = `${CELL_H - 2}px "Courier New", monospace`;
  ctx.textBaseline = "top";
  const statusEl = document.getElementById("status");

  const ws = { w, real, powered: true };
  ws.speaker = setupSpeaker(real);
  const keys = setupKeyboard(ws);
  const drives = setupDrives(ws);
  setupPower(ws, keys, drives);
  setupSpecialKeys(ws, keys);
  const printer = setupPrinter(ws);

  function render() {
    if (!ws.powered) {
      ctx.fillStyle = GLASS_OFF;
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      statusEl.textContent = "power is off";
    } else if (real.running) {
      real.frame();
      ws.speaker.onFrame(ws.powered);
      drawRealScreen(ctx, canvas, real);
      statusEl.textContent = real.settled
        ? "CPT system running — type on your keyboard (Alt = CODE, ↑ ↓ = CPT UP / DOWN, Esc = SKIP, Delete = ERASE)"
        : `starting the CPT system… ${Math.round(real.startProgress * 100)}%`;
    } else {
      // Powered and idle: a black screen, waiting for a program disk in drive 1.
      ctx.fillStyle = "#000000";
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      statusEl.textContent = "insert a CPT program disk in drive 1";
    }
    drives.update();
    printer.update();
    requestAnimationFrame(render);
  }
  requestAnimationFrame(render);
}

// Printer A: the printout as sheets of paper, redrawn whenever the CPT has sent more, so the
// document appears line by line (the CPT sends about 110 characters a second). Pages are
// separated by form feeds in the text. The paper follows the print head unless you have
// scrolled up to read.
function setupPrinter(ws) {
  const { real } = ws;
  const feed = document.getElementById("paperFeed");
  const stateEl = document.getElementById("printerState");
  const powerBtn = document.getElementById("printerPower");
  const powerLed = powerBtn.querySelector(".led");
  const powerLabel = powerBtn.querySelector("span:last-child");
  const emptyNote = feed.querySelector(".empty");
  let shown = -1;
  let text = "";
  let lastChange = -Infinity;
  let on = true;
  // Follow the print head: keep the line being printed in view, just above the bottom of the
  // paper window, so the paper scrolls up as printing reaches the bottom. Scrolling up to
  // read stops following; scrolling back to the print head (or a new printout) resumes it.
  let follow = true;
  let head = null;
  const HEAD_MARGIN = 28; // px kept below the print head

  // Where the print head is, relative to the bottom of the visible paper (px; > 0 = below it).
  function headBelowBottom() {
    if (!head || !head.getBoundingClientRect) return null;
    return head.getBoundingClientRect().bottom - feed.getBoundingClientRect().bottom;
  }

  function followHead() {
    const below = headBelowBottom();
    if (below !== null && below > -HEAD_MARGIN) feed.scrollTop += below + HEAD_MARGIN;
  }

  feed.addEventListener("scroll", () => {
    const below = headBelowBottom();
    // In view (with some slack) = following; scrolled away from it = reading.
    if (below !== null) follow = below <= HEAD_MARGIN + 4 && below > -feed.clientHeight;
  });

  function draw(printing) {
    const keep = feed.scrollTop;
    feed.replaceChildren();
    head = null;
    if (!text) {
      feed.append(emptyNote);
      follow = true;
      return;
    }
    const pages = text.split("\f");
    pages.forEach((page, i) => {
      const sheet = document.createElement("div");
      sheet.className = "sheet";
      sheet.setAttribute("aria-label", `page ${i + 1}`);
      sheet.textContent = page;
      if (i === pages.length - 1) {
        // The print head sits after the last character struck (not after the line's
        // newline). It shows only while printing, but marks the spot to follow either way.
        sheet.textContent = page.replace(/\n+$/, "");
        head = document.createElement("span");
        head.className = "print-head";
        if (!printing) head.style.visibility = "hidden";
        sheet.append(head);
      }
      feed.append(sheet);
    });
    // Replacing the sheets resets the scroll position: put it back, then follow the head.
    feed.scrollTop = keep;
    if (follow) followHead();
  }

  powerBtn.addEventListener("click", () => {
    on = !on;
    real.setPrinterOn(on);
    powerLed.classList.toggle("on", on);
    powerLabel.textContent = on ? "ON" : "OFF";
    powerBtn.setAttribute("aria-pressed", String(on));
  });
  document.getElementById("printerTear").addEventListener("click", () => real.clearPrinter());
  document.getElementById("printerSave").addEventListener("click", () => {
    if (!text) return;
    const blob = new Blob([text.replace(/\f/g, "\f\n")], { type: "text/plain" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = "printout.txt";
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  });

  return {
    update() {
      const now = performance.now();
      const v = real.printerVersion;
      if (v !== shown) {
        shown = v;
        const next = real.printout();
        if (next !== text) {
          // A new printout (after an idle spell) brings the paper back to the print head.
          if (now - lastChange > 1500) follow = true;
          lastChange = now;
        }
        text = next;
        draw(true);
      }
      // "printing" while characters keep coming; the head goes away after a pause.
      const printing = now - lastChange < 1500;
      const label = !on ? "off" : printing ? "printing…" : text ? "ready" : "idle";
      if (stateEl.textContent !== label) {
        stateEl.textContent = label;
        if (!printing) draw(false);
      }
    },
  };
}

// Draw the real system's decoded screen: black on white, with white-on-black cells (the title
// line, the ruler, format lines, rows with no document text). A row's colour carries on to its
// right edge, as on the CPT's own display.
function drawRealScreen(ctx, canvas, real) {
  const s = real.screen();
  ctx.fillStyle = PAGE;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  for (let r = 0; r < Math.min(s.rows, REAL_SCREEN_ROWS); r++) {
    const y = r * CELL_H;
    let last = -1;
    for (let c = 0; c < s.cols; c++) {
      const a = s.attrs[r * s.cols + c];
      if (!(a & 1)) continue;
      last = c;
      const rev = (a & 2) !== 0;
      if (rev) {
        ctx.fillStyle = INK;
        ctx.fillRect(c * CELL_W, y, CELL_W, CELL_H);
      }
      const code = s.codes[r * s.cols + c];
      const ch = cptGlyph(code);
      if (code === 0x83 || code === 0x84 || code === 0x87) {
        drawRulerMark(ctx, c * CELL_W, y, code, rev);
      } else if (code === 0x80 && !rev) {
        // A column tick outside a ruler: double spacing's system-inserted blank line, a tiny
        // faint dot (smaller and fainter than a period).
        // At mid-height.
        // 2 x 2 px in a light grey: lighter than a period, which is drawn in ink.
        ctx.fillStyle = DOT;
        ctx.fillRect(c * CELL_W + 4, y + CELL_H / 2 - 1, 2, 2);
      } else if (code === 0x90) {
        // A coded space (CODE + SPACE): the same dot, a little below mid-height.
        ctx.fillStyle = rev ? PAGE : DOT;
        ctx.fillRect(c * CELL_W + 4, y + CELL_H / 2 + 2, 2, 2);
      } else if (code === 0x8d) {
        // A coded return (CODE + RETURN): a small faint arrow pointing left, at mid-height.
        ctx.fillStyle = rev ? PAGE : DOT;
        const x = c * CELL_W;
        const m = y + CELL_H / 2;
        ctx.fillRect(x + 2, m, 7, 1); // the shaft
        ctx.fillRect(x + 3, m - 1, 1, 3); // the head
        ctx.fillRect(x + 4, m - 2, 1, 5);
      } else if (ch !== " ") {
        ctx.fillStyle = rev ? PAGE : INK;
        ctx.fillText(ch, c * CELL_W + 1, y + 1);
      }
      // Underlined: underlined text, or the cursor / a menu's selection, which blinks because
      // the CPT itself turns the underline on and off.
      if (a & 4) {
        ctx.fillStyle = rev ? PAGE : INK;
        ctx.fillRect(c * CELL_W, y + CELL_H - 2, CELL_W, 2);
      }
    }
    if (last >= 0 && last < s.cols - 1 && s.attrs[r * s.cols + last] & 2) {
      ctx.fillStyle = INK;
      ctx.fillRect((last + 1) * CELL_W, y, (s.cols - last - 1) * CELL_W, CELL_H);
    }
  }
}

// The ruler's marks (per the user), in a CELL_W x CELL_H cell at (x, y):
//   cursor's column (87H) - a wide (7 px) vertical bar with a pointed top
//   margin (83H, left and right alike) - a narrower, flat-topped vertical bar
//   tab stop (84H) - a short, narrow T in the lower half of the cell
// Drawn in the row's light colour (the ruler is white on black).
function drawRulerMark(ctx, x, y, code, rev) {
  ctx.fillStyle = rev ? PAGE : INK;
  if (code === 0x87) {
    // A 7 px bar under a 3-step point (1, 3, 5 px), centred on x + 4.
    ctx.fillRect(x + 4, y + 1, 1, 1);
    ctx.fillRect(x + 3, y + 2, 3, 1);
    ctx.fillRect(x + 2, y + 3, 5, 1);
    ctx.fillRect(x + 1, y + 4, 7, CELL_H - 5);
  } else if (code === 0x83) {
    ctx.fillRect(x + 3, y + 3, 3, CELL_H - 4);
  } else {
    ctx.fillRect(x + 3, y + 8, 4, 1); // the T's bar
    ctx.fillRect(x + 4, y + 9, 2, CELL_H - 11); // its stem
  }
}

// The speaker: the system's tone changes (8253 counter 0) played on a Web Audio square wave.
// Browsers only allow audio after a user gesture, so sound starts with the first click or key
// on the page. During the faster-than-real-time start-up, each frame's changes play from "now"
// with their real spacing (so the start-up chime keeps its note lengths); real-time running
// re-anchors the timing. Nothing plays with the power off.
function setupSpeaker(real) {
  const Ctor = typeof window !== "undefined" ? window.AudioContext || window.webkitAudioContext : undefined;
  const audio = new SpeakerAudio(Ctor, REAL_CPU_HZ, real.toneHz);
  let wanted = false;
  function gesture() {
    if (!wanted) return;
    if (audio.enable()) wanted = false;
  }
  if (typeof window !== "undefined") {
    for (const t of ["pointerdown", "keydown"]) window.addEventListener(t, gesture, true);
  }
  wanted = true;
  let wasTurbo = true;
  return {
    // Called once per frame, after the machine has run: play its tone changes.
    onFrame(powered) {
      if (!real.running) return;
      const { buf, count } = real.speakerTake();
      // A frame that ran ahead of real time: play its batch from now. While a tone sounds the
      // page runs in real time, keeping the anchor, so a note's on and off keep their spacing
      // across frames. When real time begins with no tone sounding (start-up has settled),
      // emulated time has run ahead of the old anchor, so start a fresh one.
      if (real.turbo || (wasTurbo && !real.toneOn)) audio.reanchor();
      wasTurbo = real.turbo;
      if (powered && count > 0) audio.schedule(buf, count);
    },
    // A new run (reset, power on): forget the old timing and silence any held tone.
    restart() {
      audio.disable();
      wanted = true;
      gesture(); // power and reset are clicks, so this is inside a gesture
    },
    off() {
      audio.disable();
      wanted = true;
    },
  };
}

// The two 8-inch drives. Each has one button with a red activity LED:
//   empty            -> open the file chooser (insert a CPT disk image, 315,392 bytes)
//   has disk, idle   -> eject it (a disk the system wrote to is offered as a download)
//   has disk, active -> nothing (can't eject mid-read; the LED is lit)
// A program disk in drive 1 boots the system when the power is on and nothing is running.
function setupDrives(ws) {
  const { w, real } = ws;
  const fileInput = document.getElementById("fileInput");
  const statusEl = document.getElementById("status");
  const names = [null, null];
  let pending = null;

  const driveEl = (d) => document.querySelector(`.drive[data-station="${d}"]`);
  const ledEl = (d) => driveEl(d).querySelector(".led");
  const btnEl = (d) => driveEl(d).querySelector(".drive-btn");

  function download(bytes, name) {
    const blob = new Blob([bytes], { type: "application/octet-stream" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = name;
    a.click();
    URL.revokeObjectURL(a.href);
  }

  function refresh(d) {
    const loaded = real.drive(d).loaded;
    const el = driveEl(d);
    el.classList.toggle("loaded", loaded);
    el.title = loaded
      ? `${names[d] || "(disk)"} — press to eject`
      : d === 0
        ? "empty — press to insert a disk (a CPT program disk boots the system)"
        : "empty — press to insert a disk";
  }

  for (const d of [0, 1]) {
    btnEl(d).addEventListener("click", () => {
      const st = real.drive(d);
      if (!st.loaded) {
        pending = d;
        fileInput.value = "";
        fileInput.click();
      } else if (!st.active) {
        const bytes = real.eject(d);
        if (bytes && st.modified) download(bytes, names[d] || `drive${d + 1}.img`);
        names[d] = null;
        refresh(d);
      }
    });
  }

  fileInput.addEventListener("change", async () => {
    const file = fileInput.files[0];
    const d = pending;
    pending = null;
    if (!file || d === null) return;
    const buf = new Uint8Array(await file.arrayBuffer());
    if (!real.insert(d, buf)) {
      statusEl.textContent = `${file.name}: not a CPT disk image (${w.cpt_real_disk_bytes()} bytes)`;
      return;
    }
    names[d] = file.name;
    // A program disk in drive 1 of an idle, powered machine: boot it.
    if (d === 0 && ws.powered && !real.running) real.start();
    refresh(0);
    refresh(1);
  });

  refresh(0);
  refresh(1);
  return {
    update() {
      for (const d of [0, 1]) {
        ledEl(d).classList.toggle("on", ws.powered && real.drive(d).active);
        driveEl(d).classList.toggle("loaded", real.drive(d).loaded);
      }
    },
  };
}

// POWER (with a green LED) and RESET, under the special keys.
//   POWER off: the machine stops and the screen goes dark; the disks stay in their drives.
//   POWER on, RESET: boot from the disk in drive 1, or wait (black screen) for one.
function setupPower(ws, keys, drives) {
  const { real } = ws;
  const powerBtn = document.getElementById("powerBtn");
  const resetBtn = document.getElementById("resetBtn");
  const led = powerBtn.querySelector(".led");
  function show() {
    led.classList.toggle("on", ws.powered);
    powerBtn.title = ws.powered ? "power is on — press to turn off" : "power is off — press to turn on";
    resetBtn.disabled = !ws.powered;
  }
  powerBtn.addEventListener("click", () => {
    keys.releaseAll();
    ws.powered = !ws.powered;
    if (ws.powered) {
      ws.speaker.restart();
      real.start();
    } else {
      ws.speaker.off();
      real.halt();
    }
    show();
    drives.update();
  });
  resetBtn.addEventListener("click", () => {
    if (!ws.powered) return;
    keys.releaseAll();
    ws.speaker.restart();
    real.start(); // stops the running system first; waits if drive 1 is empty
  });
  show();
}

// The CPT special keys, 5 columns x 5 rows (from special-keys.md), listed column-major.
// `mod: true` marks a latching modifier (CODE, held for chords like CODE+IN); `lock: true` a
// lock-on/lock-off key (none at present: CLEAR TAB SET turned out to be momentary).
const SPECIAL_KEY_COLUMNS = [
  [{ t: "CODE", mod: true }, { t: "CLEAR\nTAB\nSET" }, { t: "HOLD" }, { t: "MAR\nSET" }, { t: "MAR\nREL" }],
  [{ t: "CODE", mod: true }, { t: "INSERT" }, { t: "PROG" }, { t: "MOVE" }, { t: "ERASE" }],
  [{ t: "STOP" }, { t: "SKIP" }, { t: "ADJ" }, { t: "UP" }, { t: "DOWN" }],
  [{ t: "CHAR" }, { t: "WORD" }, { t: "LINE" }, { t: "PARA" }, { t: "PAGE" }],
  [{ t: "IN" }, { t: "OUT" }, { t: "PRINT" }, { t: "WORDPAK" }, { t: "STAPLE", icon: "staple" }],
];

// Key caps drawn rather than written. The key under WORDPAK is marked with a staple shape:
// a bar across the top with a short leg down at each end, one connected line.
const KEY_ICONS = {
  staple:
    '<svg viewBox="0 0 24 14" width="24" height="14" aria-hidden="true" focusable="false">' +
    '<path d="M3 13 V2 H21 V13" fill="none" stroke="currentColor" stroke-width="2" ' +
    'stroke-linecap="square" stroke-linejoin="miter"/></svg>',
};

// The keyboard paths into the system: the PC keyboard (physical keys -> CPT key numbers) and
// the special-key panel (labels -> CPT key numbers), with CODE latched on the panel forming a
// chord with the next key from either.
function setupKeyboard(ws) {
  const { real } = ws;
  const latched = []; // { key, el } for each latched panel modifier
  const held = new Set(); // PC KeyboardEvent.code values held down

  const press = (k) => { if (real.running) real.press(k); };
  const release = (k) => { if (real.running) real.release(k); };

  function releaseLatched() {
    for (const m of latched) {
      release(m.key);
      m.el.classList.remove("held");
    }
    latched.length = 0;
  }

  // Release everything held (power off, reset, the window losing focus).
  function releaseAll() {
    for (const code of held) {
      const k = real.keyForPcCode(code);
      if (k !== null) release(k);
    }
    held.clear();
    releaseLatched();
  }

  function pcKey(e, down) {
    if (!real.running || e.ctrlKey || e.metaKey) return; // leave browser shortcuts alone
    const k = real.keyForPcCode(e.code);
    if (k === null) return;
    e.preventDefault();
    if (down) {
      if (held.has(e.code)) return; // auto-repeat: the CPT does its own
      held.add(e.code);
      press(k);
    } else {
      held.delete(e.code);
      release(k);
      releaseLatched(); // a real key completes a chord with a latched panel CODE
    }
  }
  window.addEventListener("keydown", (e) => pcKey(e, true));
  window.addEventListener("keyup", (e) => pcKey(e, false));
  window.addEventListener("blur", releaseAll);

  return { press, release, latched, releaseLatched, releaseAll };
}

// Build the special-key panel. Keys press with a tactile animation; CODE latches until the
// next key; locking keys toggle a locked-on look. Keys not yet identified on the real system
// say so in their tooltip and do nothing.
function setupSpecialKeys(ws, keys) {
  const grid = document.getElementById("specialKeys");
  if (!grid) return;
  for (let row = 0; row < 5; row++) {
    for (let col = 0; col < 5; col++) {
      const spec = SPECIAL_KEY_COLUMNS[col][row];
      if (!spec) {
        const blank = document.createElement("div");
        blank.className = "key blank";
        grid.appendChild(blank);
        continue;
      }
      const el = document.createElement("button");
      el.className = "key";
      el.dataset.label = spec.t.replace(/\n/g, " ");
      if (spec.icon) {
        el.innerHTML = KEY_ICONS[spec.icon];
        el.setAttribute("aria-label", el.dataset.label.toLowerCase());
        el.title = el.dataset.label.toLowerCase();
      } else {
        el.innerHTML = spec.t.split("\n").join("<br>");
      }
      const k = SPECIAL_LABEL_KEYS[el.dataset.label];
      if (k === undefined) el.title = "not yet identified on the real CPT system";

      if (spec.mod) {
        // CODE: a click latches it down; it is released after the next key, or by clicking
        // it again (a two-finger chord with one mouse).
        el.addEventListener("click", () => {
          const i = keys.latched.findIndex((m) => m.el === el);
          if (i >= 0) {
            keys.release(k);
            el.classList.remove("held");
            keys.latched.splice(i, 1);
          } else {
            keys.press(k);
            el.classList.add("held");
            keys.latched.push({ key: k, el });
          }
        });
      } else {
        // A panel key is down for as long as the pointer holds it, like a real key, so the
        // CPT's own auto-repeat works: its keyboard task repeats a held key that's on its
        // repeat list (UP, DOWN, ...) after a short delay, then every 3 ticks of its 100 Hz
        // clock. A press is at least a typist's 40 ms tap. A click with no pointer press
        // (Enter or Space on the focused button, or a script) is a tap.
        const TAP_MS = 40;
        let downAt = 0; // when the pointer pressed the key; 0 while it's up
        let skipClick = false; // the click that follows a pointer press
        const pressKey = () => {
          el.classList.add("pressed");
          if (spec.lock) el.classList.toggle("locked");
          if (k !== undefined) keys.press(k);
        };
        const releaseKey = () => {
          el.classList.remove("pressed");
          if (k === undefined) return;
          keys.release(k);
          keys.releaseLatched();
        };
        const up = () => {
          if (!downAt) return;
          const held = performance.now() - downAt;
          downAt = 0;
          if (held >= TAP_MS) releaseKey();
          else setTimeout(releaseKey, TAP_MS - held);
        };
        el.addEventListener("pointerdown", (e) => {
          if ((e.button !== undefined && e.button !== 0) || downAt) return;
          downAt = performance.now();
          skipClick = true;
          // Keep the pointer on this key until it's released, even if it slides off.
          if (el.setPointerCapture && e.pointerId !== undefined) el.setPointerCapture(e.pointerId);
          pressKey();
        });
        el.addEventListener("pointerup", up);
        el.addEventListener("pointercancel", up);
        el.addEventListener("lostpointercapture", up);
        window.addEventListener("blur", up);
        // A long touch would open the context menu instead of holding the key.
        el.addEventListener("contextmenu", (e) => e.preventDefault());
        el.addEventListener("click", () => {
          if (skipClick) {
            skipClick = false;
            return;
          }
          pressKey();
          setTimeout(releaseKey, TAP_MS);
        });
      }
      grid.appendChild(el);
    }
  }
}

main().catch((e) => {
  document.getElementById("status").textContent = "error: " + e;
  console.error(e);
});
