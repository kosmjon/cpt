# Build the bootable CP/M disk image: cpm/build/cpm.img
#   CCP + BDOS: stock Digital Research source (cpm/vendor), via cpm/tools/asm80.py
#   BIOS + loader: MOL80 (cpm/bios.mol, cpm/loader.mol), via mol80asm
# Run from the repo root:  powershell -File cpm/build-cpm.ps1
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force cpm/build | Out-Null

python cpm/tools/asm80.py -Dorigin=0xE400 -o cpm/build/ccp.bin cpm/vendor/ccp.asm
python cpm/tools/asm80.py -Dorigin=0xEC00 -o cpm/build/bdos.bin -s cpm/build/bdos.sym.json cpm/vendor/bdos.asm
cargo run -q -p mol80asm -- --bin cpm/build/boot.bin cpm/boot.mol
cargo run -q -p mol80asm -- --bin cpm/build/bios.bin cpm/bios.mol
cargo run -q -p mol80asm -- --bin cpm/build/loader.bin cpm/loader.mol
node cpm/mkcpm.mjs cpm/build/cpm.img cpm/build/loader.bin cpm/build/ccp.bin cpm/build/bdos.bin cpm/build/bios.bin

# Utilities onto drive 1 (A:), and a blank formatted data disk for drive 2 (B:)
python cpm/tools/cpmfs.py cpm/build/cpm.img add (Get-ChildItem cpm/dist/*.COM).FullName
python cpm/tools/cpmfs.py cpm/build/data.img mkblank
