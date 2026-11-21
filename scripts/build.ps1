<#
.SYNOPSIS
  Builds Sweet IME and stages a distributable tree in dist\.

  dist\x64\sweet_tsf.dll, dist\x86\sweet_tsf.dll, dist\system.dict,
  dist\zenz.swz (optional), dist\licenses\*
#>
param(
    [string]$MozcData = "$PSScriptRoot\..\third_party\mozc-data\src\data\dictionary_oss",
    [string]$ZenzModel = "$PSScriptRoot\..\third_party\zenz\v3.1-xsmall"
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path "$PSScriptRoot\.."
$dist = Join-Path $root 'dist'
$env:Path += ";$env:USERPROFILE\.cargo\bin"

Push-Location $root
try {
    cargo build --release -p sweet-tsf -p sweet-dictc
    if ($LASTEXITCODE) { throw 'x64 build failed' }
    cargo build --release -p sweet-tsf --target i686-pc-windows-msvc
    if ($LASTEXITCODE) { throw 'x86 build failed' }
    cargo build --release -p sweet-zenz --example zenz_pack
    if ($LASTEXITCODE) { throw 'zenz_pack build failed' }

    New-Item -ItemType Directory -Force "$dist\x64", "$dist\x86", "$dist\licenses" | Out-Null
    Copy-Item target\release\sweet_tsf.dll "$dist\x64\"
    Copy-Item target\i686-pc-windows-msvc\release\sweet_tsf.dll "$dist\x86\"

    & target\release\sweet-dictc.exe $MozcData "$dist\system.dict"
    if ($LASTEXITCODE) { throw 'dictionary build failed' }
    Copy-Item (Join-Path $MozcData 'README.txt') "$dist\licenses\dictionary-oss.txt"

    if (Test-Path (Join-Path $ZenzModel 'model.safetensors')) {
        & target\release\examples\zenz_pack.exe $ZenzModel "$dist\zenz.swz"
        if ($LASTEXITCODE) { throw 'zenz pack failed' }
        @"
zenz.swz is converted from the zenz model by Miwa Keita
(https://huggingface.co/Miwa-Keita), licensed under CC BY-SA 4.0
(https://creativecommons.org/licenses/by-sa/4.0/). The weights were
re-packed (BF16 -> F32, transposed embedding added); no other changes.
"@ | Set-Content -Encoding utf8 "$dist\licenses\zenz.txt"
    } else {
        Write-Warning "zenz model not found at $ZenzModel; building without neural reranking"
    }
    Get-ChildItem -Recurse $dist | Where-Object { -not $_.PSIsContainer } |
        Format-Table FullName, Length -AutoSize
} finally {
    Pop-Location
}
