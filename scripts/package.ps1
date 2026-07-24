# Packages an already-built dodo.exe into a release ZIP.
#
#   pwsh scripts/package.ps1 [-Target <triple>] [-Version <v>] [-Out <dir>]
#                            [-Profile <name>]
#
# Produces, under -Out (default: dist\):
#
#   dodo-v<version>-windows-<arch>.zip
#   dodo-v<version>-windows-<arch>.zip.sha256
#
# The Unix side is scripts/package.sh. This is a separate script rather than a
# bash one run under Git Bash because Compress-Archive is the only archiver
# guaranteed to be present on a windows-latest runner, and because ZIP is what
# Windows users can open without installing anything.
#
# UNVERIFIED: this project is developed on macOS and has never been run on a
# Windows machine or a Windows runner. Treat it as a starting point.

[CmdletBinding()]
param(
    [string]$Target = "",
    [string]$Version = "",
    [string]$Out = "",
    [string]$Profile = "release"
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrEmpty($Out)) { $Out = Join-Path $repoRoot "dist" }

# Host triple when -Target was not given; `rustc -vV` is the same authority the
# bash script uses.
if ([string]::IsNullOrEmpty($Target)) {
    $Target = (rustc -vV | Select-String '^host: ').ToString().Split(' ')[1]
}

# Version comes from Cargo.toml's [package] section, the source of truth for
# archive names.
if ([string]::IsNullOrEmpty($Version)) {
    $inPackage = $false
    foreach ($line in Get-Content (Join-Path $repoRoot "Cargo.toml")) {
        if ($line -match '^\[package\]') { $inPackage = $true; continue }
        if ($line -match '^\[') { $inPackage = $false }
        if ($inPackage -and $line -match '^version\s*=\s*"([^"]+)"') {
            $Version = $Matches[1]
            break
        }
    }
}
if ([string]::IsNullOrEmpty($Version)) { throw "could not read version from Cargo.toml" }

switch -Wildcard ($Target) {
    "aarch64-*" { $arch = "arm64" }
    "x86_64-*"  { $arch = "x64" }
    default     { throw "unsupported architecture for packaging: $Target" }
}
if ($Target -notlike "*-pc-windows-*") { throw "package.ps1 only packages Windows targets: $Target" }

# cargo puts a --target build under target\<triple>\<profile>\ and a host build
# under target\<profile>\.
$bin = Join-Path $repoRoot "target\$Target\$Profile\dodo.exe"
if (-not (Test-Path $bin)) { $bin = Join-Path $repoRoot "target\$Profile\dodo.exe" }
if (-not (Test-Path $bin)) { throw "no dodo.exe found; run: cargo build --profile $Profile --locked" }

$name = "dodo-v$Version-windows-$arch"
$stage = Join-Path $Out ".stage\$name"

if (Test-Path (Join-Path $Out ".stage")) { Remove-Item -Recurse -Force (Join-Path $Out ".stage") }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
New-Item -ItemType Directory -Force -Path $Out | Out-Null

Copy-Item $bin (Join-Path $stage "dodo.exe")
# NTFS has no executable bit, so nothing to preserve here — the .exe extension
# is what makes it runnable. Mentioned because the Unix script has to chmod.
foreach ($doc in @("README.md", "LICENSE", "LICENSE.md", "LICENSE.txt")) {
    $p = Join-Path $repoRoot $doc
    if (Test-Path $p) { Copy-Item $p $stage }
}

$archive = Join-Path $Out "$name.zip"
if (Test-Path $archive) { Remove-Item -Force $archive }
Compress-Archive -Path $stage -DestinationPath $archive -CompressionLevel Optimal

$hash = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLower()
# Same two-space `<sha>  <file>` layout `shasum -c` expects, so every platform's
# checksum file is verified the same way.
"$hash  $name.zip" | Out-File -Encoding ascii -NoNewline (Join-Path $Out "$name.zip.sha256")
Add-Content -Path (Join-Path $Out "$name.zip.sha256") -Value ""

Remove-Item -Recurse -Force (Join-Path $Out ".stage")
Write-Output "packaged $archive"

# --- Future: Windows code signing ------------------------------------------
#
# Not implemented; needs a certificate this repository does not have. When it
# does (secrets WINDOWS_CERTIFICATE, WINDOWS_CERTIFICATE_PWD), sign the .exe
# BEFORE it is zipped, roughly:
#
#   $pfx = [IO.Path]::GetTempFileName()
#   [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
#   signtool sign /f $pfx /p $env:WINDOWS_CERTIFICATE_PWD `
#       /tr http://timestamp.digicert.com /td sha256 /fd sha256 $bin
#
# An MSI (WiX / `cargo-wix`) would be built from the signed .exe at this point
# too; see "Future readiness" in docs/release.md.
