param(
    [Parameter(Mandatory)] [string]$HeadersSdk,
    [Parameter(Mandatory)] [string]$MavericksSdk,
    [Parameter(Mandatory)] [string]$LlvmBin
)

$ErrorActionPreference = 'Stop'
$headers = (Resolve-Path -LiteralPath $HeadersSdk).Path
$sdk = (Resolve-Path -LiteralPath $MavericksSdk).Path
$llvm = (Resolve-Path -LiteralPath $LlvmBin).Path
$suffix = if ($IsWindows) { '.exe' } else { '' }
$clang = Join-Path $llvm "clang$suffix"
$linker = Join-Path $llvm "ld64.lld$suffix"
$objdump = Join-Path $llvm "llvm-objdump$suffix"
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root 'target/mavericks'
New-Item -ItemType Directory -Force -Path $output | Out-Null

Push-Location $root
try {
    # Keep the stock Rust target explicit. The final link uses the 10.9 SDK.
    $priorTarget = $env:MACOSX_DEPLOYMENT_TARGET
    try {
        $env:MACOSX_DEPLOYMENT_TARGET = '10.12'
        cargo build --locked --release -p sweet-macos --target x86_64-apple-darwin --target-dir $output
        if ($LASTEXITCODE -ne 0) { throw 'Rust archive build failed' }
    } finally {
        $env:MACOSX_DEPLOYMENT_TARGET = $priorTarget
    }
    foreach ($source in @('main', 'candidate_panel')) {
        & $clang -target x86_64-apple-macosx10.9 -isysroot $headers -DSWEET_MAVERICKS -fobjc-arc `
            -Wall -Wextra -Wno-unused-parameter -Werror=unguarded-availability `
            -c "macos/$source.m" -o "$output/$source.o"
        if ($LASTEXITCODE -ne 0) { throw "Compilation failed for $source" }
    }
    & $clang -target x86_64-apple-macosx10.9 -isysroot $sdk -Wall -Wextra -Werror `
        -c macos/mavericks_compat.c -o "$output/compat.o"
    if ($LASTEXITCODE -ne 0) { throw 'Compatibility source compilation failed' }

    $binary = "$output/SweetIME"
    & $linker -arch x86_64 -platform_version macos 10.9 10.9 -syslibroot $sdk -dead_strip `
        -o $binary "$output/main.o" "$output/candidate_panel.o" "$output/compat.o" `
        "$output/x86_64-apple-darwin/release/libsweet_macos.a" `
        -framework AppKit -framework Carbon -framework InputMethodKit -framework QuartzCore -lSystem -liconv
    if ($LASTEXITCODE -ne 0) { throw 'Mavericks SDK link failed' }
    $commands = & $objdump --macho --private-headers $binary
    if ($LASTEXITCODE -ne 0 -or ($commands -join "`n") -notmatch 'LC_VERSION_MIN_MACOSX\s+cmdsize\s+\d+\s+version\s+10\.9\s') {
        throw 'Expected a macOS 10.9 minimum in the linked Mach-O'
    }
    Write-Output $binary
} finally {
    Pop-Location
}
