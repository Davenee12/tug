# pack-and-register.ps1 - Spike 1 (tug v0.5.7 calling)
#
# DAVE RUNS THIS HIMSELF, after `cargo build --release` and make-cert.ps1. It:
#   1. packs package\ (AppxManifest.xml + logos only) into out\TugCallSpike.msix
#      (makeappx /nv: the exe is deliberately NOT in the package, so skip semantic validation)
#   2. signs the .msix with the "CN=Jordan Lee Dev" cert from Cert:\CurrentUser\My
#   3. removes any earlier registration of JordanLee.TugCallSpike, then registers the sparse
#      package for your user with Add-AppxPackage -ExternalLocation target\release, which gives
#      target\release\tug-call-spike.exe package identity when it runs
#
# Undo: Get-AppxPackage JordanLee.TugCallSpike | Remove-AppxPackage   (see README.md, Cleanup)
#
# Run with Windows PowerShell:  powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\pack-and-register.ps1
# Optional: -Thumbprint <thumbprint> to pick a specific cert; -SdkBin <dir> if the SDK lives elsewhere.

param(
    [string]$Thumbprint,
    [string]$SdkBin = 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64'
)

$ErrorActionPreference = 'Stop'
$packageName = 'JordanLee.TugCallSpike'
$subject = 'CN=Jordan Lee Dev'

$makeappx = Join-Path $SdkBin 'makeappx.exe'
$signtool = Join-Path $SdkBin 'signtool.exe'
foreach ($tool in @($makeappx, $signtool)) {
    if (-not (Test-Path $tool)) { throw "Not found: $tool (pass -SdkBin <Windows SDK bin\x64 dir>)" }
}

$releaseDir = Join-Path $PSScriptRoot 'target\release'
$exe = Join-Path $releaseDir 'tug-call-spike.exe'
if (-not (Test-Path $exe)) { throw "Not found: $exe. Run 'cargo build --release' in spikes\phoneline first." }
$releaseDir = (Resolve-Path $releaseDir).Path

if (-not $Thumbprint) {
    $cert = Get-ChildItem Cert:\CurrentUser\My |
        Where-Object { $_.Subject -eq $subject -and $_.HasPrivateKey -and $_.NotAfter -gt (Get-Date) } |
        Sort-Object NotAfter -Descending |
        Select-Object -First 1
    if (-not $cert) { throw "No '$subject' code-signing cert in Cert:\CurrentUser\My. Run make-cert.ps1 first." }
    $Thumbprint = $cert.Thumbprint
}
Write-Host "Signing cert: $subject ($Thumbprint)"

$outDir = Join-Path $PSScriptRoot 'out'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$msix = Join-Path $outDir 'TugCallSpike.msix'

Write-Host ""
Write-Host "== makeappx pack =="
& $makeappx pack /o /nv /d (Join-Path $PSScriptRoot 'package') /p $msix
if ($LASTEXITCODE -ne 0) { throw "makeappx failed ($LASTEXITCODE)" }

Write-Host ""
Write-Host "== signtool sign =="
& $signtool sign /fd SHA256 /s My /sha1 $Thumbprint $msix
if ($LASTEXITCODE -ne 0) { throw "signtool failed ($LASTEXITCODE)" }

Write-Host ""
Write-Host "== register sparse package =="
$old = Get-AppxPackage -Name $packageName
if ($old) {
    Write-Host "Removing earlier registration $($old.PackageFullName)"
    $old | Remove-AppxPackage
}
Add-AppxPackage -Path $msix -ExternalLocation $releaseDir

Get-AppxPackage -Name $packageName | Format-List Name, PackageFullName, Publisher, Status, IsDevelopmentMode
Write-Host "Registered. External location: $releaseDir"
Write-Host "Next: .\target\release\tug-call-spike.exe"
