# Boots a CP/M image under cpt-run and types commands at it over serial (for testing).
# Commands are separated by |.  Needs a built cpt-run (cargo build -p cpt-run) and
# cpm/build/boot.bin (cpm/build-cpm.ps1).  Disk writes persist, so use copies of the images.
#   powershell -File cpm/tools/serial-drive.ps1 -Send "DIR|STAT" -Image a.img -Image2 b.img
param(
  [string]$Send = "DIR",
  [int]$Port = 2327,
  [string]$Image = "cpm/build/cpm.img",
  [string]$Image2 = ""
)
$argv = @('--serial-listen', "127.0.0.1:$Port", '--disk1', $Image)
if ($Image2) { $argv += @('--disk2', $Image2) }
$argv += 'cpm/build/boot.bin'
$p = Start-Process -FilePath target\debug\cpt-run.exe -ArgumentList $argv -PassThru -WindowStyle Hidden
Start-Sleep 2
$c = New-Object Net.Sockets.TcpClient
$c.Connect('127.0.0.1', $Port)
$s = $c.GetStream()
$s.ReadTimeout = 2500
function Read-All {
  $sb = New-Object Text.StringBuilder
  $buf = New-Object byte[] 512
  try { while ($true) { $n = $s.Read($buf, 0, 512); if ($n -le 0) { break }; [void]$sb.Append([Text.Encoding]::ASCII.GetString($buf, 0, $n)) } } catch {}
  return $sb.ToString()
}
"--- boot:"
Read-All
foreach ($line in ($Send -split '\|')) {
  $b = [Text.Encoding]::ASCII.GetBytes($line + "`r")
  $s.Write($b, 0, $b.Length)
  "--- sent '$line':"
  Read-All
}
$c.Close()
Stop-Process -Id $p.Id -Force
