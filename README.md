# launcher-sw-multi
Tauri-based multi-OS launcher based on Tajir's windows launcher for Siege Worlds game

## Signed Windows build

This repository is now set up with:

- GitHub Actions workflow: `.github/workflows/windows-signed-build.yml`
- Local signing script: `scripts/sign-windows-artifacts.ps1`
- Default PFX location: `src-tauri/certs/codesign.pfx`

### Where to put your passphrase

Local builds use environment variable:

- `WINDOWS_CODESIGN_PASSWORD`
- Example file: `signing.env.example` (copy to `signing.env`)

GitHub Actions uses repository secrets:

- `WINDOWS_CODESIGN_PFX_BASE64` (base64 of your `codesign.pfx`)
- `WINDOWS_CODESIGN_PASSWORD` (your PFX passphrase)

### Local Windows signing flow

1. Copy your cert to `src-tauri/certs/codesign.pfx`
2. Set passphrase in PowerShell:

	```powershell
	$env:WINDOWS_CODESIGN_PASSWORD = "your-passphrase"
	```

3. Build installers:

	```powershell
	cd src-tauri
	cargo tauri build --bundles nsis,msi
	cd ..
	```

4. Sign + verify:

	```powershell
	./scripts/sign-windows-artifacts.ps1
	```
