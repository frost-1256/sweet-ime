param(
    [Parameter(Mandatory = $true)]
    [string]$Output,
    [string]$MozcData = "$PSScriptRoot/../third_party/mozc-data/src/data/dictionary_oss"
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/..").Path
$outputPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)
$mozc = (Resolve-Path -LiteralPath $MozcData).Path
$sudachi = Join-Path $root 'third_party/sudachidict'

& "$PSScriptRoot\fetch-sudachidict.ps1" -Dest $sudachi
New-Item -ItemType Directory -Force (Split-Path -Parent $outputPath) | Out-Null
Push-Location $root
try {
    cargo run --locked --release -p sweet-dictc -- $mozc $outputPath $sudachi
    if ($LASTEXITCODE -ne 0) { throw 'dictionary build failed' }
} finally {
    Pop-Location
}
