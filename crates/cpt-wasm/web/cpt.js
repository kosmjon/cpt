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

  // --- disk drives ---
  const drives = setupDrives(w);

  // --- special-key cluster ---
  setupSpecialKeys(w);

  // --- real PC keyboard for the non-special (typewriter) keys ---
  setupHostKeyboard(w);

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

    drives.updateLeds();

    statusEl.textContent = `retraces: ${w.cpt_retraces()}`;
    requestAnimationFrame(render);
  }

  requestAnimationFrame(render);
}

// Wire the two 8-inch drives. Each has one vertical button with a red activity LED. The
// button's action depends on state:
//   empty            -> open the file chooser (insert)
//   has disk, idle   -> eject (flush + download the image)
//   has disk, active -> do nothing (can't eject mid-I/O; the LED is lit)
// The LED tracks cpt_disk_active(station), polled each frame via the returned updateLeds().
function setupDrives(w) {
  const fileInput = document.getElementById("fileInput");
  const names = [null, null]; // filename per station (0=A, 1=B)
  let pendingStation = null;

  const driveEl = (s) => document.querySelector(`.drive[data-station="${s}"]`);
  const ledEl = (s) => driveEl(s).querySelector(".led");
  const btnEl = (s) => driveEl(s).querySelector(".drive-btn");

  function refresh(s) {
    const mounted = w.cpt_disk_mounted(s) === 1;
    const drive = driveEl(s);
    drive.classList.toggle("loaded", mounted);
    drive.title = mounted
      ? `${names[s] || "(disk)"} — press to eject`
      : "empty — press to insert a disk";
  }

  function insert(s) {
    pendingStation = s;
    fileInput.value = "";
    fileInput.click();
  }

  function eject(s) {
    // Only offer a download if the disk was actually written while mounted — so ejecting an
    // unmodified disk doesn't force an unwanted save dialog.
    const wasModified = w.cpt_disk_modified(s) === 1;
    const len = w.cpt_disk_eject(s);
    if (len === 0) return; // refused (active) or nothing mounted
    if (wasModified) {
      const bytes = new Uint8Array(w.memory.buffer, w.cpt_xfer_ptr(), len).slice();
      const blob = new Blob([bytes], { type: "application/octet-stream" });
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = names[s] || `drive${s + 1}.img`;
      a.click();
      URL.revokeObjectURL(a.href);
    }
    names[s] = null;
    refresh(s);
  }

  // The single button: behavior depends on state.
  for (const s of [0, 1]) {
    btnEl(s).addEventListener("click", () => {
      const mounted = w.cpt_disk_mounted(s) === 1;
      const active = w.cpt_disk_active(s) === 1;
      if (!mounted) {
        insert(s);
      } else if (!active) {
        eject(s);
      }
      // mounted && active -> do nothing (safety)
    });
  }

  fileInput.addEventListener("change", async () => {
    const file = fileInput.files[0];
    if (!file || pendingStation === null) return;
    const buf = new Uint8Array(await file.arrayBuffer());
    const ptr = w.cpt_xfer_reserve(buf.length);
    new Uint8Array(w.memory.buffer, ptr, buf.length).set(buf);
    if (w.cpt_disk_mount(pendingStation) === 1) names[pendingStation] = file.name;
    refresh(pendingStation);
    pendingStation = null;
  });

  refresh(0);
  refresh(1);

  return {
    updateLeds() {
      for (const s of [0, 1]) {
        ledEl(s).classList.toggle("on", w.cpt_disk_active(s) === 1);
      }
    },
  };
}

// The CPT special keys, 5 columns x 5 rows (from special-keys.md). Each entry is the keycap
// label (lines split on "\n"); `lock: true` marks a locking key; null is the blank position.
// Listed column-major here, matching the spec's columns.
// `mod: true` marks a latching modifier (held down to form chords like CODE+IN); `lock: true`
// marks a lock-on/lock-off key (e.g. CLEAR TAB SET).
const SPECIAL_KEY_COLUMNS = [
  // Column 1
  [{ t: "CODE", mod: true }, { t: "CLEAR\nTAB\nSET", lock: true }, { t: "HOLD" }, { t: "MAR\nSET" }, { t: "MAR\nREL" }],
  // Column 2
  [{ t: "CODE", mod: true }, { t: "INSERT" }, { t: "PROG" }, { t: "MOVE" }, { t: "ERASE" }],
  // Column 3
  [{ t: "STOP" }, { t: "SKIP" }, { t: "ADJ" }, { t: "UP" }, { t: "DOWN" }],
  // Column 4
  [{ t: "CHAR" }, { t: "WORD" }, { t: "LINE" }, { t: "PARA" }, { t: "PAGE" }],
  // Column 5
  [{ t: "IN" }, { t: "OUT" }, { t: "PRINT" }, { t: "WORDPAK" }, null],
];

