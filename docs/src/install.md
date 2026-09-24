# Install

Every release on GitHub ships archives for Linux (x86_64, ARM64), macOS (Intel, Apple Silicon), and Windows (x86_64), each with a SHA-256 checksum, plus installers, `.deb` and `.rpm` packages, and a CycloneDX SBOM.

## Installers

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/kingdaswinx/Dexo/releases/latest/download/dexo-installer.sh | sh
```

```powershell
irm https://github.com/kingdaswinx/Dexo/releases/latest/download/dexo-installer.ps1 | iex
```

Both install `dexo` into Cargo's bin directory (`~/.cargo/bin`).

## Windows installer or portable

From the [latest release](https://github.com/kingdaswinx/Dexo/releases/latest):

- `dexo-x86_64-pc-windows-msvc.msi` installs Dexo under `C:\Program Files\dexo`, adds it to `PATH`, and appears in "Add or remove programs". A newer `.msi` replaces the older version.
- `dexo-x86_64-pc-windows-msvc.exe` is the portable build: download it and run it, nothing is installed.

Both are unsigned, so Windows SmartScreen may show "Windows protected your PC" when they are downloaded through a browser; choose **More info → Run anyway**.

## Linux packages

Download the `.deb` or `.rpm` for your architecture from the [latest release](https://github.com/kingdaswinx/Dexo/releases/latest), then:

```sh
sudo apt install ./dexo_*_amd64.deb      # Debian, Ubuntu
sudo dnf install ./dexo-*.x86_64.rpm     # Fedora
```

The packages need glibc 2.35 or later (Ubuntu 22.04, Debian 12, Fedora 36, or newer). They do not update themselves; install the next release the same way.

## From source

Rust 1.93 or later:

```sh
cargo install --locked --git https://github.com/kingdaswinx/Dexo dexo
```
