@echo off
REM Build the CPT wasm display, copy it into the web folder, and serve it locally.
REM Run from the repo root:  run-web.bat   then open http://localhost:8080
REM
REM Requires: rustup target add wasm32-unknown-unknown  (one-time), and Node.js (for the server).

setlocal
cd /d "%~dp0"

echo [1/3] Building cpt-wasm for wasm32-unknown-unknown (release)...
cargo build -p cpt-wasm --target wasm32-unknown-unknown --release
if errorlevel 1 (
    echo.
    echo Build failed. If the error mentions the wasm target, run:
    echo     rustup target add wasm32-unknown-unknown
    exit /b 1
)

echo [2/3] Copying the wasm module into crates\cpt-wasm\web ...
copy /Y "target\wasm32-unknown-unknown\release\cpt_wasm.wasm" "crates\cpt-wasm\web\cpt_wasm.wasm" >nul
if errorlevel 1 (
    echo Copy failed.
    exit /b 1
)

echo [3/3] Serving on http://localhost:8080  (press Ctrl+C to stop)
echo.
npx --yes serve -l 8080 crates\cpt-wasm\web

endlocal
