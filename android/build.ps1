# Builds the Android input method APK without Gradle: the Rust library
# with cargo-ndk, then aapt2, javac, d8, zipalign and apksigner from the
# Android SDK. Output: out/SweetIME-Android.apk, signed with the key in
# SWEET_ANDROID_KEYSTORE (password in SWEET_ANDROID_KEYSTORE_PASS) or else
# the debug key.
#
#   powershell -ExecutionPolicy Bypass -File android/build.ps1 [-Install]
#
# Needs: ANDROID_HOME (default %LOCALAPPDATA%\Android\Sdk) with
# platforms;android-36, build-tools;36.0.0 and an NDK; a JDK 17+;
# cargo-ndk, the Android Rust targets and the Mozc sources under
# third_party/mozc-data. The pinned SudachiDict sources are fetched as needed.

param(
    [switch]$Install,
    [string]$Dictionary = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$app = Join-Path $PSScriptRoot 'app'
$build = Join-Path $root 'target\android'

$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$platform = Join-Path $sdk 'platforms\android-36\android.jar'
$tools = Join-Path $sdk 'build-tools\36.0.0'
if (-not $env:ANDROID_NDK_HOME) {
    $ndk = Get-ChildItem (Join-Path $sdk 'ndk') -Directory | Sort-Object Name | Select-Object -Last 1
    if (-not $ndk) { throw "no NDK under $sdk\ndk" }
    $env:ANDROID_NDK_HOME = $ndk.FullName
}
foreach ($p in @($platform, $tools)) { if (-not (Test-Path $p)) { throw "missing $p" } }

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$parts = ($version -split '[-+]', 2)[0].Split('.') | ForEach-Object { [int]$_ }
$versionCode = $parts[0] * 10000 + $parts[1] * 100 + $parts[2]

function Invoke-Tool {
    & $args[0] @($args | Select-Object -Skip 1)
    if ($LASTEXITCODE -ne 0) { throw "$($args[0]) failed ($LASTEXITCODE)" }
}

$dictSource = $null
$stagedDict = $null
if ($Dictionary) {
    if (-not (Test-Path -LiteralPath $Dictionary)) { throw "Dictionary not found: $Dictionary" }
    $dictSource = (Resolve-Path -LiteralPath $Dictionary).Path
    $buildPrefix = $build.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if ($dictSource.StartsWith($buildPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        $stagedDict = Join-Path ([IO.Path]::GetTempPath()) ("sweet-ime-android-dict-" + [Guid]::NewGuid() + '.dict')
        Copy-Item -LiteralPath $dictSource -Destination $stagedDict
        $dictSource = $stagedDict
    }
}

try {
    if (Test-Path -LiteralPath $build) {
        $staging = Get-Item -LiteralPath $build -Force
        if ($staging.FullName -ne $build -or !$staging.PSIsContainer -or
            ($staging.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Invalid staging directory: $build"
        }
        Remove-Item -LiteralPath $build -Recurse -Force
    }
    New-Item -ItemType Directory -Force "$build\lib", "$build\assets\licenses", "$build\res\mipmap-xxxhdpi", "$build\gen", "$build\classes", "$build\dex" | Out-Null

    Write-Host '== System dictionary'
    if ($dictSource) {
        Copy-Item -LiteralPath $dictSource "$build\assets\system.dict"
    } else {
        & "$root\scripts\build-dictionary.ps1" -Output "$build\assets\system.dict"
    }
} finally {
    if ($stagedDict -and (Test-Path -LiteralPath $stagedDict)) {
        Remove-Item -LiteralPath $stagedDict -Force -ErrorAction SilentlyContinue
    }
}

Write-Host '== Rust library'
$env:CARGO_PROFILE_RELEASE_DEBUG = '0'
$env:CARGO_PROFILE_RELEASE_STRIP = 'symbols'
Push-Location $root
try { Invoke-Tool cargo ndk -t arm64-v8a -t x86_64 -P 29 -o "$build\lib" build --locked --release -p sweet-android }
finally { Pop-Location }

Write-Host '== Resources'
Copy-Item "$root\assets\licenses\sudachidict.txt" "$build\assets\licenses\"
Copy-Item (Join-Path $root 'assets\icon\sweet-ime-256.png') "$build\res\mipmap-xxxhdpi\ic_launcher.png"
$aapt2 = Join-Path $tools 'aapt2.exe'
Invoke-Tool $aapt2 compile --dir (Join-Path $app 'res') -o "$build\res-app.zip"
Invoke-Tool $aapt2 compile --dir "$build\res" -o "$build\res-icon.zip"
Invoke-Tool $aapt2 link -o "$build\base.apk" -I $platform `
    --manifest (Join-Path $app 'AndroidManifest.xml') `
    --min-sdk-version 29 --target-sdk-version 36 `
    --version-code $versionCode --version-name $version `
    -A "$build\assets" -0 dict `
    --java "$build\gen" "$build\res-app.zip" "$build\res-icon.zip"

Write-Host '== Java'
$sources = @(Get-ChildItem -Recurse (Join-Path $app 'src') -Filter *.java) + @(Get-ChildItem -Recurse "$build\gen" -Filter *.java) | ForEach-Object { $_.FullName }
$javac = if ($env:JAVA_HOME -and (Test-Path "$env:JAVA_HOME\bin\javac.exe")) { "$env:JAVA_HOME\bin\javac.exe" } else { 'javac' }
Invoke-Tool $javac --release 17 -encoding UTF-8 -classpath $platform -d "$build\classes" @sources
$classes = Get-ChildItem -Recurse "$build\classes" -Filter *.class | ForEach-Object { $_.FullName }
Invoke-Tool (Join-Path $tools 'd8.bat') --release --min-api 29 --lib $platform --output "$build\dex" @classes

Write-Host '== Package'
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::Open("$build\base.apk", 'Update')
try {
    [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, "$build\dex\classes.dex", 'classes.dex') | Out-Null
    foreach ($so in Get-ChildItem -Recurse "$build\lib" -Filter *.so) {
        $abi = $so.Directory.Name
        [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $so.FullName, "lib/$abi/$($so.Name)") | Out-Null
    }
} finally { $zip.Dispose() }

Invoke-Tool (Join-Path $tools 'zipalign.exe') -f -p 4 "$build\base.apk" "$build\aligned.apk"

$out = Join-Path $root 'out'
New-Item -ItemType Directory -Force $out | Out-Null
$apk = Join-Path $out 'SweetIME-Android.apk'
$signer = Join-Path $tools 'apksigner.bat'
# A local copy of the release key, if there is one.
$localKey = Join-Path $env:USERPROFILE '.android\sweet-ime-release.jks'
$localPass = Join-Path $env:USERPROFILE '.android\sweet-ime-release.pass'
if (-not $env:SWEET_ANDROID_KEYSTORE -and (Test-Path $localKey) -and (Test-Path $localPass)) {
    $env:SWEET_ANDROID_KEYSTORE = $localKey
    $env:SWEET_ANDROID_KEYSTORE_PASS = (Get-Content -Raw $localPass).Trim()
}
if ($env:SWEET_ANDROID_KEYSTORE -and $env:SWEET_ANDROID_KEYSTORE_PASS) {
    # The release key (CI secrets): updates keep the same signature.
    Invoke-Tool $signer sign --ks $env:SWEET_ANDROID_KEYSTORE --ks-pass env:SWEET_ANDROID_KEYSTORE_PASS --out $apk "$build\aligned.apk"
} else {
    $keystore = Join-Path $env:USERPROFILE '.android\debug.keystore'
    if (-not (Test-Path $keystore)) {
        New-Item -ItemType Directory -Force (Split-Path $keystore) | Out-Null
        # The standard Android debug key (well-known password).
        $keytool = @("$env:JAVA_HOME\bin\keytool.exe") + @(Get-ChildItem "$env:ProgramFiles\Java\*\bin\keytool.exe" -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | ForEach-Object FullName) + @('keytool') | Where-Object { $_ -eq 'keytool' -or (Test-Path $_) } | Select-Object -First 1
        Invoke-Tool $keytool -genkeypair -keystore $keystore -storepass android -keypass android `
            -alias androiddebugkey -keyalg RSA -keysize 2048 -validity 10000 -dname 'CN=Android Debug,O=Android,C=US'
    }
    Invoke-Tool $signer sign --ks $keystore --ks-pass pass:android --out $apk "$build\aligned.apk"
}
Write-Host "APK: $apk ($([math]::Round((Get-Item $apk).Length / 1MB, 1)) MB)"

if ($Install) {
    Invoke-Tool adb install -r $apk
}
