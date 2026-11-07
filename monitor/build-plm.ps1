# Build monitor.plm (PL/M-80) into monitor/plm-build/monitor.bin, ready for cpt-run.
#
# Needs Mark Ogden's C ports of the Intel 8080 tools (plm80, link, locate, aomf2bin).  They are
# fetched once into monitor/plm80tools/ (gitignored) by get-plm80-tools.ps1, which this script
# runs if they are missing.
#
# Usage, from the repo root:   powershell -File monitor/build-plm.ps1
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$tools = Join-Path $here "plm80tools"
$out = Join-Path $here "plm-build"

if (-not (Test-Path (Join-Path $tools "plm80.exe"))) { & (Join-Path $here "get-plm80-tools.ps1") }
New-Item -ItemType Directory -Force $out | Out-Null
Push-Location $here
try {
    function Run($exe, [string[]]$arguments) {
        $o = & (Join-Path $tools "$exe.exe") @arguments 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) { Write-Host $o; throw "$exe failed (exit $LASTEXITCODE); see plm-build\monitor.lst" }
        return $o
    }
    # Compile.  CODE puts the generated 8080 code in the listing (handy for checking the
    # interrupt frame layout SPAWN depends on).
    Run "plm80" @("monitor.plm", "PRINT(plm-build/monitor.lst)", "OBJECT(plm-build/monitor.obj)", "CODE") | Out-Null
    # Link (no runtime library is needed: the program uses no multiply/divide).
    Run "link" @("plm-build/monitor.obj", "TO", "plm-build/monitor.lnk", "MAP", "PRINT(plm-build/monitor.lnk.lst)") | Out-Null
    # Locate: code from 2000H, above the fixed data areas (TCBs at 0800H, variables at 0C00H, the stack pool at 1000H), with a jump to the main program at address 0 (RESTART0); and a stack for the
    # start-up code (tasks use their own pooled stacks).
    Run "locate" @("plm-build/monitor.lnk", "TO", "plm-build/monitor.abs", "CODE(2000H)", "STACKSIZE(256)",
                   "RESTART0", "MAP", "PUBLICS", "PRINT(plm-build/monitor.map)") | Out-Null
    # Absolute object module -> flat binary, base address 0.
    Run "aomf2bin" @("-b", "0", "-z", "plm-build/monitor.abs", "plm-build/monitor.bin") | Out-Null
    $size = (Get-Item "plm-build/monitor.bin").Length
    Write-Host "wrote monitor/plm-build/monitor.bin ($size bytes); map in plm-build/monitor.map"
} finally { Pop-Location }
