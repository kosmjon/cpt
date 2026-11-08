@echo off
rem Build the CPT wasm module, copy it next to the web page, and serve the page.
rem Then open http://localhost:8080 in a browser. Press Ctrl+C to stop the server.
rem
rem Needs (once): rustup target add wasm32-unknown-unknown
rem For the real CPT system, insert disk\cpt.img in drive 1 on the page. Make it with:
rem   python tools\td0.py .kiro\specs\cpt8000-emulator\cpt.td0 --raw disk\cpt.img

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

echo Serving crates\cpt-wasm\web at http://localhost:8080  (Ctrl+C to stop)
python -m http.server --directory crates\cpt-wasm\web 8080
