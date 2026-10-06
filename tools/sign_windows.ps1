# Sign the Windows build outputs with a self-signed code-signing certificate (no CA, no cost).
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\sign_windows.ps1
#       create / refresh the certificate, export it to build\cert\PhotoBatchEditor.cer
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\sign_windows.ps1 ^
#       -Files "dist\PhotoBatchEditor\PhotoBatchEditor.exe" -CopyCertificateTo "dist\PhotoBatchEditor"
#       ... and sign that file (SHA256 + timestamp), copy the .cer next to the output
#
# The certificate is created once in Cert:\CurrentUser\My (CN=Photo Batch Editor, valid 5 years,
# private key never written to disk) and trusted for the current user on this build PC, so the
# script can check that the signature is really valid. On another PC install PhotoBatchEditor.cer
# ONCE (double-click > Install Certificate > Place all certificates in the following store >
# Trusted Root Certification Authorities) and the signed Setup / .exe start without Windows
# warnings. Losing the certificate (reinstalling Windows) means the new one must be trusted again.
# Exit 0 = ready (build continues), exit 1 = error (build_windows.bat stops).

[CmdletBinding()]
param(
    [string[]]$Files = @(),
    [string[]]$CopyCertificateTo = @(),
    [string]$TimestampUrl = 'http://timestamp.digicert.com'
)

$ErrorActionPreference = 'Stop'

$Subject = 'CN=Photo Batch Editor'
$FriendlyName = 'Photo Batch Editor'
$CodeSigningEku = '1.3.6.1.5.5.7.3.3'
$Store = 'Cert:\CurrentUser\My'
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$CerPath = Join-Path $ProjectRoot 'build\cert\PhotoBatchEditor.cer'

function Find-Certificate {
    $best = $null
    foreach ($cert in Get-ChildItem -Path $Store) {
        if ($cert.Subject -ne $Subject) { continue }
        if (-not $cert.HasPrivateKey) { continue }
        if ($cert.NotAfter -lt (Get-Date).AddDays(30)) { continue }
        $eku = $cert.EnhancedKeyUsageList | Where-Object { $_.ObjectId -eq $CodeSigningEku }
        if (-not $eku) { continue }
        if ($null -eq $best -or $cert.NotAfter -gt $best.NotAfter) { $best = $cert }
    }
    return $best
}

function Resolve-Path2([string]$Path) {
    if (-not [System.IO.Path]::IsPathRooted($Path)) {
        $Path = Join-Path (Get-Location).Path $Path
    }
    return [System.IO.Path]::GetFullPath($Path)
}

function Add-FileSignature([string]$Path, $Cert) {
    if (-not (Test-Path -LiteralPath $Path)) { throw "File not found: $Path" }
    $current = Get-AuthenticodeSignature -FilePath $Path
    if ($current.Status -eq 'Valid' -and $current.SignerCertificate -and
        $current.SignerCertificate.Thumbprint -eq $Cert.Thumbprint) {
        Write-Host "Already signed: $Path"
        return
    }
    $signed = $null
    if ($TimestampUrl) {
        try {
            $signed = Set-AuthenticodeSignature -FilePath $Path -Certificate $Cert -HashAlgorithm SHA256 `
                -TimestampServer $TimestampUrl -ErrorAction Stop
        } catch {
            Write-Host "Timestamp server not usable ($($_.Exception.Message)) - signing without it."
        }
    }
    if ($null -eq $signed) {
        $signed = Set-AuthenticodeSignature -FilePath $Path -Certificate $Cert -HashAlgorithm SHA256
    }
    $check = Get-AuthenticodeSignature -FilePath $Path
    if ($check.Status -ne 'Valid' -or -not $check.SignerCertificate -or
        $check.SignerCertificate.Thumbprint -ne $Cert.Thumbprint) {
        throw "Signature check failed for ${Path}: $($check.Status) - $($check.StatusMessage)"
    }
    Write-Host "Signed: $Path"
}

try {
    $cert = Find-Certificate
    if ($null -eq $cert) {
        Write-Host "Creating a self-signed code-signing certificate ($Subject)..."
        $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $Subject -FriendlyName $FriendlyName `
            -CertStoreLocation $Store -KeyExportPolicy Exportable -NotAfter (Get-Date).AddYears(5)
    }
    Write-Host "Certificate $($cert.Thumbprint), valid until $($cert.NotAfter.ToString('yyyy-MM-dd'))"

    $cerDir = Split-Path -Parent $CerPath
    if (-not (Test-Path -LiteralPath $cerDir)) {
        New-Item -ItemType Directory -Path $cerDir -Force | Out-Null
    }
    Export-Certificate -Cert $cert -FilePath $CerPath -Force | Out-Null

    if (-not (Get-ChildItem -Path "Cert:\CurrentUser\Root\$($cert.Thumbprint)" -ErrorAction SilentlyContinue)) {
        & certutil.exe -user -addstore Root $CerPath | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "certutil could not trust the certificate (exit code $LASTEXITCODE)" }
        Write-Host "Trusted the certificate for this user on this build PC (needed for the check below)."
    }

    foreach ($file in $Files) {
        Add-FileSignature -Path (Resolve-Path2 $file) -Cert $cert
    }

    foreach ($dir in $CopyCertificateTo) {
        $dest = Resolve-Path2 $dir
        if (-not (Test-Path -LiteralPath $dest)) {
            New-Item -ItemType Directory -Path $dest -Force | Out-Null
        }
        Copy-Item -LiteralPath $CerPath -Destination (Join-Path $dest 'PhotoBatchEditor.cer') -Force
        Write-Host "Certificate copied to: $dest"
    }

    Write-Host "Certificate for the target PC (install it once): $CerPath"
    exit 0
} catch {
    Write-Host "ERROR: $($_.Exception.Message)"
    exit 1
}
