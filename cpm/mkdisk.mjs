// Build a disk image with a boot sector.
//
// Usage:  node cpm/mkdisk.mjs <out.img> <bootsector.bin>
//
// Creates a blank image matching the emulator's default disk geometry (2002 blocks of 128
// bytes = 77 tracks x 26 sectors, the 8-inch IBM 3740 layout) and writes the boot sector
// binary into block 0. Later increments extend this to lay down the CP/M system tracks.
import { readFileSync, writeFileSync } from "node:fs";

// Must match cpt_machine::disk::Geometry::default().
const BLOCK_SIZE = 128;
const BLOCK_COUNT = 2002;

const [, , outPath, bootPath] = process.argv;
if (!outPath || !bootPath) {
  console.error("usage: node cpm/mkdisk.mjs <out.img> <bootsector.bin>");
  process.exit(2);
}

const boot = readFileSync(bootPath);
if (boot.length > BLOCK_SIZE) {
  console.error(`boot sector is ${boot.length} bytes; it must fit in one ${BLOCK_SIZE}-byte block`);
  process.exit(1);
}

const image = Buffer.alloc(BLOCK_SIZE * BLOCK_COUNT); // zero-filled
boot.copy(image, 0); // block 0
writeFileSync(outPath, image);
console.log(`wrote ${outPath}: ${image.length} bytes, boot sector ${boot.length} bytes in block 0`);
