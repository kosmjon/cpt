// The real CPT system in the browser: boots a real program disk image in the wasm module,
// paces it, decodes its screen, and maps the PC keyboard to CPT key numbers.
//
// Pacing: the real software checks disk timing (reads must take about 31 ms or more, as on a
// real 8-inch drive), so start-up can't be shortened by faster reads. Instead the emulation
// runs faster than real time until the system has settled (about 37 s of emulated time),
// then in real time. Loaded as a classic script in the page and with require() in Node.

const REAL_CPU_HZ = 2_000_000;

// Rows on the real CPT screen: the title line, 45 rows through the typing line, the ruler and
// 7 rows below (the system's own layout: 04ABH = 45, 04ACH = 7).
const REAL_SCREEN_ROWS = 54;

// CPT key numbers found by typing into the real system (see program-disk.md, "The keyboard").
const CPT_KEY = {
  SHIFT: 0x70,
  LOCK: 0x72,
  CODE: 0x77,
  SPACE: 0x10,
  RETURN: 0x14,
  BACKSPACE: 0x06,
  // UP and DOWN move by the selected unit (CHAR, WORD, LINE, PARA, PAGE); with no unit
  // selected, by a line. DOWN moves the text down: the upper screen scrolls into the lower
  // screen (toward the document's start). UP moves the text up (on through the document).
  UP: 0x0f,
  DOWN: 0x1f,
  // SKIP discards text after the cursor by the selected unit: PAGE then SKIP sends the whole
  // lower screen to the bit bucket. STOP and ADJ are its neighbours in the same key column
  // (the column's keys share the low digit F: 3F 2F 7F 0F 1F). STOP is the key the keyboard
  // interrupt handler treats specially; ADJ (7F) rejoins lines.
  STOP: 0x3f,
  SKIP: 0x2f,
  ADJ: 0x7f,
  CHAR: 0x3e,
  WORD: 0x2e,
  LINE: 0x7e,
  PARA: 0x0e,
  PAGE: 0x5f,
  IN: 0x3d,
  OUT: 0x2d,
  PRINT: 0x7d,
  HOLD: 0x73,
  MAR_SET: 0x71,
  TAB_SET: 0x75,
};

// Special-key panel labels (as in cpt.js) -> CPT key number, for the keys identified so far.
// Keys missing here are not yet identified on the real system.
const SPECIAL_LABEL_KEYS = {
  CODE: CPT_KEY.CODE,
  HOLD: CPT_KEY.HOLD,
  "MAR SET": CPT_KEY.MAR_SET,
  CHAR: CPT_KEY.CHAR,
  WORD: CPT_KEY.WORD,
  LINE: CPT_KEY.LINE,
  PARA: CPT_KEY.PARA,
  PAGE: CPT_KEY.PAGE,
  IN: CPT_KEY.IN,
  OUT: CPT_KEY.OUT,
  PRINT: CPT_KEY.PRINT,
  DOWN: CPT_KEY.DOWN,
  UP: CPT_KEY.UP,
  SKIP: CPT_KEY.SKIP,
  STOP: CPT_KEY.STOP,
  ADJ: CPT_KEY.ADJ,
};

// PC keys that are not plain characters (KeyboardEvent.code -> CPT key number).
const PC_NAMED_KEYS = {
  Space: CPT_KEY.SPACE,
  Enter: CPT_KEY.RETURN,
  NumpadEnter: CPT_KEY.RETURN,
  Backspace: CPT_KEY.BACKSPACE,
  ShiftLeft: CPT_KEY.SHIFT,
  ShiftRight: CPT_KEY.SHIFT,
  CapsLock: CPT_KEY.LOCK,
  AltLeft: CPT_KEY.CODE,
  AltRight: CPT_KEY.CODE,
  ArrowDown: CPT_KEY.DOWN,
  ArrowUp: CPT_KEY.UP,
  Backslash: 0x15, // the CPT key with | and \
  // The CPT keypad (keys found by the key sweep).
  Numpad0: 0x18, Numpad1: 0x49, Numpad2: 0x19, Numpad3: 0x59, Numpad4: 0x4a,
  Numpad5: 0x1a, Numpad6: 0x5a, Numpad7: 0x4b, Numpad8: 0x1b, Numpad9: 0x5b,
  NumpadDecimal: 0x6a, NumpadSubtract: 0x6c,
};

// PC keys that type a character: KeyboardEvent.code -> the character the key types unshifted
// on a US layout. The CPT key is the one that types the same character unshifted.
function pcKeyChar(code) {
  if (/^Key[A-Z]$/.test(code)) return code[3].toLowerCase();
  if (/^Digit[0-9]$/.test(code)) return code[5];
  return { Minus: "-", Equal: "=", Semicolon: ";", Quote: "'", Comma: ",", Period: ".", Slash: "/" }[code];
}

// Stand-in glyphs for the character ROM (its real shapes are unknown); matches cpt_display.rs.
function cptGlyph(code) {
  if (code >= 0x20 && code <= 0x7e) return String.fromCharCode(code);
  switch (code) {
    case 0xa0: return " ";
    case 0x80: return ".";
    case 0x81: return ":";
    case 0x82: return "|";
    case 0x83: return "M";
    case 0x84: return ">";
    case 0x87: return "^";
    case 0x89: return ">";
    case 0x8b: return "®";
    case 0x8c: return "©";
    default: return "?";
  }
}

