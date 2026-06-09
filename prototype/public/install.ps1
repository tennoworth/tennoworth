# wfm-fetch-inventory — Windows installer.
#
# Pipe idiom: `iwr <url> | iex`. Downloads the latest released binary
# from GitHub, places it under %LOCALAPPDATA%\wfminv, adds that to the
# user PATH, and verifies the SHA-256 checksum.

$ErrorActionPreference = 'Stop'

# Configure before publishing the first release.
$Repo  = if ($env:WFMINV_REPO) { $env:WFMINV_REPO } else { 'OWNER/REPO' }
$Asset = 'wfm-fetch-inventory-windows-x86_64.exe'
$Dest  = Join-Path $env:LOCALAPPDATA 'wfminv'
$Bin   = Join-Path $Dest 'wfm-fetch-inventory.exe'

if (-not [Environment]::Is64BitOperatingSystem) {
    throw 'Only 64-bit Windows is supported.'
}

New-Item -ItemType Directory -Force -Path $Dest | Out-Null
$Tmp = New-TemporaryFile

$BaseUrl = $env:WFMINV_BASE_URL
if ($BaseUrl) {
    # Plain http would let a MITM swap both binary and SHA256SUMS,
    # defeating verification below. Refuse non-https base URLs.
    if (-not $BaseUrl.StartsWith('https://')) {
        throw "WFMINV_BASE_URL must start with https:// (got: $BaseUrl)"
    }
    $Url     = "$BaseUrl/$Asset"
    $SumsUrl = "$BaseUrl/SHA256SUMS"
} else {
    $Url     = "https://github.com/$Repo/releases/latest/download/$Asset"
    $SumsUrl = "https://github.com/$Repo/releases/latest/download/SHA256SUMS"
}

Write-Host "→ Downloading $Asset"
try {
    Invoke-WebRequest -Uri $Url -OutFile $Tmp -UseBasicParsing
} catch {
    throw "Download failed. Check the release exists at $Url"
}

# Checksum — hard requirement. Missing or mismatched SHA256SUMS aborts.
# SECURITY.md commits to automatic verification; this is the path that
# implements it.
try {
    $sums = (Invoke-WebRequest -Uri $SumsUrl -UseBasicParsing).Content
} catch {
    throw "Could not fetch SHA256SUMS from $SumsUrl — refusing to install an unverified binary."
}
$expected = ($sums -split "`n" | Where-Object { $_ -match "  $Asset$" } |
    ForEach-Object { ($_ -split '\s+')[0] }) | Select-Object -First 1
if (-not $expected) {
    throw "SHA256SUMS exists but has no entry for $Asset. Aborting."
}
$actual = (Get-FileHash $Tmp -Algorithm SHA256).Hash.ToLower()
if ($expected.ToLower() -ne $actual) {
    throw "Checksum mismatch:`n  expected $expected`n  actual   $actual"
}
Write-Host '→ Checksum OK'

Move-Item -Force $Tmp $Bin
Write-Host "→ Installed: $Bin"

# Add to user PATH if not already there. This affects future shells only —
# the current one keeps the old PATH until restart.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not ($userPath -split ';' | Where-Object { $_ -eq $Dest })) {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$Dest", 'User')
    Write-Host "→ Added $Dest to user PATH (restart your shell to pick it up)"
}

@'

Next steps
  1. Start Warframe and log past the title screen.
  2. Open the trade or profile screen once (forces an auth call).
  3. In a fresh terminal, run:
       wfm-fetch-inventory
     (No admin needed — run as the same user that's running Warframe. If
      you see "Access is denied", try an elevated terminal.)
  4. inventory.json lands in your Downloads folder — drop it into the web UI.
'@
