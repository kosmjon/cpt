// Live playback of the CPT speaker through the Web Audio API.
//
// The machine reports tone changes as (CPU cycle, divisor) pairs. This class turns them into
// scheduled changes on one square-wave oscillator: at cycle c the pitch becomes
// toneHz / (2 * divisor) (a divisor of 0 means silence). Times come from the EMULATED clock
// (cycle / cpuHz seconds), not from when the page happened to receive them, so the tempo is
// right however the frames are paced. If the page falls behind real time, it re-anchors a
// little ahead of "now" and carries on.
//
// Browsers only allow audio after a user gesture, so enable() must be called from one (the
// page does it on the first click or key).

const SPEAKER_LEAD = 0.06; // seconds of headroom when (re)anchoring
const SPEAKER_MIN_LEAD = 0.02; // anything scheduled closer to "now" than this counts as late
const SPEAKER_RAMP = 0.003; // seconds; time constant for gain changes, avoids clicks
const SPEAKER_LEVEL = 0.12; // output gain while a tone sounds

class SpeakerAudio {
  constructor(AudioContextClass, cpuHz, toneHz) {
    this.Ctor = AudioContextClass;
    this.cpuHz = cpuHz;
    this.toneHz = toneHz;
    this.ctx = null;
    this.osc = null;
    this.gain = null;
    this.on = false;
    this.offset = null; // audio-clock time of CPU cycle 0, once anchored
  }

  // Create (once) and start the audio graph. Must run inside a user gesture.
  enable() {
    if (!this.Ctor) return false;
    if (!this.ctx) {
      this.ctx = new this.Ctor();
      this.osc = this.ctx.createOscillator();
      this.osc.type = "square";
      this.osc.frequency.value = 440;
      this.gain = this.ctx.createGain();
      this.gain.gain.value = 0;
      this.osc.connect(this.gain);
      this.gain.connect(this.ctx.destination);
      this.osc.start();
    }
    if (this.ctx.resume) this.ctx.resume();
    this.on = true;
    this.offset = null;
    return true;
  }

  // Silence the speaker and stop scheduling.
  disable() {
    this.on = false;
    this.offset = null;
    if (this.gain) {
      this.gain.gain.cancelScheduledValues(this.ctx.currentTime);
      this.gain.gain.setValueAtTime(0, this.ctx.currentTime);
    }
  }

  // Forget the cycle-to-time anchor; the next event re-anchors. Call when the machine's
  // program changes and a gap in emulated time is expected.
  reanchor() {
    this.offset = null;
  }

  // Schedule `count` events from `buf`, a Float64Array of [cycle, divisor] pairs.
  schedule(buf, count) {
    if (!this.on || !this.ctx) return;
    const now = this.ctx.currentTime;
    for (let i = 0; i < count; i++) {
      const cycle = buf[2 * i];
      const divisor = buf[2 * i + 1];
      let t = this.offset === null ? -Infinity : this.offset + cycle / this.cpuHz;
      if (t < now + SPEAKER_MIN_LEAD) {
        // First event, or we are late: anchor so this event lands just ahead of now.
        this.offset = now + SPEAKER_LEAD - cycle / this.cpuHz;
        t = now + SPEAKER_LEAD;
      }
      if (divisor === 0) {
        this.gain.gain.setTargetAtTime(0, t, SPEAKER_RAMP);
      } else {
        this.osc.frequency.setValueAtTime(this.toneHz / (2 * divisor), t);
        this.gain.gain.setTargetAtTime(SPEAKER_LEVEL, t, SPEAKER_RAMP);
      }
    }
  }
}

// Let Node tests load this file; in the browser the class is simply a global.
if (typeof module !== "undefined" && module.exports) {
  module.exports = { SpeakerAudio, SPEAKER_LEAD, SPEAKER_MIN_LEAD };
}
