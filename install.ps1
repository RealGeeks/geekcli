<#
.SYNOPSIS
  Install the geekcli CLI on Windows from its GitHub release archives.

  irm https://raw.githubusercontent.com/realgeeks/geekcli/main/install.ps1 | iex

  `iex` runs the script with no arguments, so -Version and -InstallDir reach it
  only from the environment, or by turning the download into a scriptblock:

    $env:GEEKCLI_VERSION = 'v0.3.0'
    irm https://raw.githubusercontent.com/realgeeks/geekcli/main/install.ps1 | iex

    & ([scriptblock]::Create((irm https://raw.githubusercontent.com/realgeeks/geekcli/main/install.ps1))) -Version v0.3.0

  Environment / parameters:
    -Version vX.Y.Z        GEEKCLI_VERSION       a tag; default: the latest release
    -InstallDir DIR        GEEKCLI_INSTALL_DIR   default: %LOCALAPPDATA%\Programs\geekcli
    GEEKCLI_REPO                                 default: realgeeks/geekcli
    GH_TOKEN / GITHUB_TOKEN                      optional; avoids GitHub's anonymous
                                                 API rate limit

  Verifies the sha256 the Release workflow publishes beside each archive and
  adds the install directory to the user PATH.
#>
[CmdletBinding()]
param(
  [string]$Version = $env:GEEKCLI_VERSION,
  [string]$InstallDir = $env:GEEKCLI_INSTALL_DIR
)
$ErrorActionPreference = 'Stop'

$repo = if ($env:GEEKCLI_REPO) { $env:GEEKCLI_REPO } else { 'realgeeks/geekcli' }
$token = if ($env:GH_TOKEN) { $env:GH_TOKEN } elseif ($env:GITHUB_TOKEN) { $env:GITHUB_TOKEN } else { $null }
$headers = @{ 'Accept' = 'application/vnd.github+json'; 'User-Agent' = 'geekcli-install' }
if ($token) { $headers['Authorization'] = "Bearer $token" }

$arch = if ([System.Environment]::Is64BitOperatingSystem) { 'x86_64' } else { throw 'geekcli needs 64-bit Windows' }
$target = "$arch-pc-windows-msvc"

if ($Version) {
  if (-not $Version.StartsWith('v')) { $Version = "v$Version" }
  $releaseUrl = "https://api.github.com/repos/$repo/releases/tags/$Version"
} else {
  $releaseUrl = "https://api.github.com/repos/$repo/releases/latest"
}
try {
  $release = Invoke-RestMethod -Uri $releaseUrl -Headers $headers
} catch {
  throw "cannot read the release ($releaseUrl). If GitHub is rate limiting you, set GH_TOKEN. $_"
}
$tag = $release.tag_name
$name = "geekcli-$tag-$target"
$archive = "$name.zip"

function Get-Asset([string]$assetName, [string]$dest) {
  if ($token) {
    $asset = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
    if (-not $asset) { throw "release $tag has no asset $assetName" }
    $h = $headers.Clone(); $h['Accept'] = 'application/octet-stream'
    Invoke-WebRequest -Uri $asset.url -Headers $h -OutFile $dest
  } else {
    Invoke-WebRequest -Uri "https://github.com/$repo/releases/download/$tag/$assetName" -Headers @{ 'User-Agent' = 'geekcli-install' } -OutFile $dest
  }
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("geekcli-install-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  Write-Host "Downloading geekcli $tag for $target..."
  Get-Asset $archive (Join-Path $tmp $archive)
  Get-Asset "$archive.sha256" (Join-Path $tmp "$archive.sha256")

  $expected = ((Get-Content (Join-Path $tmp "$archive.sha256") -Raw) -split '\s+')[0].ToLower()
  $actual = (Get-FileHash (Join-Path $tmp $archive) -Algorithm SHA256).Hash.ToLower()
  if ($expected -ne $actual) { throw "checksum mismatch for $archive (expected $expected, got $actual)" }

  Expand-Archive -Path (Join-Path $tmp $archive) -DestinationPath $tmp -Force
  $exe = Join-Path $tmp "$name\geekcli.exe"
  if (-not (Test-Path $exe)) { throw "$archive did not contain $name\geekcli.exe" }

  if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\geekcli' }
  New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
  Copy-Item $exe (Join-Path $InstallDir 'geekcli.exe') -Force

  $installed = & (Join-Path $InstallDir 'geekcli.exe') --version
  Write-Host "Installed $InstallDir\geekcli.exe ($installed)"

  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (($userPath -split ';') -notcontains $InstallDir) {
    [Environment]::SetEnvironmentVariable('Path', "$InstallDir;$userPath", 'User')
    $env:Path = "$InstallDir;$env:Path"
    Write-Host "Added $InstallDir to your PATH (open a new terminal to pick it up)."
  }
  Write-Host 'Next: geekcli auth login --site www.yoursite.com   (then: geekcli guide)'
} finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
