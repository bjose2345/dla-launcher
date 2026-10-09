# Builds the DLA Launcher Windows installer (NSIS) on a Windows 10/11 PC.
#
# Needs, already installed: Visual Studio Build Tools 2022 with the C++
# workload and rustup. Rust is pinned through RUSTUP_TOOLCHAIN and installed by
# rustup when missing, so the machine's default toolchain is left alone. Node,
# pnpm and the Tauri CLI are installed at pinned versions into
# tauri2\.windows-tools, and Tauri downloads NSIS itself on the first build.
#
# Run from a copy of the repository after tauri2/scripts/fetch-cloudflared.sh
# and fetch-7zip.sh have filled tauri2\src-tauri\binaries:
#   powershell -ExecutionPolicy Bypass -File tauri2\scripts\build-windows.ps1
# Without state\tunnel\launcher-token.json the app builds but cannot reach DLA.

param([switch]$SkipTests)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$RustVersion = '1.99.0'
$NodeVersion = '26.10.0'
$NodeSha256 = '9fef7eca6743a6b910989cd8e78712376b394fcb9b6e1e9c44a0799a287f90c5'
$PnpmVersion = '11.28.5'
$TauriCliVersion = '2.12.1'
$Sidecars = @{
    'dla-cloudflared-x86_64-pc-windows-msvc.exe' = 'f096265ec2fcbe9bb6e2d64268db167ced3fcbb83d894bdb9e2fcdb26f2ea7e2'
    'dla-7zip-x86_64-pc-windows-msvc.exe' = '95fff19b73eff77c87c0d7bd32822e43fb90dd2932459f962eb532d8dfb6ba53'
    '7zip\windows\7z.dll' = '899cd73d7fc60e1c1c35c7de0f5bed3dc69bfe2cecbd48014d868a347970a0b1'
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$tauri = Join-Path $root 'tauri2'

function Invoke-Checked([string]$what, [scriptblock]$command) {
    Write-Host "==> $what"
    & $command
    if ($LASTEXITCODE -ne 0) { throw "$what failed (exit code $LASTEXITCODE)" }
}

foreach ($name in $Sidecars.Keys) {
    $path = Join-Path $tauri "src-tauri\binaries\$name"
    if (-not (Test-Path $path)) { throw "Missing $path; run the fetch scripts in tauri2/scripts on Linux and copy the binaries." }
    $hash = (Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $Sidecars[$name]) { throw "$name checksum mismatch: $hash" }
}

$token = Join-Path $root 'state\tunnel\launcher-token.json'
if (Test-Path $token) {
    Write-Host '==> Launcher token found; Install from DLA will reach DLA.'
} else {
    Write-Warning 'No state\tunnel\launcher-token.json: Install from DLA will be unavailable.'
}

$env:RUSTUP_TOOLCHAIN = $RustVersion
Invoke-Checked "Installing Rust $RustVersion" { rustup toolchain install $RustVersion --profile minimal --no-self-update }
$rustc = ((rustc --version) -split ' ')[1]
if ($rustc -ne $RustVersion) { throw "Rust $RustVersion is required, found $rustc" }

# Pinned build tools, local to this checkout. The .cmd shims also work where
# PowerShell's script policy blocks npm.ps1.
$tools = Join-Path $tauri '.windows-tools'
$node = Join-Path $tools "node-v$NodeVersion-win-x64"
if (-not (Test-Path (Join-Path $node 'node.exe'))) {
    New-Item -ItemType Directory -Force $tools | Out-Null
    $zip = Join-Path $tools "node-v$NodeVersion-win-x64.zip"
    Write-Host "==> Downloading Node $NodeVersion"
    Invoke-WebRequest "https://nodejs.org/dist/v$NodeVersion/node-v$NodeVersion-win-x64.zip" -OutFile $zip
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $NodeSha256) { Remove-Item $zip; throw "Node checksum mismatch: $hash" }
    Expand-Archive $zip -DestinationPath $tools
    Remove-Item $zip
}
$bin = Join-Path $tools 'node_modules\.bin'
$env:PATH = "$bin;$node;$env:PATH"
if (-not (Test-Path (Join-Path $bin 'pnpm.cmd')) -or -not (Test-Path (Join-Path $bin 'tauri.cmd'))) {
    Invoke-Checked 'Installing pnpm and the Tauri CLI' {
        npm.cmd install --prefix $tools --no-audit --no-fund --save-exact "pnpm@$PnpmVersion" "@tauri-apps/cli@$TauriCliVersion"
    }
}

Push-Location $root
try {
    # No TTY here: pnpm must not stop to ask before replacing a node_modules
    # that was installed under another path.
    Invoke-Checked 'Installing UI dependencies' { pnpm.cmd install --frozen-lockfile --config.confirmModulesPurge=false }
} finally {
    Pop-Location
}

Push-Location $tauri
try {
    Invoke-Checked 'Checking and testing the UI' {
        pnpm.cmd --dir ..\shared\ui typecheck
        if ($LASTEXITCODE -eq 0) { pnpm.cmd --dir ui typecheck }
        if ($LASTEXITCODE -eq 0) { pnpm.cmd --dir ..\shared\ui test }
    }
    if (-not $SkipTests) {
        Invoke-Checked 'Testing the Rust workspace' { cargo test --workspace }
    }
    $env:CI = 'true'
    # Only this build's installer may be left for the caller to pick up.
    Remove-Item (Join-Path $tauri 'target\release\bundle\nsis\*-setup.exe') -ErrorAction SilentlyContinue
    Invoke-Checked 'Building the installer' { tauri.cmd build --ci --bundles nsis }
} finally {
    Pop-Location
}

$installer = Get-ChildItem (Join-Path $tauri 'target\release\bundle\nsis\*-setup.exe') |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $installer) { throw 'The build finished but no installer was found.' }
$sha = (Get-FileHash $installer.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Host ''
Write-Host "Installer: $($installer.FullName)"
Write-Host ("Size:      {0:N1} MB" -f ($installer.Length / 1MB))
Write-Host "SHA-256:   $sha"
