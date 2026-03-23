# Linux QA Environment — agentscommander

Guide to set up a Linux QA environment for building and testing agentscommander using Docker's webtop (Ubuntu XFCE desktop accessible via browser).

## Prerequisites

- **Docker Desktop** installed and running on the host machine (Windows/macOS)
- ~4 GB free disk space for the container image + build artifacts
- Internet connection for pulling images and dependencies

## 1. Launch the Webtop Container

```bash
docker run -d \
  --name agentscommander-linux-qa \
  --security-opt seccomp=unconfined \
  -e PUID=1000 \
  -e PGID=1000 \
  -e TZ=America/Argentina/Buenos_Aires \
  --shm-size="1gb" \
  -p 3000:3000 \
  -v agentscommander-qa-config:/config \
  --restart unless-stopped \
  lscr.io/linuxserver/webtop:ubuntu-xfce
```

| Parameter | Purpose |
|---|---|
| `--security-opt seccomp=unconfined` | Required for AppImage bundling and some build tools |
| `--shm-size="1gb"` | Prevents crashes in WebKit/browser processes |
| `-p 3000:3000` | Exposes the XFCE desktop at `http://localhost:3000` |
| `-v agentscommander-qa-config:/config` | Persistent volume for user data, cloned repos, and build cache |
| `PUID/PGID=1000` | Runs as non-root user `abc` inside the container |

After launching, access the desktop at **http://localhost:3000** in your browser.

The container runs Ubuntu 24.04 LTS (Noble Numbat) with XFCE desktop.

## 2. Install System Dependencies

Open a terminal inside the webtop (or `docker exec`) and run:

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential \
  curl \
  wget \
  git \
  pkg-config \
  libssl-dev \
  libwebkit2gtk-4.1-dev \
  librsvg2-dev \
  patchelf \
  file \
  libxdo-dev \
  libayatana-appindicator3-dev
```

> **Note:** `curl` and `git` are pre-installed in the webtop image but are included for completeness.

### Troubleshooting: libappindicator3 vs libayatana-appindicator3

On Ubuntu 24.04, `libappindicator3-dev` conflicts with `libayatana-appindicator3-dev`. The `libayatana-*` packages are the maintained fork and replacement. **Use `libayatana-appindicator3-dev` only** — do NOT install both.

If you see this error:
```
The following packages have unmet dependencies:
 libayatana-appindicator3-1 : Conflicts: libappindicator3-1
```
Remove `libappindicator3-dev` from the install command and use `libayatana-appindicator3-dev` instead.

### Troubleshooting: PackageKit Permission Denied

You may see this warning during apt operations:
```
Error: GDBus.Error:org.freedesktop.DBus.Error.Spawn.ExecFailed:
Failed to execute program org.freedesktop.PackageKit: Permission denied
```
This is **cosmetic only** and does not affect package installation. It occurs because the webtop container doesn't run the full PackageKit daemon. Safe to ignore.

## 3. Install Rust

Install Rust via rustup for the `abc` user:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

Verify:
```bash
rustc --version   # e.g., rustc 1.94.0
cargo --version   # e.g., cargo 1.94.0
```

## 4. Install Node.js 20+

```bash
# As root (or with sudo):
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo bash -
sudo apt-get install -y nodejs
```

Verify:
```bash
node --version   # e.g., v20.20.1
npm --version    # e.g., 10.8.2
```

## 5. Clone and Build

```bash
cd /config
git clone https://github.com/mblua/agentscommander.git
cd agentscommander

# Install frontend dependencies
npm install

# Quick type check (Rust only)
cd src-tauri
cargo check

# Full release build with all bundles
cd ..
npm run tauri build
```

### Build Outputs

A successful build produces three bundles in `src-tauri/target/release/bundle/`:

| Format | Path | Use Case |
|---|---|---|
| `.deb` | `bundle/deb/Agents Commander_x.y.z_amd64.deb` | Debian/Ubuntu install |
| `.rpm` | `bundle/rpm/Agents Commander-x.y.z-1.x86_64.rpm` | Fedora/RHEL install |
| `.AppImage` | `bundle/appimage/Agents Commander_x.y.z_amd64.AppImage` | Portable, no install needed |

### Build Times (Reference)

On the Docker webtop container (host: Windows 11, 16 GB RAM):
- `cargo check`: ~40 seconds (first run with dependency compilation)
- `npm run tauri build`: ~65 seconds (release profile)

Subsequent builds are significantly faster due to cached dependencies.

## 6. Testing the AppImage

To test the built app inside the webtop desktop:

```bash
chmod +x src-tauri/target/release/bundle/appimage/Agents\ Commander_*.AppImage
./src-tauri/target/release/bundle/appimage/Agents\ Commander_*.AppImage
```

The app should launch within the XFCE desktop visible at `http://localhost:3000`.

> **Note:** The `--security-opt seccomp=unconfined` flag is required for AppImage FUSE mounting to work inside Docker.

## Container Management

```bash
# Stop the container
docker stop agentscommander-linux-qa

# Start it again (data persists in the volume)
docker start agentscommander-linux-qa

# Remove container (volume data preserved)
docker rm agentscommander-linux-qa

# Remove container AND volume (full cleanup)
docker rm agentscommander-linux-qa
docker volume rm agentscommander-qa-config

# Shell into the container
docker exec -it agentscommander-linux-qa bash

# Shell as user abc (has Rust/cargo)
docker exec -it agentscommander-linux-qa su - abc
```

## Running from Windows (MSYS/Git Bash)

When running `docker exec` commands from Git Bash on Windows, MSYS automatically converts Unix paths to Windows paths, breaking commands. Use one of these workarounds:

```bash
# Option 1: Disable path conversion for all arguments
MSYS2_ARG_CONV_EXCL="*" docker exec agentscommander-linux-qa /some/command

# Option 2: Write a script inside the container, then execute it
docker exec agentscommander-linux-qa bash -c 'cat > /tmp/script.sh << "EOF"
#!/bin/bash
source /config/.cargo/env
cd /config/agentscommander
cargo check
EOF
chmod +x /tmp/script.sh'

MSYS2_ARG_CONV_EXCL="*" docker exec agentscommander-linux-qa su - abc -c "/tmp/script.sh"
```

## Dependency Summary

| Component | Version (tested) | Install Method |
|---|---|---|
| Container OS | Ubuntu 24.04.4 LTS | webtop image |
| Rust | 1.94.0 | rustup |
| Cargo | 1.94.0 | rustup |
| Node.js | 20.20.1 | NodeSource |
| npm | 10.8.2 | NodeSource |
| WebKit2GTK | 2.50.4 | apt (libwebkit2gtk-4.1-dev) |
| GTK3 | 3.24.41 | apt (dependency) |

## CI Comparison

The GitHub Actions CI (`release.yml`) builds on `ubuntu-22.04` with these deps:
```
libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
```

This QA environment uses Ubuntu 24.04 where `libappindicator3-dev` is replaced by `libayatana-appindicator3-dev`. Both provide the same functionality — ayatana is the actively maintained fork.
