<#
.SYNOPSIS
  Unregisters Sweet IME and removes it from the user's language list.
  Files in Program Files are removed when not in use. Must run elevated.
#>
param([string]$Target = "$env:ProgramFiles\SweetIME")
$ErrorActionPreference = 'Stop'
$tip = '0411:{7E1D3C2A-5B8F-4C1E-9A3D-2F6B8C4E1A01}{7E1D3C2A-5B8F-4C1E-9A3D-2F6B8C4E1A02}'
$list = Get-WinUserLanguageList
foreach ($l in $list) {
    if ($l.InputMethodTips -contains $tip) { [void]$l.InputMethodTips.Remove($tip) }
}
Set-WinUserLanguageList $list -Force
if (Test-Path "$Target\x64\sweet_tsf.dll") {
    & "$env:windir\System32\regsvr32.exe" /s /u "$Target\x64\sweet_tsf.dll"
}
if (Test-Path "$Target\x86\sweet_tsf.dll") {
    & "$env:windir\SysWOW64\regsvr32.exe" /s /u "$Target\x86\sweet_tsf.dll"
}
try {
    Remove-Item -Recurse -Force $Target -ErrorAction Stop
} catch {
    Write-Warning "Some files are still in use; they can be deleted after sign-out: $Target"
}
Write-Host 'Sweet IME uninstalled'
