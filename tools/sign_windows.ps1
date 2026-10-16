# Ky so (Authenticode, SHA256 + timestamp) cho exe / Setup / uninstaller cua ban Windows.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\sign_windows.ps1 -Files a.exe,b.exe
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\sign_windows.ps1 -ExportCertTo installer_output
#
# Chon chung chi theo thu tu:
#   1. $env:PBE_SIGN_PFX (+ $env:PBE_SIGN_PFX_PASSWORD): file .pfx chung chi code-signing mua tu CA
#      (Sectigo, Certum, SSL.com...). Day la cach DUY NHAT de may la khong bi SmartScreen hoi.
#   2. $env:PBE_SIGN_THUMBPRINT: chung chi da cai trong Cert:\CurrentUser\My (vd. USB token cua CA).
#   3. Mac dinh: chung chi TU KY "CN=Photo Batch Editor" (tao 1 lan, 5 nam, khoa rieng nam trong
#      Cert:\CurrentUser\My, khong ghi ra dia). May khac phai chay "Cai chung chi.bat" 1 lan
#      (cai .cer vao Trusted Root + Trusted Publishers) thi moi tin chu ky nay.
# Exit 0 = ok, exit 1 = loi (build dung lai).

[CmdletBinding()]
param(
    [string[]]$Files = @(),
    [string]$ExportCertTo = '',
    [string]$TimestampUrl = 'http://timestamp.digicert.com'
)

$ErrorActionPreference = 'Stop'

$Subject = 'CN=Photo Batch Editor'
$CodeSigningEku = '1.3.6.1.5.5.7.3.3'
$Store = 'Cert:\CurrentUser\My'
$CerName = 'PhotoBatchEditor.cer'

function Get-SelfSignedCert {
    $best = $null
    foreach ($c in Get-ChildItem -Path $Store) {
        if ($c.Subject -ne $Subject -or -not $c.HasPrivateKey) { continue }
        if ($c.NotAfter -lt (Get-Date).AddDays(30)) { continue }
        if (-not ($c.EnhancedKeyUsageList | Where-Object { $_.ObjectId -eq $CodeSigningEku })) { continue }
        if ($null -eq $best -or $c.NotAfter -gt $best.NotAfter) { $best = $c }
    }
    if ($null -eq $best) {
        Write-Host "Tao chung chi tu ky ($Subject)..."
        $best = New-SelfSignedCertificate -Type CodeSigningCert -Subject $Subject -FriendlyName 'Photo Batch Editor' `
            -CertStoreLocation $Store -KeyExportPolicy Exportable -KeyAlgorithm RSA -KeyLength 3072 `
            -HashAlgorithm SHA256 -NotAfter (Get-Date).AddYears(5)
    }
    # Tin chung chi tren may build (user hien tai) de kiem tra chu ky ben duoi la 'Valid'.
    if (-not (Test-Path "Cert:\CurrentUser\Root\$($best.Thumbprint)")) {
        $tmp = Join-Path $env:TEMP $CerName
        Export-Certificate -Cert $best -FilePath $tmp -Force | Out-Null
        & certutil.exe -user -f -addstore Root $tmp | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "certutil khong them duoc chung chi vao Root (exit $LASTEXITCODE)" }
        Remove-Item -LiteralPath $tmp -Force
    }
    return $best
}

function Get-SigningCert {
    if ($env:PBE_SIGN_PFX) {
        if (-not (Test-Path -LiteralPath $env:PBE_SIGN_PFX)) { throw "Khong thay PBE_SIGN_PFX: $env:PBE_SIGN_PFX" }
        $flags = [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::Exportable
        return New-Object System.Security.Cryptography.X509Certificates.X509Certificate2(
            $env:PBE_SIGN_PFX, $env:PBE_SIGN_PFX_PASSWORD, $flags)
    }
    if ($env:PBE_SIGN_THUMBPRINT) {
        $c = Get-ChildItem -Path "$Store\$($env:PBE_SIGN_THUMBPRINT)" -ErrorAction SilentlyContinue
        if (-not $c) { throw "Khong thay chung chi $env:PBE_SIGN_THUMBPRINT trong $Store" }
        return $c
    }
    return Get-SelfSignedCert
}

function Add-Signature([string]$Path, $Cert) {
    $Path = [System.IO.Path]::GetFullPath($Path)
    if (-not (Test-Path -LiteralPath $Path)) { throw "Khong thay file: $Path" }
    $signed = $null
    if ($TimestampUrl) {
        try {
            $signed = Set-AuthenticodeSignature -FilePath $Path -Certificate $Cert -HashAlgorithm SHA256 `
                -TimestampServer $TimestampUrl -IncludeChain NotRoot -ErrorAction Stop
        } catch {
            Write-Host "Timestamp server loi ($($_.Exception.Message)) - ky khong timestamp."
        }
    }
    if ($null -eq $signed -or $signed.Status -ne 'Valid') {
        $signed = Set-AuthenticodeSignature -FilePath $Path -Certificate $Cert -HashAlgorithm SHA256 -IncludeChain NotRoot
    }
    $check = Get-AuthenticodeSignature -FilePath $Path
    if ($check.Status -ne 'Valid' -or $check.SignerCertificate.Thumbprint -ne $Cert.Thumbprint) {
        throw "Ky that bai ${Path}: $($check.Status) - $($check.StatusMessage)"
    }
    Write-Host "Da ky: $Path"
}

try {
    $cert = Get-SigningCert
    foreach ($f in $Files) { Add-Signature -Path $f.Trim('"') -Cert $cert }
    if ($ExportCertTo) {
        if (-not (Test-Path -LiteralPath $ExportCertTo)) { New-Item -ItemType Directory -Path $ExportCertTo -Force | Out-Null }
        $out = Join-Path $ExportCertTo $CerName
        Export-Certificate -Cert $cert -FilePath $out -Force | Out-Null
        Write-Host "Chung chi cho may khac: $out"
    }
    exit 0
} catch {
    Write-Host "ERROR: $($_.Exception.Message)"
    exit 1
}
