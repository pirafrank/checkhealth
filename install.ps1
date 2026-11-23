# checkhealth install script for Windows (PowerShell 5.1+)
#
# Usage:
#   irm https://raw.githubusercontent.com/pirafrank/checkhealth/main/install.ps1 | iex
#
# Environment:
#   CHECKHEALTH_TAG          install this release tag (e.g. v0.1.0) instead of the latest
#   CHECKHEALTH_INSTALL_DIR  install here instead of %LOCALAPPDATA%\Programs\checkhealth
#
# The archive is verified against the release checksums manifest. Running it again
# overwrites the installed binary, so it can also be used to update.

# Everything runs inside a function so that the preference variables below do not leak into
# the caller's session when the script is run with `iex`.
function Install-Checkhealth {
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Owner = 'pirafrank'
$Repo = 'checkhealth'
$BinName = 'checkhealth'
$InstallDir = if ($env:CHECKHEALTH_INSTALL_DIR) { $env:CHECKHEALTH_INSTALL_DIR }
              else { Join-Path $env:LOCALAPPDATA "Programs\$BinName" }

# Targets must match matrix.jsonc.
$arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
switch ($arch) {
    'AMD64' { $Target = 'x86_64-pc-windows-msvc' }
    'ARM64' { $Target = 'aarch64-pc-windows-msvc' }
    default { throw "Unsupported architecture $arch" }
}
Write-Host "Target: $Target"

$Tag = $env:CHECKHEALTH_TAG
if (-not $Tag) {
    Write-Host 'Fetching latest version...'
    $release = Invoke-RestMethod "https://api.github.com/repos/$Owner/$Repo/releases/latest"
    $Tag = $release.tag_name
}
if (-not $Tag -or $Tag -notmatch '^[a-zA-Z0-9._-]+$' -or $Tag -eq '.' -or $Tag -eq '..') {
    throw "Invalid or missing release tag. Download manually from https://github.com/$Owner/$Repo/releases"
}
$Version = $Tag -replace '^v', ''
Write-Host "Selected release: $Tag"

# Asset names match release.yml: <name>-<version>-<target>.zip
$FileName = "$BinName-$Version-$Target.zip"
$BaseUrl = "https://github.com/$Owner/$Repo/releases/download/$Tag"

$TmpDir = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $TmpDir | Out-Null
try {
    $Archive = Join-Path $TmpDir $FileName
    Write-Host "Downloading $BaseUrl/$FileName..."
    Invoke-WebRequest "$BaseUrl/$FileName" -OutFile $Archive

    $Manifest = Join-Path $TmpDir "$BinName-$Version-checksums.txt"
    Invoke-WebRequest "$BaseUrl/$BinName-$Version-checksums.txt" -OutFile $Manifest
    $line = Get-Content $Manifest | Where-Object { $_ -match "  $([regex]::Escape($FileName))$" } | Select-Object -First 1
    $expected = if ($line) { ($line -split ' ')[0] } else { $null }
    $actual = (Get-FileHash $Archive -Algorithm SHA256).Hash
    if (-not $expected -or $expected -ne $actual.ToLower()) {
        throw "Checksum verification failed for $FileName."
    }
    Write-Host 'Checksum verified.'

    $Extract = Join-Path $TmpDir 'extract'
    Expand-Archive -Path $Archive -DestinationPath $Extract
    $Exe = Join-Path $Extract "$BinName.exe"
    if (-not (Test-Path $Exe)) { throw 'Binary not found in archive.' }

    Write-Host "Installing to $InstallDir..."
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item $Exe (Join-Path $InstallDir "$BinName.exe") -Force
}
finally {
    Remove-Item $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
}

# Add the install directory to the user PATH if missing.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$entries = if ($userPath) { $userPath -split ';' } else { @() }
if ($entries -notcontains $InstallDir) {
    $newPath = (@($entries | Where-Object { $_ }) + $InstallDir) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "Added $InstallDir to your user PATH. Restart your terminal to use it."
}

Write-Host "Successfully installed $BinName to $InstallDir"
Write-Host "Run $BinName --help to get started."
}

Install-Checkhealth