// Build the special-key panel. Keys render in row-major order (CSS grid), press with a tactile
// animation, and locking keys toggle a locked-on state. Clicks are not yet wired to the machine
// (no keyboard device / known key codes yet) — they log for now.
// Shared latched-modifier state, so both the on-screen panel and the host PC keyboard honor
// chords (e.g. latch CODE on screen, then type a letter -> CODE+letter). Each entry is
// { code, el } for a currently-held modifier.
const latchedMods = [];

// Release all latched modifiers (send their break events) and clear the held state + visuals.
// Called after a non-modifier key is struck, completing any chord (the modifier's make was
// already sent when it latched, so the downstream software sees modifier-down ... key ...
// modifier-up, exactly like a real two-finger chord).
function releaseLatchedMods(w) {
  for (const m of latchedMods) {
    w.cpt_key_up(m.code);
    if (m.el) m.el.classList.remove("held");
  }
  latchedMods.length = 0;
}

function setupSpecialKeys(w) {
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
      const key = document.createElement("button");
      key.className = "key";
      key.innerHTML = spec.t.split("\n").join("<br>");
      key.dataset.label = spec.t.replace(/\n/g, " ");

      // Assign a placeholder CPT key code (0xE0+) per special key. [VERIFY] real codes.
      const code = 0xe0 + row * 5 + col;
      key.dataset.code = code;

      if (spec.mod) {
        // Latching modifier (CODE): click latches it down (make, no break yet); click again
        // unlatches (break). It stays held until a non-modifier key is struck or it is clicked
        // again — reconstructing a two-finger chord with one mouse.
        key.addEventListener("click", () => {
          const idx = latchedMods.findIndex((m) => m.el === key);
          if (idx >= 0) {
            // Already latched -> unlatch.
            w.cpt_key_up(code);
            key.classList.remove("held");
            latchedMods.splice(idx, 1);
          } else {
            w.cpt_key_down(code);
            key.classList.add("held");
            latchedMods.push({ code, el: key });
          }
        });
      } else if (spec.lock) {
        // Lock-on/lock-off key (e.g. CLEAR TAB SET): toggles a locked state, press pair each time.
        key.addEventListener("click", () => {
          key.classList.toggle("locked");
          w.cpt_key_down(code);
          w.cpt_key_up(code);
          releaseLatchedMods(w);
        });
      } else {
        // Momentary key: make + (release latched mods) + its own break — forming any chord.
        key.addEventListener("click", () => {
          key.classList.add("pressed");
          setTimeout(() => key.classList.remove("pressed"), 90);
          w.cpt_key_down(code);
          w.cpt_key_up(code);
          releaseLatchedMods(w);
        });
      }
      grid.appendChild(key);
    }
  }
}

// Capture the real PC keyboard for the non-special (typewriter) keys and forward make/break
// events to the machine. The PC-key -> CPT-code mapping is a placeholder ([VERIFY]): printable
// characters send their ASCII code; a few named keys map to control codes.
function setupHostKeyboard(w) {
  // Map a KeyboardEvent to a CPT key code, or null to ignore (let the browser handle it).
  function codeFor(e) {
    // Ignore modifier-only keys and browser shortcuts (Ctrl/Alt/Meta held).
    if (e.ctrlKey || e.altKey || e.metaKey) return null;
    const named = {
      Enter: 0x0d,
      Backspace: 0x08,
      Tab: 0x09,
      Escape: 0x1b,
      " ": 0x20,
    };
    if (e.key in named) return named[e.key];
    // Single printable character -> its ASCII code.
    if (e.key.length === 1) {
      const c = e.key.charCodeAt(0);
      if (c >= 0x20 && c <= 0x7e) return c;
    }
    return null;
  }

  window.addEventListener("keydown", (e) => {
    const code = codeFor(e);
    if (code === null) return;
    e.preventDefault(); // keep the browser from acting on keys we consume (Tab, Backspace, …)
    if (!e.repeat) w.cpt_key_down(code);
  });
  window.addEventListener("keyup", (e) => {
    const code = codeFor(e);
    if (code === null) return;
    e.preventDefault();
    w.cpt_key_up(code);
    // A real key completes any pending on-screen chord (e.g. latched CODE + typed letter).
    releaseLatchedMods(w);
  });
}

main().catch((e) => {
  document.getElementById("status").textContent = "error: " + e;
  console.error(e);
});
