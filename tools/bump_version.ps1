# Moi lan build = 1 version moi: tang "build" trong version.json, in ra version day du (1.0.0.12).
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\bump_version.ps1           # 1.0.0.11 -> 1.0.0.12
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\bump_version.ps1 -Print    # chi in, khong tang
#
# Version phat hanh (X.Y.Z) sua bang tay trong version.json; "build" khong bao gio reset ve 0
# de version 4 so (X.Y.Z.build) cua installer luon tang -> Windows/Inno Setup nang cap ban cu.

[CmdletBinding()]
param([switch]$Print)

$ErrorActionPreference = 'Stop'
$path = Join-Path (Split-Path -Parent $PSScriptRoot) 'version.json'

try {
    $data = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    $parts = @("$($data.version)".Split('.') | Select-Object -First 3 | ForEach-Object { [int]$_ })
    while ($parts.Count -lt 3) { $parts += 0 }
    $build = [int]$data.build
    if (-not $Print) {
        $build += 1
        $json = "{`n  `"version`": `"$($data.version)`",`n  `"build`": $build`n}`n"
        [System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
    }
    Write-Output ("{0}.{1}.{2}.{3}" -f $parts[0], $parts[1], $parts[2], $build)
    exit 0
} catch {
    [Console]::Error.WriteLine("ERROR: version.json: $($_.Exception.Message)")
    exit 1
}
