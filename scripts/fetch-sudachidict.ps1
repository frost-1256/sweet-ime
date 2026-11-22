<#
Downloads the pinned SudachiDict lexicon sources into third_party\sudachidict
and checks their SHA-256. sweet-dictc takes the folder as its third argument.
Keep the version and hashes in step with nix/package.nix.
#>
param(
    [string]$Dest = "$PSScriptRoot\..\third_party\sudachidict"
)
$ErrorActionPreference = 'Stop'

$version = '20261015'
$files = [ordered]@{
    'small_lex.zip'   = '981b78cdc532ce058b34c76cd9acd3c9a6c771cc0ce528d7de92424b21172125'
    'core_lex.zip'    = 'e9ac06222f0ce401a0fef9b129e61e93d59e3ff395a1b037f498962ee5b8ad8b'
    'notcore_lex.zip' = '16072d75abe6a048947aacbcecfe1ab65a200965a9f1ddb7ceab366348fa21f2'
}

New-Item -ItemType Directory -Force $Dest | Out-Null
$stamp = Join-Path $Dest 'version.txt'
if ((Test-Path $stamp) -and (Get-Content $stamp -Raw).Trim() -eq $version -and
    -not ($files.Keys | Where-Object { -not (Test-Path (Join-Path $Dest ($_ -replace '\.zip$', '.csv'))) })) {
    Write-Host "SudachiDict $version is already in $Dest"
    return
}
foreach ($name in $files.Keys) {
    $zip = Join-Path $Dest $name
    $hash = if (Test-Path $zip) { (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() }
    if ($hash -ne $files[$name]) {
        Write-Host "Downloading $name"
        Invoke-WebRequest "https://sudachi.s3.ap-northeast-1.amazonaws.com/sudachidict-raw/v1/$version/$name" -OutFile $zip
        $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
        if ($hash -ne $files[$name]) {
            Remove-Item $zip
            throw "$name has SHA-256 $hash, expected $($files[$name])"
        }
    }
    Expand-Archive $zip -DestinationPath $Dest -Force
}
Set-Content $stamp $version
Write-Host "SudachiDict $version is in $Dest"
