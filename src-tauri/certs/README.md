Place your code-signing certificate here as:

codesign.pfx

This path is used by:
- scripts/sign-windows-artifacts.ps1 (default PFX path)

Do not commit the PFX to git.
Use environment variable WINDOWS_CODESIGN_PASSWORD for the passphrase.
