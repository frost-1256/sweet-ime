<#
.SYNOPSIS
  Installs Sweet IME from dist\ into Program Files and registers the TSF
  text service (64-bit and 32-bit). Must run elevated.

.PARAMETER AddToLanguageList
  Also add Sweet IME to the current user's Japanese keyboard list.
#>
param(
    [string]$Source = "$PSScriptRoot\..\dist",
    [string]$Target = "$env:ProgramFiles\SweetIME",
    [switch]$AddToLanguageList
)
$ErrorActionPreference = 'Stop'
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'install.ps1 must be run as Administrator'
}
$Source = Resolve-Path $Source

foreach ($arch in 'x64', 'x86') {
    New-Item -ItemType Directory -Force "$Target\$arch" | Out-Null
    $dll = "$Target\$arch\sweet_tsf.dll"
    if (Test-Path $dll) {
        # A loaded DLL cannot be overwritten but can be renamed.
        $old = "$dll.$([DateTime]::Now.ToString('yyyyMMddHHmmss')).old"
        Move-Item $dll $old
    }
    Copy-Item "$Source\$arch\sweet_tsf.dll" $dll
}
foreach ($f in 'system.dict', 'sweet-settings.exe', 'get-zenz.ps1', 'tools\sweet-zenz-pack.exe') {
    if (Test-Path "$Source\$f") {
        $dst = "$Target\$f"
        New-Item -ItemType Directory -Force (Split-Path $dst) | Out-Null
        if (Test-Path $dst) {
            $old = "$dst.$([DateTime]::Now.ToString('yyyyMMddHHmmss')).old"
            Move-Item $dst $old
        }
        Copy-Item "$Source\$f" $dst
    }
}
Copy-Item -Recurse -Force "$Source\licenses" "$Target\"
# Remove stale copies that are no longer in use.
Get-ChildItem -Recurse $Target -Filter '*.old' | ForEach-Object {
    try { Remove-Item $_.FullName -ErrorAction Stop } catch { }
}

& "$env:windir\System32\regsvr32.exe" /s "$Target\x64\sweet_tsf.dll"
if ($LASTEXITCODE) { throw "regsvr32 (x64) failed: $LASTEXITCODE" }
& "$env:windir\SysWOW64\regsvr32.exe" /s "$Target\x86\sweet_tsf.dll"
if ($LASTEXITCODE) { throw "regsvr32 (x86) failed: $LASTEXITCODE" }

if ($AddToLanguageList) {
    $tip = '0411:{7E1D3C2A-5B8F-4C1E-9A3D-2F6B8C4E1A01}{7E1D3C2A-5B8F-4C1E-9A3D-2F6B8C4E1A02}'
    $list = Get-WinUserLanguageList
    $ja = $list | Where-Object LanguageTag -eq 'ja'
    if (-not $ja) {
        $list.Add('ja')
        $ja = $list | Where-Object LanguageTag -eq 'ja'
    }
    if (-not ($ja.InputMethodTips -contains $tip)) {
        $ja.InputMethodTips.Add($tip)
        Set-WinUserLanguageList $list -Force
    }
}
# Start menu shortcut to the settings app (all users). This file is saved
# as UTF-8 with BOM so Windows PowerShell 5.1 reads the name correctly.
# WScript.Shell saves through the ANSI code page, so save under an ASCII
# name and rename (Unicode-safe) afterwards.
$programs = "$env:ProgramData\Microsoft\Windows\Start Menu\Programs"
$lnk = Join-Path $programs 'Sweet IME の設定.lnk'
$tmp = Join-Path $programs 'SweetIME-settings.lnk'
$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($tmp)
$shortcut.TargetPath = "$Target\sweet-settings.exe"
$shortcut.WorkingDirectory = $Target
$shortcut.Save()
Move-Item -Force $tmp $lnk

Write-Host "Sweet IME installed to $Target"
Write-Host "Settings (including the optional AI model download): Start menu > Sweet IME の設定"
