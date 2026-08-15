# Install

Install from a GitHub Release archive, the shell installer, or the PowerShell installer produced by cargo-dist.

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/kingdaswinx/Dexo/releases/download/v1.1.0/dexo-installer.sh | sh
```

```powershell
irm https://github.com/kingdaswinx/Dexo/releases/download/v1.1.0/dexo-installer.ps1 | iex
```

Homebrew and MSI ship when a tap and WiX publisher are configured; archives always ship.
