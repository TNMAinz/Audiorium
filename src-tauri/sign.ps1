# Signs one file for a Windows bundle, or deliberately does nothing.
#
# The certificate never lives in this repository: a publicly trusted code-signing
# key is an identity, so it stays in the Windows certificate store and this script
# is told which one by its thumbprint. With no thumbprint it leaves the file
# unsigned and the build succeeds, which is what a machine without a certificate
# and every contributor needs.
#
#   $env:AUDIORIUM_CERT_THUMBPRINT = 'AB12...'   # sign with that certificate
#   $env:AUDIORIUM_TIMESTAMP_URL   = 'none'      # skip timestamping (tests only)
#
# Timestamping is on by default: without it the signature stops verifying the day
# the certificate expires.
param([Parameter(Mandatory = $true)][string]$File)

$thumbprint = $env:AUDIORIUM_CERT_THUMBPRINT
if ([string]::IsNullOrWhiteSpace($thumbprint)) {
  Write-Output "AUDIORIUM: no certificate configured (AUDIORIUM_CERT_THUMBPRINT); $File stays unsigned"
  exit 0
}

$signtool = $null
$onPath = Get-Command signtool.exe -ErrorAction SilentlyContinue
if ($onPath) { $signtool = $onPath.Source }
if (-not $signtool) {
  $kits = 'C:\Program Files (x86)\Windows Kits\10\bin'
  $signtool = Get-ChildItem $kits -Directory -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } |
    Where-Object { Test-Path $_ } |
    Select-Object -First 1
}
if (-not $signtool) {
  Write-Error 'AUDIORIUM: signtool.exe was not found; install the Windows SDK'
  exit 1
}

$timestamp = $env:AUDIORIUM_TIMESTAMP_URL
if ([string]::IsNullOrWhiteSpace($timestamp)) { $timestamp = 'http://timestamp.digicert.com' }
$signArgs = @('sign', '/sha1', $thumbprint.Trim(), '/fd', 'SHA256')
if ($timestamp -ne 'none') { $signArgs += @('/tr', $timestamp, '/td', 'SHA256') }
$signArgs += $File

& $signtool @signArgs
if ($LASTEXITCODE -ne 0) {
  Write-Error "AUDIORIUM: signtool failed for $File (exit $LASTEXITCODE)"
  exit $LASTEXITCODE
}
Write-Output "AUDIORIUM: signed $File"