class RealCpt {
  constructor(w, now = () => performance.now()) {
    this.w = w;
    this.now = now;
    this.lastWall = null;
    // Faster than real time until the system has settled: at most this much wall time per
    // animation frame goes to emulation.
    this.turboBudgetMs = 12;
    this.turboChunk = 400_000;
  }

  get running() {
    return this.w.cpt_real_running() === 1;
  }

  // Put a disk image (Uint8Array) in drive `d` (0 = drive 1, 1 = drive 2). Returns false if
  // it is not a CPT disk image. Works with or without a running system.
  insert(d, bytes) {
    const ptr = this.w.cpt_xfer_reserve(bytes.length);
    new Uint8Array(this.w.memory.buffer, ptr, bytes.length).set(bytes);
    return this.w.cpt_real_insert(d) === 1;
  }

  // Take the disk out of drive `d`; returns its bytes (a copy) or null. A running system
  // carries on without it.
  eject(d) {
    const len = this.w.cpt_real_eject(d);
    return len ? new Uint8Array(this.w.memory.buffer, this.w.cpt_xfer_ptr(), len).slice() : null;
  }

  // Boot a fresh system from the disk in drive 1 (power on, reset). Returns false if drive 1
  // is empty.
  start() {
    this.lastWall = null;
    return this.w.cpt_real_start() === 1;
  }

  // Stop the system; the disks stay in their drives (power off).
  halt() {
    this.w.cpt_real_halt();
  }

  // Convenience for checks: put a program disk in drive 1 and boot it.
  boot(bytes) {
    return this.insert(0, bytes) && this.start();
  }

  get cycles() {
    return this.w.cpt_real_cycles();
  }

  get settled() {
    return this.cycles >= this.w.cpt_real_settled_cycles();
  }

  // Fraction of start-up done (0..1), for a progress note.
  get startProgress() {
    return Math.min(1, this.cycles / this.w.cpt_real_settled_cycles());
  }

  // Run the machine for one animation frame.
  frame() {
    const wall = this.now();
    if (!this.settled) {
      // Turbo: run in chunks until the frame's budget is used.
      const until = wall + this.turboBudgetMs;
      do {
        this.w.cpt_real_run(this.turboChunk);
      } while (!this.settled && this.now() < until);
      this.lastWall = this.now();
      return;
    }
    // Real time: the cycles that match the wall time since the last frame (at most 0.1 s, so
    // a background tab doesn't come back to a long catch-up).
    const dt = this.lastWall === null ? 1000 / 60 : Math.min(100, wall - this.lastWall);
    this.lastWall = wall;
    this.w.cpt_real_run(Math.round((dt / 1000) * REAL_CPU_HZ));
  }

  // Run a number of cycles directly (tests and scripted typing).
  runCycles(n) {
    this.w.cpt_real_run(n);
  }

  // The decoded screen: { rows, cols, codes, attrs } (attrs bit 0 = a cell, bit 1 = white on
  // black). The arrays are views into wasm memory, valid until the next call into wasm.
  screen() {
    const rows = this.w.cpt_real_screen();
    const cols = this.w.cpt_real_cols();
    const n = this.w.cpt_real_max_rows() * cols;
    return {
      rows,
      cols,
      codes: new Uint8Array(this.w.memory.buffer, this.w.cpt_real_codes_ptr(), n),
      attrs: new Uint8Array(this.w.memory.buffer, this.w.cpt_real_attrs_ptr(), n),
    };
  }

  // Screen rows as text (for checks and logs).
  screenText() {
    const s = this.screen();
    const out = [];
    for (let r = 0; r < s.rows; r++) {
      let line = "";
      for (let c = 0; c < s.cols; c++) {
        if (s.attrs[r * s.cols + c] & 1) line += cptGlyph(s.codes[r * s.cols + c]);
      }
      out.push(line);
    }
    return out;
  }

  press(key) {
    this.w.cpt_real_key(key, 1);
  }

  release(key) {
    this.w.cpt_real_key(key, 0);
  }

  // The CPT key for a PC KeyboardEvent.code, or null if the CPT has no such key.
  keyForPcCode(code) {
    if (code in PC_NAMED_KEYS) return PC_NAMED_KEYS[code];
    const ch = pcKeyChar(code);
    if (!ch) return null;
    const k = this.w.cpt_real_key_for_char(ch.charCodeAt(0));
    // Only keys that type the character unshifted: the user's own Shift does the rest.
    return k >= 0 && !(k & 0x100) ? k & 0x7f : null;
  }

  // The key (and whether SHIFT is needed) that types a character, or null.
  keyForChar(ch) {
    const k = this.w.cpt_real_key_for_char(ch.charCodeAt(0));
    return k < 0 ? null : { key: k & 0x7f, shift: (k & 0x100) !== 0 };
  }

  // Drive state for the UI: { loaded, modified, active } for drive 0 (station 1) or 1.
  drive(d) {
    const s = this.w.cpt_real_drive_state(d);
    return { loaded: (s & 1) !== 0, modified: (s & 2) !== 0, active: (s & 4) !== 0 };
  }

}

if (typeof module !== "undefined" && module.exports) {
  module.exports = { RealCpt, CPT_KEY, SPECIAL_LABEL_KEYS, PC_NAMED_KEYS, REAL_SCREEN_ROWS, cptGlyph, pcKeyChar };
}
