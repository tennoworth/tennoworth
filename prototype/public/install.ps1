# wfm-fetch-inventory — Windows installer.
#
# Pipe idiom: `iwr <url> | iex`. Downloads the latest released binary
# from GitHub, places it under %LOCALAPPDATA%\wfminv, adds that to the
# user PATH, and verifies the SHA-256 checksum.

$ErrorActionPreference = 'Stop'

# Configure before publishing the first release.
$Repo  = if ($env:WFMINV_REPO) { $env:WFMINV_REPO } else { 'tennoworth/tennoworth' }
$Asset = 'wfm-fetch-inventory-windows-x86_64.exe'
$Dest  = Join-Path $env:LOCALAPPDATA 'wfminv'
$Bin   = Join-Path $Dest 'wfm-fetch-inventory.exe'

if (-not [Environment]::Is64BitOperatingSystem) {
    throw 'Only 64-bit Windows is supported.'
}

New-Item -ItemType Directory -Force -Path $Dest | Out-Null
$Tmp = New-TemporaryFile

# Fail honestly while the project is pre-release: a placeholder repo
# would otherwise surface as a baffling GitHub 404 mid-install.
if ($Repo -eq 'OWNER/REPO' -and -not $env:WFMINV_BASE_URL) {
    throw @'
No public release exists yet — this project hasn't been published to
GitHub, so there is no binary to download.
If you're the developer, build it locally instead:
    cd companion; cargo build --release
'@
}

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

# Authenticode check — advisory only. The SHA-256 match above is the actual
# integrity guarantee; an unsigned (or not-yet-trusted) binary must never
# block the install while releases are pre-signing. Status is anything but
# 'Valid' until the Certum cert lands (see docs/signing-runbook.md).
$sig = Get-AuthenticodeSignature $Bin
if ($sig.Status -eq 'Valid') {
    Write-Host "→ Authenticode signature: VALID"
    Write-Host "  Signed by: $($sig.SignerCertificate.Subject)"
} else {
    Write-Host "→ Authenticode signature: not present yet (status: $($sig.Status))"
}

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
  3. In a fresh PowerShell window, run:
       wfm-fetch-inventory
     No admin needed. Run it at the SAME elevation as Warframe — if you
     launched the game via Steam (the usual case), use a NORMAL (non-admin)
     PowerShell. An elevated terminal can't read a non-elevated game and
     fails with "Access is denied".
  4. inventory.json lands in the directory you ran it from — drop it into the web UI.

Optional — to create/edit warframe.market listings from the web app:
  5. wfm-fetch-inventory login     # once; interactive sign-in
  6. wfm-fetch-inventory serve     # leave this window open
     Paste the URL it prints into the app's Companion tab. The port is
     random (not the website's 5173).
'@

if ($sig.Status -eq 'Valid') {
    @'

Note: this build is code-signed and its signature verified above. Windows
SmartScreen may still warn until the publisher builds reputation — that is
expected for a new certificate and fades as more people install. If warned,
click "More info" -> "Run anyway".
'@
} else {
    @'

Note: the binary is not code-signed yet, so Windows SmartScreen may warn
"Windows protected your PC". Click "More info" -> "Run anyway". The
checksum was already verified above against the published SHA256SUMS.
'@
}
