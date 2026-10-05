# make-cert.ps1 - Spike 1 (tug v0.5.7 calling)
#
# DAVE RUNS THIS HIMSELF. It changes your user certificate store (no admin needed):
#   1. creates a self-signed code-signing certificate "CN=Jordan Lee Dev" (with private key)
#      in Cert:\CurrentUser\My  (Personal), valid for 1 year
#   2. exports its public part to .\TugCallSpike.cer
#   3. imports that .cer into Cert:\CurrentUser\TrustedPeople so Windows trusts packages signed
#      with it for your account
#
# Re-running reuses an existing "CN=Jordan Lee Dev" cert instead of creating another.
# Undo: see "Cleanup" in README.md (removes the cert from both stores).
#
# Run with Windows PowerShell:  powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\make-cert.ps1

$ErrorActionPreference = 'Stop'
$subject = 'CN=Jordan Lee Dev'
$cerPath = Join-Path $PSScriptRoot 'TugCallSpike.cer'

$cert = Get-ChildItem Cert:\CurrentUser\My |
    Where-Object { $_.Subject -eq $subject -and $_.HasPrivateKey -and $_.NotAfter -gt (Get-Date) } |
    Sort-Object NotAfter -Descending |
    Select-Object -First 1

if ($cert) {
    Write-Host "Reusing existing certificate $subject ($($cert.Thumbprint))"
} else {
    # EKU 1.3.6.1.5.5.7.3.3 = code signing; empty basic constraints = end-entity cert.
    $cert = New-SelfSignedCertificate `
        -Type Custom `
        -Subject $subject `
        -FriendlyName 'tug call spike (dev signing, safe to delete)' `
        -KeyUsage DigitalSignature `
        -KeyAlgorithm RSA -KeyLength 2048 `
        -CertStoreLocation 'Cert:\CurrentUser\My' `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}') `
        -NotAfter (Get-Date).AddYears(1)
    Write-Host "Created certificate $subject ($($cert.Thumbprint)) in Cert:\CurrentUser\My"
}

Export-Certificate -Cert $cert -FilePath $cerPath | Out-Null
Write-Host "Exported public certificate to $cerPath"

Import-Certificate -FilePath $cerPath -CertStoreLocation 'Cert:\CurrentUser\TrustedPeople' | Out-Null
Write-Host "Imported it into Cert:\CurrentUser\TrustedPeople"

Write-Host ""
Write-Host "Thumbprint: $($cert.Thumbprint)"
Write-Host "Next: powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\pack-and-register.ps1"
