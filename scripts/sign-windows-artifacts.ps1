param(
  [string]$PfxPath = "src-tauri/certs/codesign.pfx",
  [string]$BundleDir = "src-tauri/target/release/bundle",
  [string]$TimestampUrl = "http://timestamp.digicert.com",
  [string]$Passphrase = $env:WINDOWS_CODESIGN_PASSWORD
)

if (-not (Test-Path $PfxPath)) {
  throw "PFX file not found at $PfxPath"
}

if ([string]::IsNullOrWhiteSpace($Passphrase)) {
  throw "Passphrase missing. Set WINDOWS_CODESIGN_PASSWORD or pass -Passphrase."
}

$signtool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter signtool.exe |
  Where-Object { $_.FullName -match "\\x64\\signtool.exe$" } |
  Sort-Object FullName -Descending |
  Select-Object -First 1

if (-not $signtool) {
  throw "signtool.exe not found. Install Windows SDK Signing Tools."
}

$artifacts = Get-ChildItem $BundleDir -Recurse -File |
  Where-Object { $_.Extension -in @('.exe', '.msi') }

if (-not $artifacts) {
  throw "No .exe or .msi artifacts found under $BundleDir"
}

foreach ($artifact in $artifacts) {
  Write-Host "Signing $($artifact.FullName)"
  & $signtool.FullName sign /fd SHA256 /tr $TimestampUrl /td SHA256 /f $PfxPath /p $Passphrase "$($artifact.FullName)"
  if ($LASTEXITCODE -ne 0) {
    throw "Signing failed for $($artifact.FullName)"
  }

  & $signtool.FullName verify /pa /v "$($artifact.FullName)"
  if ($LASTEXITCODE -ne 0) {
    throw "Verification failed for $($artifact.FullName)"
  }
}

Write-Host "All artifacts were signed and verified."
