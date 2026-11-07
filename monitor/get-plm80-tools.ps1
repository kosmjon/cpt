# Fetch the PL/M-80 toolchain used by build-plm.ps1 into monitor/plm80tools/ (gitignored).
#
# These are the prebuilt 32-bit Windows C ports of the Intel ISIS-II tools by Mark Ogden
# (https://github.com/ogdenpm/intel80tools, tools/ directory; C source in ogdenpm/c-ports).
# They are third-party binaries released for hobbyist and academic use, so they are downloaded
# on demand rather than kept in this repository.
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$tools = Join-Path $here "plm80tools"
New-Item -ItemType Directory -Force $tools | Out-Null
foreach ($f in "plm80.exe", "link.exe", "locate.exe", "aomf2bin.exe", "asm80.exe") {
    $url = "https://raw.githubusercontent.com/ogdenpm/intel80tools/master/tools/$f"
    Write-Host "fetching $f"
    Invoke-WebRequest $url -OutFile (Join-Path $tools $f)
}
Write-Host "tools in $tools"
