<#
.SYNOPSIS
  Downloads a zenz model (by Miwa Keita, CC BY-SA 4.0) from Hugging Face
  and converts it for Sweet IME. Optional: without it Sweet IME converts
  with the dictionary lattice only.

  The result is written to %LOCALAPPDATA%\SweetIME\zenz.swz, which the IME
  picks up the next time it is activated. No administrator rights needed.

.PARAMETER Model
  Hugging Face repository. zenz-v3.1-xsmall (~100 MB, fastest) or
  zenz-v3.1-small (~380 MB, more accurate, ~4x slower).
#>
param(
    [string]$Model = 'Miwa-Keita/zenz-v3.1-xsmall',
    [string]$Out = "$env:LOCALAPPDATA\SweetIME\zenz.swz"
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$tool = @(
    "$PSScriptRoot\tools\sweet-zenz-pack.exe",
    "$PSScriptRoot\..\target\release\sweet-zenz-pack.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $tool) { throw 'sweet-zenz-pack.exe not found' }

Write-Host "The zenz model is licensed under CC BY-SA 4.0:"
Write-Host "  https://huggingface.co/$Model"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("sweet-zenz-" + [Guid]::NewGuid())
New-Item -ItemType Directory $tmp | Out-Null
try {
    foreach ($f in 'config.json', 'vocab.json', 'merges.txt', 'model.safetensors') {
        Write-Host "downloading $f"
        Invoke-WebRequest "https://huggingface.co/$Model/resolve/main/$f" -OutFile (Join-Path $tmp $f)
    }
    New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
    $staging = "$Out.new"
    & $tool $tmp $staging
    if ($LASTEXITCODE) { throw 'conversion failed' }
    if (Test-Path $Out) {
        # The IME may have the old file mapped; move it aside.
        $old = "$Out.$([DateTime]::Now.ToString('yyyyMMddHHmmss')).old"
        Move-Item $Out $old
        try { Remove-Item $old -ErrorAction Stop } catch { }
    }
    Move-Item $staging $Out
    Write-Host "installed $Out"
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
