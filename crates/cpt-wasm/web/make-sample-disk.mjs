// Generate a sample CPT disk image matching the default geometry (2002 blocks x 128 bytes).
// Each block begins with an ASCII marker so the contents are recognizable if read back.
// Usage:  node make-sample-disk.mjs   ->  writes sample.disk next to this script.
import { writeFile } from "node:fs/promises";

const BLOCK_SIZE = 128;
const BLOCK_COUNT = 2002;
const data = new Uint8Array(BLOCK_SIZE * BLOCK_COUNT);

const enc = new TextEncoder();
for (let b = 0; b < BLOCK_COUNT; b++) {
  const base = b * BLOCK_SIZE;
  // Human-readable marker at the start of each block.
  const label = `CPT SAMPLE DISK  block ${b.toString().padStart(4, "0")}  `;
  const bytes = enc.encode(label);
  data.set(bytes.subarray(0, BLOCK_SIZE), base);
  // Fill the rest of the block with a ramp so it isn't all zeros.
  for (let i = bytes.length; i < BLOCK_SIZE; i++) {
    data[base + i] = (b + i) & 0xff;
  }
}

// A friendly volume header in block 0.
const header = enc.encode("CPT DATA DISK - sample volume - not real CPT data - for mount/eject testing");
data.set(header.subarray(0, BLOCK_SIZE), 0);

await writeFile(new URL("./sample.disk", import.meta.url), data);
console.log(`wrote sample.disk (${data.length} bytes, ${BLOCK_COUNT} x ${BLOCK_SIZE})`);
