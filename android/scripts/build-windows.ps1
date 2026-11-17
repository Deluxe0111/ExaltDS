param(
    [string]$Sdk = '',
    [string]$JavaHome = '',
    [string]$Ndk = '',
    [string]$CMake = '',
    [string]$OutputDirectory = '',
    [string]$BuildTools = '36.0.0',
    [string]$Platform = 'android-37.0',
    # zakuroishi (the library app) or exalt (ExaltDS: its own package, android/exalt manifest and resources)
    [ValidateSet('zakuroishi','exalt')][string]$Flavor = 'zakuroishi'
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
if (!$Sdk) { $Sdk = $env:ANDROID_HOME }
if (!$Sdk) { $Sdk = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Android/Sdk' }
if (!$JavaHome) { $JavaHome = 'C:/Program Files/Android/Android Studio/jbr' }
if (!(Test-Path "$JavaHome/bin/javac.exe")) { $JavaHome = $env:JAVA_HOME }
if (!$Ndk) { $Ndk = Join-Path $repo 'build/toolchain/android-ndk-r27c' }
if (!$CMake) { $CMake = Join-Path $repo 'build/toolchain/cmake-3.31.6-windows-x86_64/bin/cmake.exe' }
if (!$OutputDirectory) { $OutputDirectory = Join-Path $repo 'build/android-windows' }
$out = [IO.Path]::GetFullPath($OutputDirectory)
$ndkBin = Join-Path $Ndk 'toolchains/llvm/prebuilt/windows-x86_64/bin'
$tools = Join-Path $Sdk "build-tools/$BuildTools"
$androidJar = Join-Path $Sdk "platforms/$Platform/android.jar"
foreach ($file in @($CMake, "$Ndk/prebuilt/windows-x86_64/bin/make.exe", "$ndkBin/aarch64-linux-android30-clang.cmd", "$ndkBin/llvm-strip.exe",
    "$JavaHome/bin/javac.exe", "$JavaHome/bin/jar.exe", "$tools/aapt2.exe",
    "$tools/aapt.exe", "$tools/d8.bat", "$tools/zipalign.exe", "$tools/apksigner.bat", $androidJar)) {
    if (!(Test-Path -LiteralPath $file)) { throw "Missing tool: $file" }
}
$run = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8)
$work = Join-Path $out "work-$run"
New-Item -ItemType Directory -Force "$work/classes", "$work/dex", "$work/lib/arm64-v8a" | Out-Null
$log = Join-Path $out "build-$run.log"
$apk = Join-Path $out ($(if ($Flavor -eq 'exalt') { 'exaltds' } else { 'zakuro-android-test' }) + "-$run.apk")
$package = if ($Flavor -eq 'exalt') { 'io.github.exaltds' } else { 'io.github.zakuro.android' }
$launcherActivity = if ($Flavor -eq 'exalt') { 'io.github.zakuro.android.ExaltHome' } else { 'io.github.zakuro.android.Launcher' }
$appDir = if ($Flavor -eq 'exalt') { 'android/exalt' } else { 'android/app/src/main' }
function Invoke-Tool([string]$Exe, [string[]]$Arguments) {
    # Windows PowerShell 5.1 turns native stderr into terminating errors under Stop;
    # the exit code below is the real failure signal.
    $ErrorActionPreference = 'Continue'
    & $Exe @Arguments 2>&1 | ForEach-Object { "$_" } | Tee-Object -FilePath $log -Append | Out-Host
    if ($LASTEXITCODE -ne 0) {
        $failure = [Exception]::new("$Exe failed with exit $LASTEXITCODE; log: $log")
        $failure.Data['ExitCode'] = $LASTEXITCODE
        throw $failure
    }
}
$vars = @('JAVA_HOME','PATH','CARGO_TARGET_DIR','CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER',
    'CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS','CC_aarch64_linux_android','AR_aarch64_linux_android')
$previous = @{}
foreach ($name in $vars) { $previous[$name] = [Environment]::GetEnvironmentVariable($name,'Process') }
$oldLocation = Get-Location
$buildLock = $null
try {
    Set-Location $repo
    $env:JAVA_HOME = $JavaHome
    $env:PATH = "$JavaHome/bin;$ndkBin;$env:PATH"
    # GNU dlltool does not quote its temporary prefix when the target path has spaces.
    $env:CARGO_TARGET_DIR = Join-Path ([IO.Path]::GetTempPath()) 'zakuro-android-target'
    if ($env:CARGO_TARGET_DIR -match '\s') { throw 'Windows GNU build requires a TEMP path without spaces' }
    Write-Host "Cargo cache: $env:CARGO_TARGET_DIR"
    New-Item -ItemType Directory -Force $env:CARGO_TARGET_DIR | Out-Null
    try {
        $buildLock = [IO.FileStream]::new((Join-Path $env:CARGO_TARGET_DIR '.build.lock'),
            [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    } catch [IO.IOException] {
        throw "Cannot acquire Windows build lock at $env:CARGO_TARGET_DIR/.build.lock; another build may be running."
    }
    $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = "$ndkBin/aarch64-linux-android30-clang.cmd"
    $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS = '-C link-arg=-Wl,-z,max-page-size=16384'
    $env:CC_aarch64_linux_android = $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER
    $env:AR_aarch64_linux_android = "$ndkBin/llvm-ar.exe"
    $driverBuild = Join-Path $env:CARGO_TARGET_DIR 'adrenotools'
    # MinGW Makefiles uses an 8.3 compiler name, losing clang++'s C++ driver selection.
    Invoke-Tool $CMake @('-S',"$repo/android/third_party/libadrenotools",'-B',$driverBuild,'-G','MinGW Makefiles',
        "-DCMAKE_MAKE_PROGRAM=$Ndk/prebuilt/windows-x86_64/bin/make.exe",
        "-DCMAKE_TOOLCHAIN_FILE=$Ndk/build/cmake/android.toolchain.cmake",
        '-DANDROID_ABI=arm64-v8a','-DANDROID_PLATFORM=android-30','-DANDROID_STL=c++_static',
        '-DCMAKE_CXX_FLAGS=--driver-mode=g++',
        '-DBUILD_SHARED_LIBS=ON','-DCMAKE_BUILD_TYPE=Release','-DCMAKE_SHARED_LINKER_FLAGS=-Wl,-z,max-page-size=16384')
    Invoke-Tool $CMake @('--build',$driverBuild,'--parallel','4')
    foreach ($library in @('libadrenotools.so','libhook_impl.so','libmain_hook.so','libfile_redirect_hook.so','libgsl_alloc_hook.so')) {
        $found = @(Get-ChildItem $driverBuild -Recurse -File -Filter $library)
        if ($found.Count -ne 1) { throw "Missing or duplicate driver support library: $library" }
        Invoke-Tool "$ndkBin/llvm-strip.exe" @('-o',"$work/lib/arm64-v8a/$library",$found[0].FullName)
    }
    $lsfgBuild = Join-Path $env:CARGO_TARGET_DIR 'lsfg-bridge'
    Invoke-Tool $CMake @('-S',"$repo/android/third_party/lsfg",'-B',$lsfgBuild,'-G','MinGW Makefiles',
        "-DCMAKE_MAKE_PROGRAM=$Ndk/prebuilt/windows-x86_64/bin/make.exe",
        "-DCMAKE_TOOLCHAIN_FILE=$Ndk/build/cmake/android.toolchain.cmake",
        '-DANDROID_ABI=arm64-v8a','-DANDROID_PLATFORM=android-30','-DANDROID_STL=c++_static',
        '-DCMAKE_CXX_FLAGS=--driver-mode=g++',
        '-DCMAKE_BUILD_TYPE=Release','-DCMAKE_SHARED_LINKER_FLAGS=-Wl,-z,max-page-size=16384')
    Invoke-Tool $CMake @('--build',$lsfgBuild,'--parallel','8')
    Invoke-Tool "$ndkBin/llvm-strip.exe" @('-o',"$work/lib/arm64-v8a/liblsfg_bridge.so","$lsfgBuild/liblsfg_bridge.so")
    Invoke-Tool 'rustup' @('target','add','--toolchain','1.95.0','aarch64-linux-android')
    Invoke-Tool 'cargo' @('+1.95.0','build','--release','--locked','--target','aarch64-linux-android',
        '-p','zakuro-android','--lib','--features','zakuro-gpu/vulkan')
    Invoke-Tool "$ndkBin/llvm-strip.exe" @('-o',"$work/lib/arm64-v8a/libzakuro_android.so",
        "$env:CARGO_TARGET_DIR/aarch64-linux-android/release/libzakuro_android.so")
    $manifest = Join-Path $repo "$appDir/AndroidManifest.xml"
    $sources = @(Get-ChildItem "$repo/android/app/src/main/java" -Filter '*.java' -Recurse | Sort-Object FullName | ForEach-Object FullName)
    if (!$sources.Count) { throw 'No Java sources' }
    Invoke-Tool "$tools/aapt2.exe" @('compile','--dir',"$repo/$appDir/res",'-o',"$work/resources.zip")
    $resources = @("$work/resources.zip")
    # ExaltDS's telemetry address and key (telemetry_url, telemetry_key) live in a git-ignored local folder, never in the
    # repository; a build without it has no diagnostics at all
    $localRes = Join-Path $repo "$appDir/local-res"
    if ($Flavor -eq 'exalt' -and (Test-Path -LiteralPath $localRes)) {
        Invoke-Tool "$tools/aapt2.exe" @('compile','--dir',$localRes,'-o',"$work/local-resources.zip")
        $resources += "$work/local-resources.zip"
    }
    Invoke-Tool "$tools/aapt2.exe" (@('link','-o',"$work/unsigned.apk",'--manifest',$manifest,'-I',$androidJar) + $resources)
    Invoke-Tool "$JavaHome/bin/javac.exe" (@('-nowarn','--release','11','-classpath',$androidJar,'-d',"$work/classes") + $sources)
    # one jar, not every .class on the command line (too long for d8.bat once there are enough classes)
    Invoke-Tool "$JavaHome/bin/jar.exe" @('--create','--file',"$work/classes.jar",'-C',"$work/classes",'.')
    Invoke-Tool "$tools/d8.bat" @('--min-api','30','--lib',$androidJar,'--output',"$work/dex","$work/classes.jar")
    # Windows aapt2 emits backslashes in nested asset names; jar writes portable ZIP paths.
    New-Item -ItemType Directory -Force "$work/assets/licenses" | Out-Null
    foreach ($license in @('ADRENOTOOLS','LINKERNSBYPASS','LSFG','ZAKURO')) {
        Copy-Item -LiteralPath "$repo/android/app/src/main/assets/licenses/$license.txt" -Destination "$work/assets/licenses/$license.txt"
    }
    # ExaltDS's on-device compiler (android/exalt/clang-kit.py): programs and libraries for lib/,
    # headers and runtime in assets/clang-kit.zip, LLVM's license
    if ($Flavor -eq 'exalt') {
        Invoke-Tool 'python' @('-I',"$repo/android/exalt/clang-kit.py",$Ndk,"$work/kit","$repo/build/clang-kit-cache")
        Copy-Item -LiteralPath "$work/kit/assets/clang-kit.zip" -Destination "$work/assets/clang-kit.zip"
        Copy-Item -LiteralPath "$work/kit/assets/licenses/LLVM.txt" -Destination "$work/assets/licenses/LLVM.txt"
        Copy-Item -LiteralPath "$repo/crates/exalt-mods/NOTICE.md" -Destination "$work/assets/licenses/EXALT-MODS.txt"
    }
    Invoke-Tool "$JavaHome/bin/jar.exe" @('--update','--file',"$work/unsigned.apk",'--no-manifest',
        '--no-compress','-C',"$work/dex",'classes.dex','-C',$work,'lib','-C',$work,'assets')
    # the compiler's ~200 MB compress well; installed with extractNativeLibs, they need not be stored
    if ($Flavor -eq 'exalt') {
        Invoke-Tool "$JavaHome/bin/jar.exe" @('--update','--file',"$work/unsigned.apk",'--no-manifest','-C',"$work/kit",'lib')
    }
    Invoke-Tool "$tools/zipalign.exe" @('-P','16','-f','4',"$work/unsigned.apk","$work/aligned.apk")
    # The Windows fallback has its own local test identity; no Docker key is exported.
    $private = Join-Path $out 'private-signing'
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
    if (!(Test-Path $private)) {
        New-Item -ItemType Directory $private | Out-Null
        $acl = [Security.AccessControl.DirectorySecurity]::new()
        $acl.SetOwner($sid)
        $acl.SetAccessRuleProtection($true,$false)
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))
        Set-Acl -LiteralPath $private -AclObject $acl
    } else {
        $acl = Get-Acl -LiteralPath $private
        $otherUsers = @($acl.Access | Where-Object {
            $_.AccessControlType -eq 'Allow' -and
            $_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -ne $sid.Value
        })
        if (!$acl.AreAccessRulesProtected -or $otherUsers.Count) { throw 'Existing signing directory is not private to the current user' }
    }
    $key = Join-Path $private 'test.p12'
    if (!(Test-Path $key)) {
        Invoke-Tool "$JavaHome/bin/keytool.exe" @('-genkeypair','-storetype','PKCS12','-keystore',$key,
            '-storepass','android','-keypass','android','-alias','zakuro-android-test','-keyalg','RSA',
            '-keysize','4096','-validity','10000','-dname','CN=Zakuro Android Windows Test')
    }
    $release = Join-Path $private 'release.p12'
    $exaltRelease = Join-Path $private 'exaltds-release.p12'
    if ($Flavor -eq 'exalt' -and (Test-Path -LiteralPath $exaltRelease)) {
        # ExaltDS's own release key; android/exalt/signing/lineage.bin proves it descends from test.p12, so
        # test builds already installed update in place. Its password file is DPAPI-encrypted for this Windows user.
        $env:EXALT_KEYSTORE_PASSWORD = [Net.NetworkCredential]::new('', (Get-Content -LiteralPath (Join-Path $private 'exaltds-release.password.dpapi') | ConvertTo-SecureString)).Password
        try {
            Invoke-Tool "$tools/apksigner.bat" @('sign','--ks',$exaltRelease,'--ks-pass','env:EXALT_KEYSTORE_PASSWORD','--ks-key-alias','exaltds-release',
                '--lineage',"$repo/android/exalt/signing/lineage.bin",'--rotation-min-sdk-version','28','--v1-signing-enabled','false','--v2-signing-enabled','false','--out',$apk,"$work/aligned.apk")
        } finally { Remove-Item Env:EXALT_KEYSTORE_PASSWORD }
    } elseif (Test-Path -LiteralPath $release) {
        # The lineage proves this key descends from test.p12, so an installed app updates in place.
        # The password file is encrypted for this Windows user (DPAPI) and the value only travels through the environment.
        $env:ZAKURO_KEYSTORE_PASSWORD = [Net.NetworkCredential]::new('', (Get-Content -LiteralPath (Join-Path $private 'release.password.dpapi') | ConvertTo-SecureString)).Password
        try {
            Invoke-Tool "$tools/apksigner.bat" @('sign','--ks',$release,'--ks-pass','env:ZAKURO_KEYSTORE_PASSWORD','--ks-key-alias','zakuroishi-release',
                '--lineage',(Join-Path $PSScriptRoot '../signing/lineage.bin'),'--rotation-min-sdk-version','28','--v1-signing-enabled','false','--v2-signing-enabled','false','--out',$apk,"$work/aligned.apk")
        } finally { Remove-Item Env:ZAKURO_KEYSTORE_PASSWORD }
    } else {
        Invoke-Tool "$tools/apksigner.bat" @('sign','--ks',$key,'--ks-pass','pass:android',
            '--ks-key-alias','zakuro-android-test','--out',$apk,"$work/aligned.apk")
    }
    Invoke-Tool "$tools/apksigner.bat" @('verify','--verbose','--print-certs',$apk)
    Invoke-Tool "$tools/zipalign.exe" @('-c','-P','16','4',$apk)
    $badging = & "$tools/aapt.exe" dump badging $apk
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect APK' }
    $badging | Add-Content $log
    if (!($badging -match "package: name='$package'") -or
        !($badging -match "launchable-activity: name='$launcherActivity'")) { throw 'APK identity mismatch' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($apk)
    try {
        $entries = @($archive.Entries | ForEach-Object FullName)
        if ('lib/arm64-v8a/libzakuro_android.so' -notin $entries) { throw 'Native library missing' }
        $allowedAssets = @('assets/licenses/ADRENOTOOLS.txt','assets/licenses/LINKERNSBYPASS.txt','assets/licenses/LSFG.txt','assets/licenses/ZAKURO.txt')
        # ExaltDS's compiler kit (headers and runtime only: no game data)
        if ($Flavor -eq 'exalt') { $allowedAssets += @('assets/clang-kit.zip','assets/licenses/LLVM.txt','assets/licenses/EXALT-MODS.txt') }
        if (@($entries | Where-Object { $_ -match '^assets/' -and !$_.EndsWith('/') -and $_ -notin $allowedAssets }).Count) { throw 'Unexpected bundled assets' }
        if (@($entries | Where-Object { $_ -match '\.(dll|spv|dxbc)$' }).Count) { throw 'Shader or DLL files in the APK' }
        if ('lib/arm64-v8a/liblsfg_bridge.so' -notin $entries) { throw 'Frame generation bridge missing' }
        foreach ($library in @('libadrenotools.so','libhook_impl.so','libmain_hook.so','libfile_redirect_hook.so','libgsl_alloc_hook.so')) {
            if ("lib/arm64-v8a/$library" -notin $entries) { throw "Driver support missing: $library" }
        }
    } finally { $archive.Dispose() }
    $hash = (Get-FileHash $apk -Algorithm SHA256).Hash.ToLower()
    "$hash  $([IO.Path]::GetFileName($apk))" | Set-Content -Encoding ascii "$apk.sha256"
    Write-Host "APK: $apk"
    Write-Host "Log: $log"
} catch {
    Write-Host $_ -ForegroundColor Red
    $code = $_.Exception.Data['ExitCode']
    if (!$code) { $code = 1 }
    exit $code
} finally {
    if ($buildLock) { $buildLock.Dispose() }
    Set-Location $oldLocation
    foreach ($name in $vars) { [Environment]::SetEnvironmentVariable($name,$previous[$name],'Process') }
}
