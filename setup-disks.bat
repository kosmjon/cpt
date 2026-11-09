@echo off
rem Make the disk images the emulator and its tests use (needs Python only).
rem
rem   disk\cpt.img, disk\cpt2365.img, disk\cpt2489.img   decoded from the .td0 files in
rem                                                      .kiro\specs\cpt8000-emulator
rem   disk\blank-data.img                                a blank, formatted data disk
rem
rem Run this once after cloning, and before "cargo test" if you want the real-disk boot tests to run
rem (without the images they print a note and skip). Run from anywhere: it changes to its own folder.
cd /d "%~dp0"
python tools\setup_disks.py %*
