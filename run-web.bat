@echo off
rem Build the CPT wasm module, copy it next to the web page, and serve the page.
rem Then open http://localhost:8080 in a browser. Press Ctrl+C to stop the server.
rem
rem Needs (once): rustup target add wasm32-unknown-unknown
rem For the real CPT system, insert disk\cpt.img in drive 1 on the page. Make the images with
rem setup-disks.bat (decodes the .td0 files in .kiro\specs\cpt8000-emulator). The page also
rem links a blank data disk for drive 2 (crates\cpt-wasm\web\blank-data.img).

rem Run from the repository root (where this file is), wherever it was started from.
cd /d "%~dp0"

echo Building the wasm module...
cargo build -p cpt-wasm --target wasm32-unknown-unknown --release
if errorlevel 1 (
    echo Build failed.
    exit /b 1
)

rem Honour CARGO_TARGET_DIR if it is set; otherwise cargo builds into target\.
set "TARGET=target"
if defined CARGO_TARGET_DIR set "TARGET=%CARGO_TARGET_DIR%"

copy /y "%TARGET%\wasm32-unknown-unknown\release\cpt_wasm.wasm" "crates\cpt-wasm\web\" >nul
if errorlevel 1 (
    echo Could not copy cpt_wasm.wasm from %TARGET%\wasm32-unknown-unknown\release.
    exit /b 1
)

rem The page's LOAD SAMPLE PROGRAM DISK button fetches cpt2489.img from next to the page (on
rem GitHub Pages the workflow publishes it there). Copy it in if setup-disks.bat has made it.
if exist "disk\cpt2489.img" (
    copy /y "disk\cpt2489.img" "crates\cpt-wasm\web\" >nul
) else (
    echo Note: disk\cpt2489.img not found; run setup-disks.bat for the sample disk button.
)

rem The server sends Cache-Control: no-cache, so after a rebuild a plain reload gets the new
rem cpt.js and cpt_wasm.wasm instead of the browser's old copies.
python tools\serve_web.py 8080 crates\cpt-wasm\web
