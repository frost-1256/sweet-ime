<#
.SYNOPSIS
  Builds Sweet IME and stages a distributable tree in dist\.

  dist\x64\sweet_tsf.dll, dist\x86\sweet_tsf.dll, dist\system.dict,
  dist\sweet-settings.exe, dist\tools\sweet-zenz-pack.exe,
  dist\get-zenz.ps1, dist\licenses\*

  The zenz neural model is NOT bundled (it is CC BY-SA 4.0); users fetch
  it themselves with get-zenz.ps1.
#>
param(
    [string]$MozcData = "$PSScriptRoot\..\third_party\mozc-data\src\data\dictionary_oss"
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot\..").Path
$dist = Join-Path $root 'dist'
$env:Path += ";$env:USERPROFILE\.cargo\bin"

Push-Location $root
try {
    cargo build --locked --release -p sweet-tsf -p sweet-dictc -p sweet-zenz -p sweet-settings -p sweet-installer
    if ($LASTEXITCODE) { throw 'x64 build failed' }
    cargo build --locked --release -p sweet-tsf --target i686-pc-windows-msvc
    if ($LASTEXITCODE) { throw 'x86 build failed' }

    if (Test-Path -LiteralPath $dist) {
        $staging = Get-Item -LiteralPath $dist -Force
        if ($staging.FullName -ne $dist -or !$staging.PSIsContainer -or
            ($staging.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Invalid staging directory: $dist"
        }
        Remove-Item -LiteralPath $dist -Recurse -Force
    }
    New-Item -ItemType Directory -Force "$dist\x64", "$dist\x86", "$dist\tools", "$dist\licenses" | Out-Null
    Copy-Item target\release\sweet_tsf.dll "$dist\x64\"
    Copy-Item target\i686-pc-windows-msvc\release\sweet_tsf.dll "$dist\x86\"
    Copy-Item target\release\sweet-zenz-pack.exe "$dist\tools\"
    Copy-Item target\release\sweet-settings.exe "$dist\"
    Copy-Item scripts\get-zenz.ps1 "$dist\"

    & target\release\sweet-dictc.exe $MozcData "$dist\system.dict"
    if ($LASTEXITCODE) { throw 'dictionary build failed' }
    Copy-Item LICENSE "$dist\licenses\sweet-ime.txt"
    Copy-Item (Join-Path $MozcData 'README.txt') "$dist\licenses\dictionary-oss.txt"

    Get-ChildItem -Recurse $dist | Where-Object { -not $_.PSIsContainer } |
        Format-Table FullName, Length -AutoSize

    # Single-file graphical installer.
    New-Item -ItemType Directory -Force out | Out-Null
    & target\release\sweet-setup.exe --bundle $dist out\SweetIME-Setup.exe | Out-Host
    if ($LASTEXITCODE) { throw 'installer bundle failed' }
    & out\SweetIME-Setup.exe --verify | Out-Null
    if ($LASTEXITCODE) { throw 'installer verification failed' }
    Write-Host "Installer: $root\out\SweetIME-Setup.exe"
} finally {
    Pop-Location
}
