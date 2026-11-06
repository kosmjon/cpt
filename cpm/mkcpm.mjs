// Build a bootable CP/M system disk image.
//
// Usage:  node cpm/mkcpm.mjs <out.img> <loader.bin> <ccp.bin> <bdos.bin> <bios.bin>
//
// Layout (linear 128-byte blocks; block = track*26 + sector-1, 2002 blocks):
//   block 0        boot sector loader (cpm/loader.mol)
//   blocks 1..44   CCP then BDOS, padded to 0x1600 bytes (loaded at 0E400H)
//   blocks 45..    BIOS (loaded at 0FA00H, immediately after the BDOS area)
//   blocks 0..51   the two system tracks; the rest is the empty CP/M directory/data (0E5H).
import { readFileSync, writeFileSync } from "node:fs";

const BLOCK_SIZE = 128;
const BLOCK_COUNT = 2002; // must match cpt_machine::disk::Geometry::default()
const SYSTEM_BLOCKS = 2 * 26;
const CCP_BDOS_BYTES = 0x1600; // CCP 0x800 + BDOS 0xE00

const [, , outPath, loaderPath, ccpPath, bdosPath, biosPath] = process.argv;
if (!biosPath) {
  console.error("usage: node cpm/mkcpm.mjs <out.img> <loader.bin> <ccp.bin> <bdos.bin> <bios.bin>");
  process.exit(2);
}
const loader = readFileSync(loaderPath);
const ccp = readFileSync(ccpPath);
const bdos = readFileSync(bdosPath);
const bios = readFileSync(biosPath);

if (loader.length > BLOCK_SIZE) {
  console.error(`loader is ${loader.length} bytes; it must fit in ${BLOCK_SIZE}`);
  process.exit(1);
}
if (ccp.length > 0x800 || ccp.length + 0 > CCP_BDOS_BYTES) {
  console.error(`ccp is ${ccp.length} bytes; expected at most 0x800`);
  process.exit(1);
}
if (0x800 + bdos.length > CCP_BDOS_BYTES) {
  console.error(`bdos is ${bdos.length} bytes; expected at most 0xE00`);
  process.exit(1);
}
const sysBytes = CCP_BDOS_BYTES + bios.length;
const sysBlocks = Math.ceil(sysBytes / BLOCK_SIZE);
if (1 + sysBlocks > SYSTEM_BLOCKS) {
  console.error(`system needs ${1 + sysBlocks} blocks; the system tracks hold ${SYSTEM_BLOCKS}`);
  process.exit(1);
}

const image = Buffer.alloc(BLOCK_SIZE * BLOCK_COUNT, 0xe5);
image.fill(0, 0, SYSTEM_BLOCKS * BLOCK_SIZE); // system tracks start zeroed
loader.copy(image, 0);
ccp.copy(image, BLOCK_SIZE); // CCP at 0E400H
bdos.copy(image, BLOCK_SIZE + 0x800); // BDOS at 0EC00H
bios.copy(image, BLOCK_SIZE + CCP_BDOS_BYTES); // BIOS at 0FA00H
writeFileSync(outPath, image);
console.log(
  `wrote ${outPath}: loader ${loader.length}B, ccp ${ccp.length}B, bdos ${bdos.length}B, ` +
    `bios ${bios.length}B; system uses blocks 0..${sysBlocks} of ${SYSTEM_BLOCKS}`,
);
