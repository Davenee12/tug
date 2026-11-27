# Checks the browser extension's path to tug on this PC, without a browser: the installer's
# registration for Chrome and Edge, the host manifest, and one call through bin\tug.exe exactly
# as the browser makes it (a 32-bit length, then JSON, over stdin/stdout).
#
#   powershell -ExecutionPolicy Bypass -File extension\test\host-check.ps1
#   powershell -ExecutionPolicy Bypass -File extension\test\host-check.ps1 -Call '{"tool":"get_latest_code","copy":false}'
#
# tug must be installed (the setup exe; the portable exe doesn't register the host) and running,
# with Settings > Developer tools > Let AI tools use tug on.

param([string]$Call = '{"tool":"status"}')
$ErrorActionPreference = 'Stop'

$name = 'dev.davejames.tug.browser'
$origin = 'chrome-extension://fdlhifdliipbdojnokcjdomknldbejbg/'
$failed = $false

function Registered($browser) {
  $key = "HKCU:\Software\$browser\NativeMessagingHosts\$name"
  $item = Get-Item -Path $key -ErrorAction SilentlyContinue
  if ($item) { return $item.GetValue('') }
  return $null
}

$manifestPath = $null
foreach ($browser in 'Google\Chrome', 'Microsoft\Edge') {
  $path = Registered $browser
  if ($path) {
    Write-Output "ok    $browser knows the host: $path"
    if (-not $manifestPath) { $manifestPath = $path }
  } else {
    Write-Output "FAIL  $browser doesn't know the host (reinstall tug with the setup exe)"
    $failed = $true
  }
}
if (-not $manifestPath -or -not (Test-Path $manifestPath)) {
  Write-Output "FAIL  no host manifest on disk"
  exit 1
}

$manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json
$exe = Join-Path (Split-Path $manifestPath) $manifest.path
if (Test-Path $exe) { Write-Output "ok    host exe: $exe" } else { Write-Output "FAIL  host exe missing: $exe"; exit 1 }
if ($manifest.allowed_origins -contains $origin) {
  Write-Output "ok    the extension's ID is allowed"
} else {
  Write-Output "FAIL  allowed_origins doesn't list $origin"
  $failed = $true
}

# One call, as the browser makes it.
$psi = New-Object System.Diagnostics.ProcessStartInfo $exe
$psi.Arguments = $origin
$psi.UseShellExecute = $false
$psi.RedirectStandardInput = $true
$psi.RedirectStandardOutput = $true
$psi.CreateNoWindow = $true
# The pipe to the host must carry exactly the frame. Windows PowerShell 5.1 opens it with the
# console's input encoding, and when that's UTF-8 (chcp 65001) it writes a byte-order mark first,
# which the host rightly reads as part of the length. PowerShell 7 can say so on the process;
# 5.1 takes it from the console, so set that for the start and put it back.
$utf8 = New-Object System.Text.UTF8Encoding $false
$consoleIn = [Console]::InputEncoding
if ($psi.PSObject.Properties.Name -contains 'StandardInputEncoding') { $psi.StandardInputEncoding = $utf8 }
try {
  [Console]::InputEncoding = $utf8
  $p = [System.Diagnostics.Process]::Start($psi)
} finally {
  [Console]::InputEncoding = $consoleIn
}

$body = [System.Text.Encoding]::UTF8.GetBytes("{`"id`":1,`"call`":$Call}")
$in = $p.StandardInput.BaseStream
$in.Write([BitConverter]::GetBytes([int]$body.Length), 0, 4)
$in.Write($body, 0, $body.Length)
$in.Flush()
$in.Close()

function ReadExactly($stream, [int]$count) {
  $buf = New-Object byte[] $count
  $got = 0
  while ($got -lt $count) {
    $n = $stream.Read($buf, $got, $count - $got)
    if ($n -le 0) { throw "the host closed after $got of $count bytes" }
    $got += $n
  }
  return $buf
}

$out = $p.StandardOutput.BaseStream
$len = [BitConverter]::ToInt32((ReadExactly $out 4), 0)
$reply = [System.Text.Encoding]::UTF8.GetString((ReadExactly $out $len))
if (-not $p.WaitForExit(15000)) { $p.Kill() }

$r = $reply | ConvertFrom-Json
# tug's own answers about the call (no code yet, a switch off, the phone away) mean the path from
# the browser to tug works; anything from the host itself, or the master switch, means it doesn't.
$appAnswers = 'not_found', 'tool_off', 'unavailable', 'rate_limited', 'busy', 'invalid'
if ($r.error) {
  if ($appAnswers -contains $r.error.code) {
    Write-Output "ok    the path to tug works; tug answered $($r.error.code): $($r.error.message)"
    if ($failed) { exit 1 }
    exit 0
  }
  Write-Output "FAIL  tug answered with an error: $($r.error.code): $($r.error.message)"
  exit 1
}
Write-Output "ok    tug answered $Call"
Write-Output ($r.ok | ConvertTo-Json -Depth 6)
if ($failed) { exit 1 }
exit 0
